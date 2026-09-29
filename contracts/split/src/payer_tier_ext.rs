//! Issue #817: advanced rate limiting with per-payer tiers.
//!
//! Admins assign payers to a numeric tier and configure a per-tier
//! `max_payments` that overrides the global invoice rate limit
//! (`set_rate_limit`) for payers in that tier. The sliding window length is
//! still the global `window_ledgers`. A tier limit of `0` means unlimited.

use super::*;
use soroban_sdk::{contractimpl, symbol_short, Address, Env, Symbol};

fn tier_key(payer: &Address) -> (Symbol, Address) {
    (symbol_short!("pay_tier"), payer.clone())
}

fn tier_limit_key(tier: u32) -> (Symbol, u32) {
    (symbol_short!("tier_lim"), tier)
}

/// Effective `max_payments` for `payer`: the tier override if one is set,
/// otherwise `default`.
pub(crate) fn max_payments_for(env: &Env, payer: &Address, default: u32) -> u32 {
    let tier: Option<u32> = env.storage().persistent().get(&tier_key(payer));
    match tier {
        Some(t) => env
            .storage()
            .instance()
            .get(&tier_limit_key(t))
            .unwrap_or(default),
        None => default,
    }
}

#[contractimpl]
impl SplitContract {
    /// Assign `payer` to `tier` (admin only).
    pub fn set_payer_tier(env: Env, admin: Address, payer: Address, tier: u32) {
        require_admin_role(&env, &admin, AdminRole::Operator);
        env.storage().persistent().set(&tier_key(&payer), &tier);
        env.events().publish(
            (symbol_short!("split"), symbol_short!("tier_set"), payer),
            tier,
        );
    }

    /// Set the max payments per rate-limit window for `tier` (admin only; 0 = unlimited).
    pub fn set_tier_rate_limit(env: Env, admin: Address, tier: u32, max_payments: u32) {
        require_admin_role(&env, &admin, AdminRole::Operator);
        env.storage()
            .instance()
            .set(&tier_limit_key(tier), &max_payments);
        env.events().publish(
            (symbol_short!("split"), symbol_short!("tier_lim"), tier),
            max_payments,
        );
    }

    pub fn get_payer_tier(env: Env, payer: Address) -> Option<u32> {
        env.storage().persistent().get(&tier_key(&payer))
    }

    pub fn get_tier_rate_limit(env: Env, tier: u32) -> Option<u32> {
        env.storage().instance().get(&tier_limit_key(tier))
    }
}

#[cfg(test)]
mod tests {
    use crate::ext_test_util::{fixture, mint, new_invoice, pay};
    use soroban_sdk::{testutils::Address as _, Address};

    #[test]
    fn tier_limit_overrides_global_limit() {
        let f = fixture();
        f.c.set_rate_limit(&f.admin, &100, &1);
        let (id, _, _) = new_invoice(&f, 1_000);
        let vip = Address::generate(&f.env);
        mint(&f.env, &f.token, &vip, 1_000);
        f.c.set_tier_rate_limit(&f.admin, &1, &3);
        f.c.set_payer_tier(&f.admin, &vip, &1);
        assert_eq!(f.c.get_payer_tier(&vip), Some(1));
        assert_eq!(f.c.get_tier_rate_limit(&1), Some(3));
        pay(&f, &vip, id, 10);
        pay(&f, &vip, id, 10);
        pay(&f, &vip, id, 10);
    }

    #[test]
    #[should_panic(expected = "RateLimitExceeded")]
    fn tier_limit_enforced() {
        let f = fixture();
        f.c.set_rate_limit(&f.admin, &100, &5);
        let (id, _, _) = new_invoice(&f, 1_000);
        let basic = Address::generate(&f.env);
        mint(&f.env, &f.token, &basic, 1_000);
        f.c.set_tier_rate_limit(&f.admin, &0, &1);
        f.c.set_payer_tier(&f.admin, &basic, &0);
        pay(&f, &basic, id, 10);
        pay(&f, &basic, id, 10);
    }

    #[test]
    #[should_panic(expected = "RateLimitExceeded")]
    fn untiered_payer_uses_global_limit() {
        let f = fixture();
        f.c.set_rate_limit(&f.admin, &100, &1);
        let (id, _, _) = new_invoice(&f, 1_000);
        let p = Address::generate(&f.env);
        mint(&f.env, &f.token, &p, 1_000);
        assert_eq!(f.c.get_payer_tier(&p), None);
        pay(&f, &p, id, 10);
        pay(&f, &p, id, 10);
    }
}
