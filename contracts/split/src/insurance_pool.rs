//! Issue #888: Recipient insurance pool governance.
//!
//! Members fund a shared pool per token. Recipients file claims against the
//! pool and members vote on them, weighted by their net contribution. A claim
//! is payable once approving weight exceeds half of the total contributions.

use crate::SplitContract;
#[allow(unused_imports)]
use crate::{SplitContractArgs, SplitContractClient};
use soroban_sdk::{
    contractimpl, contracttype, symbol_short, token, Address, Env, Map, String,
};

/// Voting period for a claim, in ledgers.
const CLAIM_VOTING_LEDGERS: u32 = 120_960;

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum InsuranceKey {
    /// Pool balance per token: i128
    Balance(Address),
    /// Member contribution per (token, member): i128
    Contribution(Address, Address),
    /// Total contributions per token: i128
    TotalContrib(Address),
    /// Next claim id: u64
    NextClaim,
    /// Claim by id
    Claim(u64),
    /// Votes cast on a claim: Map<Address, bool>
    Votes(u64),
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ClaimStatus {
    Open,
    Paid,
    Rejected,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct InsuranceClaim {
    pub id: u64,
    pub token: Address,
    pub claimant: Address,
    pub amount: i128,
    pub reason: String,
    pub invoice_id: u64,
    pub approve_weight: i128,
    pub reject_weight: i128,
    pub voting_ends_ledger: u32,
    pub status: ClaimStatus,
}

fn get_i128(env: &Env, key: &InsuranceKey) -> i128 {
    env.storage().persistent().get(key).unwrap_or(0)
}

fn load_claim(env: &Env, claim_id: u64) -> InsuranceClaim {
    env.storage()
        .persistent()
        .get(&InsuranceKey::Claim(claim_id))
        .expect("claim not found")
}

#[contractimpl]
impl SplitContract {
    /// Contribute `amount` of `token` to the insurance pool. Contribution
    /// grants proportional voting weight on claims.
    pub fn fund_insurance_pool(env: Env, member: Address, token: Address, amount: i128) {
        member.require_auth();
        assert!(amount > 0, "amount must be positive");
        token::Client::new(&env, &token).transfer(
            &member,
            &env.current_contract_address(),
            &amount,
        );
        let bal_key = InsuranceKey::Balance(token.clone());
        let c_key = InsuranceKey::Contribution(token.clone(), member.clone());
        let t_key = InsuranceKey::TotalContrib(token.clone());
        env.storage()
            .persistent()
            .set(&bal_key, &(get_i128(&env, &bal_key) + amount));
        env.storage()
            .persistent()
            .set(&c_key, &(get_i128(&env, &c_key) + amount));
        env.storage()
            .persistent()
            .set(&t_key, &(get_i128(&env, &t_key) + amount));
        env.events().publish(
            (symbol_short!("split"), symbol_short!("ins_fund")),
            (member, token, amount),
        );
    }

    /// File a claim against the pool on behalf of a recipient. Returns the claim id.
    pub fn file_insurance_claim(
        env: Env,
        claimant: Address,
        token: Address,
        amount: i128,
        invoice_id: u64,
        reason: String,
    ) -> u64 {
        claimant.require_auth();
        assert!(amount > 0, "amount must be positive");
        let balance = get_i128(&env, &InsuranceKey::Balance(token.clone()));
        assert!(amount <= balance, "claim exceeds pool balance");
        let id: u64 = env
            .storage()
            .persistent()
            .get(&InsuranceKey::NextClaim)
            .unwrap_or(1u64);
        env.storage()
            .persistent()
            .set(&InsuranceKey::NextClaim, &(id + 1));
        let claim = InsuranceClaim {
            id,
            token: token.clone(),
            claimant: claimant.clone(),
            amount,
            reason,
            invoice_id,
            approve_weight: 0,
            reject_weight: 0,
            voting_ends_ledger: env.ledger().sequence() + CLAIM_VOTING_LEDGERS,
            status: ClaimStatus::Open,
        };
        env.storage()
            .persistent()
            .set(&InsuranceKey::Claim(id), &claim);
        env.events().publish(
            (symbol_short!("split"), symbol_short!("ins_claim"), id),
            (claimant, token, amount, invoice_id),
        );
        id
    }

    /// Vote on an open claim. Weight equals the voter's pool contribution.
    pub fn vote_insurance_claim(env: Env, voter: Address, claim_id: u64, approve: bool) {
        voter.require_auth();
        let mut claim = load_claim(&env, claim_id);
        assert!(claim.status == ClaimStatus::Open, "claim not open");
        assert!(
            env.ledger().sequence() <= claim.voting_ends_ledger,
            "voting period ended"
        );
        let weight = get_i128(
            &env,
            &InsuranceKey::Contribution(claim.token.clone(), voter.clone()),
        );
        assert!(weight > 0, "not a pool member");
        let votes_key = InsuranceKey::Votes(claim_id);
        let mut votes: Map<Address, bool> = env
            .storage()
            .persistent()
            .get(&votes_key)
            .unwrap_or(Map::new(&env));
        assert!(!votes.contains_key(voter.clone()), "already voted");
        votes.set(voter.clone(), approve);
        env.storage().persistent().set(&votes_key, &votes);
        if approve {
            claim.approve_weight += weight;
        } else {
            claim.reject_weight += weight;
        }
        env.storage()
            .persistent()
            .set(&InsuranceKey::Claim(claim_id), &claim);
        env.events().publish(
            (symbol_short!("split"), symbol_short!("ins_vote"), claim_id),
            (voter, approve, weight),
        );
    }

    /// Settle a claim: pays out when approving weight exceeds half of total
    /// contributions, rejects when rejecting weight does (or voting expired
    /// without a majority).
    pub fn execute_insurance_claim(env: Env, claim_id: u64) {
        let mut claim = load_claim(&env, claim_id);
        assert!(claim.status == ClaimStatus::Open, "claim not open");
        let total = get_i128(&env, &InsuranceKey::TotalContrib(claim.token.clone()));
        if claim.approve_weight * 2 > total {
            let bal_key = InsuranceKey::Balance(claim.token.clone());
            let balance = get_i128(&env, &bal_key);
            assert!(claim.amount <= balance, "claim exceeds pool balance");
            env.storage()
                .persistent()
                .set(&bal_key, &(balance - claim.amount));
            token::Client::new(&env, &claim.token).transfer(
                &env.current_contract_address(),
                &claim.claimant,
                &claim.amount,
            );
            claim.status = ClaimStatus::Paid;
            env.events().publish(
                (symbol_short!("split"), symbol_short!("ins_paid"), claim_id),
                (claim.claimant.clone(), claim.amount),
            );
        } else if claim.reject_weight * 2 >= total
            || env.ledger().sequence() > claim.voting_ends_ledger
        {
            claim.status = ClaimStatus::Rejected;
            env.events().publish(
                (symbol_short!("split"), symbol_short!("ins_rej"), claim_id),
                claim.claimant.clone(),
            );
        } else {
            panic!("claim undecided");
        }
        env.storage()
            .persistent()
            .set(&InsuranceKey::Claim(claim_id), &claim);
    }

    /// Current pool balance for a token.
    pub fn get_insurance_pool_balance(env: Env, token: Address) -> i128 {
        get_i128(&env, &InsuranceKey::Balance(token))
    }

    /// A member's contribution to the pool for a token.
    pub fn get_insurance_contribution(env: Env, token: Address, member: Address) -> i128 {
        get_i128(&env, &InsuranceKey::Contribution(token, member))
    }

    /// Fetch a claim by id.
    pub fn get_insurance_claim(env: Env, claim_id: u64) -> InsuranceClaim {
        load_claim(&env, claim_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger};
    use soroban_sdk::token::StellarAssetClient;

    fn setup() -> (Env, SplitContractClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(SplitContract, ());
        let token = env
            .register_stellar_asset_contract_v2(Address::generate(&env))
            .address();
        let c = SplitContractClient::new(&env, &id);
        (env, c, token)
    }

    fn mint(env: &Env, token: &Address, to: &Address, amt: i128) {
        StellarAssetClient::new(env, token).mint(to, &amt);
    }

    #[test]
    fn test_fund_and_claim_paid_by_majority() {
        let (env, c, token) = setup();
        let a = Address::generate(&env);
        let b = Address::generate(&env);
        let claimant = Address::generate(&env);
        mint(&env, &token, &a, 600);
        mint(&env, &token, &b, 400);
        c.fund_insurance_pool(&a, &token, &600);
        c.fund_insurance_pool(&b, &token, &400);
        assert_eq!(c.get_insurance_pool_balance(&token), 1000);

        let id = c.file_insurance_claim(&claimant, &token, &300, &1, &String::from_str(&env, "loss"));
        c.vote_insurance_claim(&a, &id, &true);
        c.execute_insurance_claim(&id);
        assert_eq!(c.get_insurance_pool_balance(&token), 700);
        assert_eq!(c.get_insurance_claim(&id).status, ClaimStatus::Paid);
    }

    #[test]
    #[should_panic(expected = "claim undecided")]
    fn test_minority_vote_undecided() {
        let (env, c, token) = setup();
        let a = Address::generate(&env);
        let b = Address::generate(&env);
        mint(&env, &token, &a, 300);
        mint(&env, &token, &b, 700);
        c.fund_insurance_pool(&a, &token, &300);
        c.fund_insurance_pool(&b, &token, &700);
        let id = c.file_insurance_claim(&a, &token, &100, &1, &String::from_str(&env, "x"));
        c.vote_insurance_claim(&a, &id, &true);
        c.execute_insurance_claim(&id);
    }

    #[test]
    fn test_expired_claim_rejected() {
        let (env, c, token) = setup();
        let a = Address::generate(&env);
        mint(&env, &token, &a, 100);
        c.fund_insurance_pool(&a, &token, &100);
        let id = c.file_insurance_claim(&a, &token, &50, &1, &String::from_str(&env, "x"));
        env.ledger().set_sequence_number(env.ledger().sequence() + CLAIM_VOTING_LEDGERS + 1);
        c.execute_insurance_claim(&id);
        assert_eq!(c.get_insurance_claim(&id).status, ClaimStatus::Rejected);
    }

    #[test]
    #[should_panic(expected = "not a pool member")]
    fn test_non_member_cannot_vote() {
        let (env, c, token) = setup();
        let a = Address::generate(&env);
        mint(&env, &token, &a, 100);
        c.fund_insurance_pool(&a, &token, &100);
        let id = c.file_insurance_claim(&a, &token, &50, &1, &String::from_str(&env, "x"));
        c.vote_insurance_claim(&Address::generate(&env), &id, &true);
    }

    #[test]
    #[should_panic(expected = "already voted")]
    fn test_double_vote_rejected() {
        let (env, c, token) = setup();
        let a = Address::generate(&env);
        mint(&env, &token, &a, 100);
        c.fund_insurance_pool(&a, &token, &100);
        let id = c.file_insurance_claim(&a, &token, &50, &1, &String::from_str(&env, "x"));
        c.vote_insurance_claim(&a, &id, &true);
        c.vote_insurance_claim(&a, &id, &true);
    }
}
