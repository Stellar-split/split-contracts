//! Issue #771: invoice funding goal tiers with unlock rewards.
//!
//! Tiers are validated and stored at creation. After every `pay`, tiers whose
//! `threshold_bps` of the goal has been reached are marked unlocked and a
//! `TierUnlocked` event is emitted once per newly crossed tier:
//! topics `(split, tier_unl, invoice_id)`, data
//! `(tier_index, threshold_bps, funded_amount)`.
//!
//! Note: the check hooks the standard `pay` path only.

use crate::error::ContractError;
use crate::types::FundingTier;
use crate::{SplitContract, SplitContractClient};
use soroban_sdk::{contractimpl, contracttype, panic_with_error, symbol_short, Env, Vec};

/// Maximum number of tiers per invoice.
pub const MAX_TIERS: u32 = 4;
/// Maximum reward description length in bytes.
pub const MAX_TIER_DESCRIPTION_LEN: u32 = 64;

/// Persistent storage keys owned by this module (issue #771).
#[contracttype]
#[derive(Clone)]
pub enum TierKey {
    /// `Vec<FundingTier>` configured for an invoice.
    Tiers(u64),
    /// `Vec<u32>` indices of unlocked tiers.
    Unlocked(u64),
}

/// Validate and persist tiers at invoice creation (no-op for `None`/empty).
pub(crate) fn apply_tiers(env: &Env, invoice_id: u64, tiers: Option<Vec<FundingTier>>) {
    let tiers = match tiers {
        Some(t) if !t.is_empty() => t,
        _ => return,
    };
    if tiers.len() > MAX_TIERS {
        panic_with_error!(env, ContractError::InvalidFundingTiers);
    }
    let mut prev = 0u32;
    for t in tiers.iter() {
        if t.threshold_bps <= prev
            || t.threshold_bps > 10_000
            || t.reward_description.len() > MAX_TIER_DESCRIPTION_LEN
        {
            panic_with_error!(env, ContractError::InvalidFundingTiers);
        }
        prev = t.threshold_bps;
    }
    env.storage()
        .persistent()
        .set(&TierKey::Tiers(invoice_id), &tiers);
}

/// Unlock and announce every not-yet-unlocked tier crossed by `funded`.
pub(crate) fn check_tiers(env: &Env, invoice_id: u64, funded: i128, total: i128) {
    let tiers: Vec<FundingTier> = match env.storage().persistent().get(&TierKey::Tiers(invoice_id))
    {
        Some(t) => t,
        None => return,
    };
    if total <= 0 {
        return;
    }
    let mut unlocked: Vec<u32> = env
        .storage()
        .persistent()
        .get(&TierKey::Unlocked(invoice_id))
        .unwrap_or_else(|| Vec::new(env));
    let mut changed = false;
    for (i, tier) in tiers.iter().enumerate() {
        let i = i as u32;
        if unlocked.contains(i) {
            continue;
        }
        if funded.saturating_mul(10_000) >= (tier.threshold_bps as i128).saturating_mul(total) {
            unlocked.push_back(i);
            changed = true;
            env.events().publish(
                (symbol_short!("split"), symbol_short!("tier_unl"), invoice_id),
                (i, tier.threshold_bps, funded),
            );
        }
    }
    if changed {
        env.storage()
            .persistent()
            .set(&TierKey::Unlocked(invoice_id), &unlocked);
    }
}

#[contractimpl]
impl SplitContract {
    /// Indices of the funding tiers already unlocked for an invoice.
    pub fn get_unlocked_tiers(env: Env, invoice_id: u64) -> Vec<u32> {
        env.storage()
            .persistent()
            .get(&TierKey::Unlocked(invoice_id))
            .unwrap_or_else(|| Vec::new(&env))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{client, default_options};
    use soroban_sdk::testutils::{Address as _, Events as _};
    use soroban_sdk::token::StellarAssetClient;
    use soroban_sdk::{Address, Bytes, Env, Symbol, TryFromVal, Val};

    fn tier(env: &Env, bps: u32) -> FundingTier {
        FundingTier {
            threshold_bps: bps,
            reward_description: Bytes::from_slice(env, b"reward"),
        }
    }

    fn tier_events(env: &Env) -> u32 {
        let want = Symbol::new(env, "tier_unl");
        env.events()
            .all()
            .iter()
            .filter(|(_c, topics, _d)| {
                topics
                    .get(1)
                    .and_then(|v: Val| Symbol::try_from_val(env, &v).ok())
                    == Some(want.clone())
            })
            .count() as u32
    }

    fn create(
        env: &Env,
        c: &SplitContractClient,
        token: &Address,
        bps: &[u32],
    ) -> Result<u64, ()> {
        let mut opts = default_options(env);
        let mut v = Vec::new(env);
        for b in bps {
            v.push_back(tier(env, *b));
        }
        opts.ext.tiers = Some(v);
        let mut r = Vec::new(env);
        r.push_back(Address::generate(env));
        let mut a = Vec::new(env);
        a.push_back(1_000i128);
        c.try_create_invoice(&Address::generate(env), &r, &a, token, &9_999u64, &opts)
            .map(|x| x.unwrap())
            .map_err(|_| ())
    }

    fn pay(env: &Env, c: &SplitContractClient, token: &Address, id: u64, amt: i128) {
        let payer = Address::generate(env);
        StellarAssetClient::new(env, token).mint(&payer, &amt);
        c.pay(&payer, &id, &amt, &0u64, &false, &false, &None);
    }

    #[test]
    fn cross_first_then_multiple_without_reemit() {
        let (env, cid, token) = crate::test::setup_initialized();
        let c = client(&env, &cid);
        let id = create(&env, &c, &token, &[2_500, 5_000, 7_500]).unwrap();
        pay(&env, &c, &token, id, 300);
        assert_eq!(tier_events(&env), 1);
        assert_eq!(c.get_unlocked_tiers(&id).len(), 1);
        // 300 -> 800 crosses tiers 1 and 2 but not tier 0 again.
        pay(&env, &c, &token, id, 500);
        assert_eq!(tier_events(&env), 2);
        assert_eq!(c.get_unlocked_tiers(&id).len(), 3);
        pay(&env, &c, &token, id, 50);
        assert_eq!(tier_events(&env), 0);
    }

    #[test]
    fn invalid_tier_order_rejected() {
        let (env, cid, token) = crate::test::setup_initialized();
        let c = client(&env, &cid);
        assert!(create(&env, &c, &token, &[5_000, 2_500]).is_err());
        assert!(create(&env, &c, &token, &[2_500, 2_500]).is_err());
        assert!(create(&env, &c, &token, &[1, 2, 3, 4, 5]).is_err());
    }
}
