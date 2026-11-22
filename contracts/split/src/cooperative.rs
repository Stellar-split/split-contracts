//! Issue #890: Creator cooperative profit sharing.
//!
//! Creators form a cooperative with fixed member shares (basis points summing
//! to 10_000). Anyone may deposit profit into a cooperative; each deposit is
//! immediately credited to members pro rata and members claim at will.

use crate::SplitContract;
#[allow(unused_imports)]
use crate::{SplitContractArgs, SplitContractClient};
use soroban_sdk::{contractimpl, contracttype, symbol_short, token, Address, Env, Vec};

/// Maximum members per cooperative (bounds per-deposit work).
const MAX_COOP_MEMBERS: u32 = 50;

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum CoopKey {
    NextId,
    Coop(u64),
    /// Claimable balance per (coop, token, member): i128
    Claimable(u64, Address, Address),
    /// Total ever deposited per (coop, token): i128
    Deposited(u64, Address),
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Cooperative {
    pub id: u64,
    pub founder: Address,
    pub members: Vec<Address>,
    pub shares_bps: Vec<u32>,
}

fn load_coop(env: &Env, coop_id: u64) -> Cooperative {
    env.storage()
        .persistent()
        .get(&CoopKey::Coop(coop_id))
        .expect("cooperative not found")
}

#[contractimpl]
impl SplitContract {
    /// Create a cooperative. `shares_bps` must align with `members`, contain
    /// no zero entries, and sum to exactly 10_000. Returns the cooperative id.
    pub fn create_cooperative(
        env: Env,
        founder: Address,
        members: Vec<Address>,
        shares_bps: Vec<u32>,
    ) -> u64 {
        founder.require_auth();
        assert!(
            !members.is_empty() && members.len() <= MAX_COOP_MEMBERS,
            "invalid member count"
        );
        assert!(members.len() == shares_bps.len(), "members/shares length mismatch");
        let mut total: u32 = 0;
        for (i, share) in shares_bps.iter().enumerate() {
            assert!(share > 0, "share must be positive");
            total += share;
            let m = members.get(i as u32).unwrap();
            for j in 0..i as u32 {
                assert!(members.get(j).unwrap() != m, "duplicate member");
            }
        }
        assert!(total == 10_000, "shares must sum to 10000 bps");

        let id: u64 = env.storage().persistent().get(&CoopKey::NextId).unwrap_or(1u64);
        env.storage().persistent().set(&CoopKey::NextId, &(id + 1));
        let coop = Cooperative {
            id,
            founder: founder.clone(),
            members,
            shares_bps,
        };
        env.storage().persistent().set(&CoopKey::Coop(id), &coop);
        env.events().publish(
            (symbol_short!("split"), symbol_short!("coop_new"), id),
            (founder, coop.members.len()),
        );
        id
    }

    /// Deposit profit into a cooperative; it is credited to members by share.
    /// Rounding dust goes to the last member so credits always sum to `amount`.
    pub fn deposit_coop_profit(
        env: Env,
        from: Address,
        coop_id: u64,
        token: Address,
        amount: i128,
    ) {
        from.require_auth();
        assert!(amount > 0, "amount must be positive");
        let coop = load_coop(&env, coop_id);
        token::Client::new(&env, &token).transfer(
            &from,
            &env.current_contract_address(),
            &amount,
        );
        let last = coop.members.len() - 1;
        let mut credited: i128 = 0;
        for (i, member) in coop.members.iter().enumerate() {
            let i = i as u32;
            let part = if i == last {
                amount - credited
            } else {
                amount * coop.shares_bps.get(i).unwrap() as i128 / 10_000
            };
            credited += part;
            let key = CoopKey::Claimable(coop_id, token.clone(), member);
            let cur: i128 = env.storage().persistent().get(&key).unwrap_or(0);
            env.storage().persistent().set(&key, &(cur + part));
        }
        let dkey = CoopKey::Deposited(coop_id, token.clone());
        let dep: i128 = env.storage().persistent().get(&dkey).unwrap_or(0);
        env.storage().persistent().set(&dkey, &(dep + amount));
        env.events().publish(
            (symbol_short!("split"), symbol_short!("coop_dep"), coop_id),
            (from, token, amount),
        );
    }

    /// Claim a member's accumulated profit in `token`. Returns the amount paid.
    pub fn claim_coop_profit(env: Env, member: Address, coop_id: u64, token: Address) -> i128 {
        member.require_auth();
        let coop = load_coop(&env, coop_id);
        assert!(coop.members.contains(member.clone()), "not a cooperative member");
        let key = CoopKey::Claimable(coop_id, token.clone(), member.clone());
        let amount: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        assert!(amount > 0, "nothing to claim");
        env.storage().persistent().set(&key, &0i128);
        token::Client::new(&env, &token).transfer(
            &env.current_contract_address(),
            &member,
            &amount,
        );
        env.events().publish(
            (symbol_short!("split"), symbol_short!("coop_clm"), coop_id),
            (member, token, amount),
        );
        amount
    }

    /// Fetch a cooperative.
    pub fn get_cooperative(env: Env, coop_id: u64) -> Cooperative {
        load_coop(&env, coop_id)
    }

    /// Unclaimed profit for a member in `token`.
    pub fn get_coop_claimable(env: Env, coop_id: u64, token: Address, member: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&CoopKey::Claimable(coop_id, token, member))
            .unwrap_or(0)
    }

    /// Total profit ever deposited into a cooperative in `token`.
    pub fn get_coop_total_deposited(env: Env, coop_id: u64, token: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&CoopKey::Deposited(coop_id, token))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
    use soroban_sdk::vec;

    fn setup() -> (Env, SplitContractClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(SplitContract, ());
        let token = env
            .register_stellar_asset_contract_v2(Address::generate(&env))
            .address();
        (env.clone(), SplitContractClient::new(&env, &id), token)
    }

    #[test]
    fn test_deposit_splits_by_share_and_claim() {
        let (env, c, token) = setup();
        let (a, b, payer) = (
            Address::generate(&env),
            Address::generate(&env),
            Address::generate(&env),
        );
        StellarAssetClient::new(&env, &token).mint(&payer, &1_001);
        let id = c.create_cooperative(&a, &vec![&env, a.clone(), b.clone()], &vec![&env, 7_000u32, 3_000u32]);
        c.deposit_coop_profit(&payer, &id, &token, &1_001);
        assert_eq!(c.get_coop_claimable(&id, &token, &a), 700);
        assert_eq!(c.get_coop_claimable(&id, &token, &b), 301);
        assert_eq!(c.claim_coop_profit(&b, &id, &token), 301);
        assert_eq!(TokenClient::new(&env, &token).balance(&b), 301);
        assert_eq!(c.get_coop_claimable(&id, &token, &b), 0);
        assert_eq!(c.get_coop_total_deposited(&id, &token), 1_001);
    }

    #[test]
    #[should_panic(expected = "shares must sum to 10000 bps")]
    fn test_bad_share_sum_rejected() {
        let (env, c, _t) = setup();
        let a = Address::generate(&env);
        c.create_cooperative(&a, &vec![&env, a.clone()], &vec![&env, 9_000u32]);
    }

    #[test]
    #[should_panic(expected = "duplicate member")]
    fn test_duplicate_member_rejected() {
        let (env, c, _t) = setup();
        let a = Address::generate(&env);
        c.create_cooperative(&a, &vec![&env, a.clone(), a.clone()], &vec![&env, 5_000u32, 5_000u32]);
    }

    #[test]
    #[should_panic(expected = "not a cooperative member")]
    fn test_non_member_cannot_claim() {
        let (env, c, token) = setup();
        let a = Address::generate(&env);
        let id = c.create_cooperative(&a, &vec![&env, a.clone()], &vec![&env, 10_000u32]);
        c.claim_coop_profit(&Address::generate(&env), &id, &token);
    }

    #[test]
    #[should_panic(expected = "nothing to claim")]
    fn test_claim_with_nothing_rejected() {
        let (env, c, token) = setup();
        let a = Address::generate(&env);
        let id = c.create_cooperative(&a, &vec![&env, a.clone()], &vec![&env, 10_000u32]);
        c.claim_coop_profit(&a, &id, &token);
    }
}
