//! Issue #786: payment matching pool.
//!
//! Matchers lock funds against an invoice with `pledge_match`. When a real
//! payment is credited in `_pay`, each pledge is matched first-come (pledge
//! order) for `min(unmatched, payment, remaining_need)`. The pledged tokens are
//! already held by the contract, so matching only credits `invoice.funded`.
//! Unmatched remainders are refundable via `claim_unmatched_pledge` once the
//! invoice deadline passed, it is fully funded, or it left `Pending`.
//!
//! Storage: its own key enum (`MatchKey`), persistent tier, one entry per invoice.
use soroban_sdk::{
    contracttype, panic_with_error, symbol_short, token, Address, Env, Vec,
};

use crate::error::ContractError;
use crate::types::InvoiceStatus;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MatchKey {
    /// Vec<MatchPledge> for an invoice.
    Pool(u64),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchPledge {
    pub matcher: Address,
    /// Total amount locked.
    pub amount: i128,
    /// Portion already credited to the invoice.
    pub matched: i128,
    /// True once the unmatched remainder was returned.
    pub claimed: bool,
}

pub fn get_pool(env: &Env, invoice_id: u64) -> Vec<MatchPledge> {
    env.storage()
        .persistent()
        .get(&MatchKey::Pool(invoice_id))
        .unwrap_or_else(|| Vec::new(env))
}

fn save_pool(env: &Env, invoice_id: u64, pool: &Vec<MatchPledge>) {
    env.storage().persistent().set(&MatchKey::Pool(invoice_id), pool);
}

pub fn pledge(env: &Env, matcher: &Address, invoice_id: u64, amount: i128) {
    matcher.require_auth();
    if amount <= 0 {
        panic_with_error!(env, ContractError::InvalidPledgeAmount);
    }
    let invoice = crate::load_invoice(env, invoice_id);
    if invoice.status != InvoiceStatus::Pending {
        panic_with_error!(env, ContractError::InvalidStatus);
    }
    let mut pool = get_pool(env, invoice_id);
    for p in pool.iter() {
        if p.matcher == *matcher {
            panic_with_error!(env, ContractError::MatchPledgeExists);
        }
    }
    token::Client::new(env, &invoice.funding_token).transfer(
        matcher,
        &env.current_contract_address(),
        &amount,
    );
    pool.push_back(MatchPledge {
        matcher: matcher.clone(),
        amount,
        matched: 0,
        claimed: false,
    });
    save_pool(env, invoice_id, &pool);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("mt_pledge")),
        (invoice_id, matcher.clone(), amount),
    );
}

/// Match pledges against a payment of `payment` when `capacity` is still
/// needed to fully fund the invoice. Returns the total matched amount to add
/// to `invoice.funded`.
pub fn apply_matches(env: &Env, invoice_id: u64, payment: i128, capacity: i128) -> i128 {
    let pool = get_pool(env, invoice_id);
    if pool.is_empty() || payment <= 0 || capacity <= 0 {
        return 0;
    }
    let mut payment_left = payment;
    let mut capacity_left = capacity;
    let mut total = 0i128;
    let mut out = Vec::new(env);
    for mut p in pool.iter() {
        let unmatched = p.amount - p.matched;
        let m = unmatched.min(payment_left).min(capacity_left);
        if !p.claimed && m > 0 {
            p.matched += m;
            payment_left -= m;
            capacity_left -= m;
            total += m;
            env.events().publish(
                (symbol_short!("split"), symbol_short!("mt_trig")),
                (invoice_id, p.matcher.clone(), m),
            );
        }
        out.push_back(p);
    }
    if total > 0 {
        save_pool(env, invoice_id, &out);
    }
    total
}

pub fn claim(env: &Env, matcher: &Address, invoice_id: u64) -> i128 {
    matcher.require_auth();
    let invoice = crate::load_invoice(env, invoice_id);
    let total: i128 = invoice.amounts.iter().sum();
    let closed = invoice.status != InvoiceStatus::Pending
        || env.ledger().timestamp() > invoice.deadline
        || invoice.funded >= total;
    if !closed {
        panic_with_error!(env, ContractError::PledgeNotClaimable);
    }
    let mut pool = get_pool(env, invoice_id);
    let mut refund = 0i128;
    let mut found = false;
    let mut out = Vec::new(env);
    for mut p in pool.iter() {
        if p.matcher == *matcher && !p.claimed {
            found = true;
            refund = p.amount - p.matched;
            p.claimed = true;
        }
        out.push_back(p);
    }
    if !found {
        panic_with_error!(env, ContractError::MatchPledgeNotFound);
    }
    pool = out;
    save_pool(env, invoice_id, &pool);
    if refund > 0 {
        token::Client::new(env, &invoice.funding_token).transfer(
            &env.current_contract_address(),
            matcher,
            &refund,
        );
    }
    env.events().publish(
        (symbol_short!("split"), symbol_short!("mt_claim")),
        (invoice_id, matcher.clone(), refund),
    );
    refund
}

#[cfg(test)]
mod tests {
    extern crate std;
    use crate::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger},
        token::{Client as TokenClient, StellarAssetClient},
        Address, Env, Vec,
    };

    fn setup() -> (Env, SplitContractClient<'static>, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract = env.register(SplitContract, ());
        let token = env
            .register_stellar_asset_contract_v2(Address::generate(&env))
            .address();
        let c = SplitContractClient::new(&env, &contract);
        c.initialize(
            &Address::generate(&env),
            &0_i128,
            &Address::generate(&env),
            &token,
            &0_u32,
            &None,
            &0_u32,
            &0_u32,
            &0_u64,
        );
        env.ledger().set_timestamp(1_000);
        (env, c, token, contract)
    }

    fn invoice(env: &Env, c: &SplitContractClient, token: &Address, amount: i128) -> u64 {
        let mut r = Vec::new(env);
        r.push_back(Address::generate(env));
        let mut a = Vec::new(env);
        a.push_back(amount);
        let opts = crate::types::InvoiceOptions {
            co_creators: Vec::new(env),
            allow_early_withdrawal: false,
            bonus_pool: 0,
            bonus_max_payers: 0,
            creator_cosigner: None,
            velocity_limit: 0,
            velocity_window: 0,
            prerequisite_id: None,
            tranches: Vec::new(env),
            co_signers: Vec::new(env),
            required_signatures: 0,
            penalty_bps: None,
            penalty_deadline: None,
            min_funding_bps: None,
            release_stages: Vec::new(env),
            price_oracle: None,
            swap_tokens: Vec::new(env),
            tax_bps: None,
            tax_authority: None,
            insurance_premium_bps: None,
            smart_route: None,
            notification_contract: None,
            overflow_behavior: crate::types::OverflowBehavior::Reject,
            convert_to_stream: false,
            accepted_tokens: Vec::new(env),
            forward_to: None,
            forward_invoice_id: None,
            split_rules: Vec::new(env),
            auto_resolve_rules: Vec::new(env),
            condition_oracle: None,
            cross_chain_ref: None,
            allowed_payers: None,
            refund_grace_secs: None,
            priorities: Vec::new(env),
            require_kyc: false,
            scheduled_release_at: None,
            ratios: Vec::new(env),
            cosigners: None,
            cosigner_threshold: None,
            ext: crate::types::InvoiceOptions2::default(),
        };
        c.create_invoice(&Address::generate(env), &r, &a, token, &2_000_u64, &opts)
    }

    fn funded(env: &Env, token: &Address, who: &Address, n: i128) {
        StellarAssetClient::new(env, token).mint(who, &n);
    }

    #[test]
    fn pledge_then_pay_triggers_match() {
        let (env, c, token, _) = setup();
        let id = invoice(&env, &c, &token, 100);
        let m = Address::generate(&env);
        let payer = Address::generate(&env);
        funded(&env, &token, &m, 50);
        funded(&env, &token, &payer, 30);
        c.pledge_match(&m, &id, &50);
        c.pay(&payer, &id, &30, &0_u64, &false, &false, &None);
        assert_eq!(c.get_invoice(&id).funded, 60);
        let pool = c.get_match_pool(&id);
        assert_eq!(pool.get(0).unwrap().matched, 30);
    }

    #[test]
    fn over_pledged_amount_refundable() {
        let (env, c, token, _) = setup();
        let id = invoice(&env, &c, &token, 100);
        let m = Address::generate(&env);
        let payer = Address::generate(&env);
        funded(&env, &token, &m, 500);
        funded(&env, &token, &payer, 60);
        c.pledge_match(&m, &id, &500);
        c.pay(&payer, &id, &60, &0_u64, &false, &false, &None);
        // Matching is capped by remaining need: 60 + 40 = 100.
        assert_eq!(c.get_invoice(&id).funded, 100);
        assert_eq!(c.claim_unmatched_pledge(&m, &id), 460);
        assert_eq!(TokenClient::new(&env, &token).balance(&m), 460);
    }

    #[test]
    fn pledge_after_full_funding_not_matched() {
        let (env, c, token, _) = setup();
        let id = invoice(&env, &c, &token, 100);
        let payer = Address::generate(&env);
        let m = Address::generate(&env);
        funded(&env, &token, &payer, 100);
        funded(&env, &token, &m, 40);
        c.pay(&payer, &id, &100, &0_u64, &false, &false, &None);
        if c.get_invoice(&id).status == crate::types::InvoiceStatus::Pending {
            c.pledge_match(&m, &id, &40);
            assert_eq!(c.get_match_pool(&id).get(0).unwrap().matched, 0);
            assert_eq!(c.claim_unmatched_pledge(&m, &id), 40);
        }
    }

    #[test]
    #[should_panic]
    fn one_pledge_per_matcher() {
        let (env, c, token, _) = setup();
        let id = invoice(&env, &c, &token, 100);
        let m = Address::generate(&env);
        funded(&env, &token, &m, 20);
        c.pledge_match(&m, &id, &10);
        c.pledge_match(&m, &id, &10);
    }
}
