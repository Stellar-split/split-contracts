//! Tests for issues #857 (dynamic fees), #858 (earnings insurance),
//! #859 (cross-contract invoice links) and #860 (creator liquidity pools).
#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};
use types::{
    DynamicFeeConfig, EarningsPolicyStatus, InvoiceOptions, InvoiceOptions2, OverflowBehavior,
    OverfundingPolicy,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct Ctx {
    env: Env,
    contract_id: Address,
    token_id: Address,
    admin: Address,
}

impl Ctx {
    fn client(&self) -> SplitContractClient<'_> {
        SplitContractClient::new(&self.env, &self.contract_id)
    }

    fn token(&self) -> TokenClient<'_> {
        TokenClient::new(&self.env, &self.token_id)
    }

    fn mint(&self, to: &Address, amount: i128) {
        StellarAssetClient::new(&self.env, &self.token_id).mint(to, &amount);
    }
}

fn register_split(env: &Env, token_id: &Address) -> (Address, Address) {
    let contract_id = env.register(SplitContract, ());
    let admin = Address::generate(env);
    let treasury = Address::generate(env);
    SplitContractClient::new(env, &contract_id).initialize(
        &admin, &0_i128, &treasury, token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64,
    );
    (contract_id, admin)
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000);
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let (contract_id, admin) = register_split(&env, &token_id);
    Ctx { env, contract_id, token_id, admin }
}

fn options(env: &Env) -> InvoiceOptions {
    InvoiceOptions {
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
        overflow_behavior: OverflowBehavior::Reject,
        convert_to_stream: false,
        accepted_tokens: Vec::new(env),
        forward_to: None,
        forward_invoice_id: None,
        split_rules: Vec::new(env),
        auto_resolve_rules: Vec::new(env),
        oracle_address: None,
        cross_chain_ref: None,
        allowed_payers: None,
        refund_grace_secs: None,
        priorities: Vec::new(env),
        require_kyc: false,
        scheduled_release_at: None,
        ratios: Vec::new(env),
        cosigners: None,
        cosigner_threshold: None,
        ext: InvoiceOptions2 {
            target_usd_cents: None,
            payment_token: None,
            release_delay_ledgers: None,
            metadata_hash: None,
            payment_cooldown_secs: None,
            max_payments_per_window: None,
            payment_window_secs: None,
            oracle: None,
            oracle_asset_pair_base: None,
            oracle_asset_pair_quote: None,
            min_payer_rep: None,
            payment_open_at: None,
            payment_close_at: None,
            milestones: None,
            recipient_max_payouts: None,
            release_condition_hash: None,
            recipient_whitelist_enabled: false,
            escrow_hold_period: None,
            overfunding_policy: OverfundingPolicy::Cap,
            early_bird_window_ledgers: 0,
            early_bird_fee_bps: 0,
            creator_fee_bps: 0,
            early_bird_fee_credit: 0,
            ratio_denominator: 10_000,
        },
    }
}

fn make_invoice_on(
    env: &Env,
    contract_id: &Address,
    token_id: &Address,
    creator: &Address,
    recipient: &Address,
    amount: i128,
) -> u64 {
    let mut recipients = Vec::new(env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(env);
    amounts.push_back(amount);
    SplitContractClient::new(env, contract_id).create_invoice(
        creator,
        &recipients,
        &amounts,
        token_id,
        &9_999,
        &options(env),
    )
}

fn make_invoice(ctx: &Ctx, creator: &Address, recipient: &Address, amount: i128) -> u64 {
    make_invoice_on(&ctx.env, &ctx.contract_id, &ctx.token_id, creator, recipient, amount)
}

// ---------------------------------------------------------------------------
// Issue #860: Creator liquidity pool
// ---------------------------------------------------------------------------

#[test]
fn test_pool_first_deposit_mints_one_to_one() {
    let ctx = setup();
    let c = ctx.client();
    let creator = Address::generate(&ctx.env);
    let lp = Address::generate(&ctx.env);
    ctx.mint(&lp, 1_000);

    let shares = c.pool_deposit(&lp, &creator, &ctx.token_id, &400);

    assert_eq!(shares, 400);
    assert_eq!(c.get_pool_shares(&creator, &ctx.token_id, &lp), 400);
    let pool = c.get_creator_pool(&creator, &ctx.token_id);
    assert_eq!(pool.available, 400);
    assert_eq!(pool.total_shares, 400);
    assert_eq!(pool.outstanding, 0);
    assert_eq!(ctx.token().balance(&lp), 600);
    assert_eq!(ctx.token().balance(&ctx.contract_id), 400);
}

#[test]
fn test_pool_deposit_emits_event() {
    let ctx = setup();
    let c = ctx.client();
    let creator = Address::generate(&ctx.env);
    let lp = Address::generate(&ctx.env);
    ctx.mint(&lp, 100);

    c.pool_deposit(&lp, &creator, &ctx.token_id, &100);

    let last = ctx.env.events().all().last().unwrap();
    assert_eq!(last.0, ctx.contract_id);
    let topics: Vec<soroban_sdk::Val> = last.1;
    let t0: Symbol = Symbol::try_from_val(&ctx.env, &topics.get(0).unwrap()).unwrap();
    let t1: Symbol = Symbol::try_from_val(&ctx.env, &topics.get(1).unwrap()).unwrap();
    assert_eq!(t0, symbol_short!("pool"));
    assert_eq!(t1, symbol_short!("deposit"));
}

#[test]
fn test_pool_repay_fee_accrues_to_providers() {
    let ctx = setup();
    let c = ctx.client();
    let creator = Address::generate(&ctx.env);
    let lp_a = Address::generate(&ctx.env);
    let lp_b = Address::generate(&ctx.env);
    ctx.mint(&lp_a, 1_000);
    ctx.mint(&lp_b, 1_000);
    ctx.mint(&creator, 100);

    c.pool_deposit(&lp_a, &creator, &ctx.token_id, &600);
    c.pool_deposit(&lp_b, &creator, &ctx.token_id, &400);

    c.pool_draw(&creator, &ctx.token_id, &500);
    let pool = c.get_creator_pool(&creator, &ctx.token_id);
    assert_eq!(pool.available, 500);
    assert_eq!(pool.outstanding, 500);
    assert_eq!(ctx.token().balance(&creator), 600);

    c.pool_repay(&creator, &ctx.token_id, &500, &100);
    let pool = c.get_creator_pool(&creator, &ctx.token_id);
    assert_eq!(pool.available, 1_100);
    assert_eq!(pool.outstanding, 0);
    assert_eq!(pool.fees_earned, 100);

    assert_eq!(c.get_pool_position_value(&creator, &ctx.token_id, &lp_a), 660);
    assert_eq!(c.get_pool_position_value(&creator, &ctx.token_id, &lp_b), 440);

    let out = c.pool_withdraw(&lp_a, &creator, &ctx.token_id, &600);
    assert_eq!(out, 660);
    assert_eq!(ctx.token().balance(&lp_a), 1_060);
    assert_eq!(c.get_pool_shares(&creator, &ctx.token_id, &lp_a), 0);
}

#[test]
fn test_pool_deposit_after_fee_mints_fewer_shares() {
    let ctx = setup();
    let c = ctx.client();
    let creator = Address::generate(&ctx.env);
    let lp_a = Address::generate(&ctx.env);
    let lp_b = Address::generate(&ctx.env);
    ctx.mint(&lp_a, 1_000);
    ctx.mint(&lp_b, 1_000);
    ctx.mint(&creator, 1_000);

    c.pool_deposit(&lp_a, &creator, &ctx.token_id, &1_000);
    c.pool_draw(&creator, &ctx.token_id, &1_000);
    c.pool_repay(&creator, &ctx.token_id, &1_000, &1_000);

    // Share price is now 2 tokens/share.
    let shares = c.pool_deposit(&lp_b, &creator, &ctx.token_id, &1_000);
    assert_eq!(shares, 500);
}

#[test]
fn test_pool_draw_exceeding_available_fails() {
    let ctx = setup();
    let c = ctx.client();
    let creator = Address::generate(&ctx.env);
    let lp = Address::generate(&ctx.env);
    ctx.mint(&lp, 100);
    c.pool_deposit(&lp, &creator, &ctx.token_id, &100);

    assert!(c.try_pool_draw(&creator, &ctx.token_id, &101).is_err());
}

#[test]
fn test_pool_withdraw_blocked_while_liquidity_drawn() {
    let ctx = setup();
    let c = ctx.client();
    let creator = Address::generate(&ctx.env);
    let lp = Address::generate(&ctx.env);
    ctx.mint(&lp, 100);
    c.pool_deposit(&lp, &creator, &ctx.token_id, &100);
    c.pool_draw(&creator, &ctx.token_id, &60);

    assert!(c.try_pool_withdraw(&lp, &creator, &ctx.token_id, &100).is_err());
    // Withdrawing only the undrawn portion succeeds.
    assert_eq!(c.pool_withdraw(&lp, &creator, &ctx.token_id, &40), 40);
}

#[test]
fn test_pool_withdraw_more_shares_than_held_fails() {
    let ctx = setup();
    let c = ctx.client();
    let creator = Address::generate(&ctx.env);
    let lp = Address::generate(&ctx.env);
    ctx.mint(&lp, 100);
    c.pool_deposit(&lp, &creator, &ctx.token_id, &100);

    assert!(c.try_pool_withdraw(&lp, &creator, &ctx.token_id, &101).is_err());
}

#[test]
fn test_pool_repay_exceeding_debt_fails() {
    let ctx = setup();
    let c = ctx.client();
    let creator = Address::generate(&ctx.env);
    let lp = Address::generate(&ctx.env);
    ctx.mint(&lp, 100);
    ctx.mint(&creator, 100);
    c.pool_deposit(&lp, &creator, &ctx.token_id, &100);
    c.pool_draw(&creator, &ctx.token_id, &50);

    assert!(c.try_pool_repay(&creator, &ctx.token_id, &51, &0).is_err());
}

#[test]
fn test_pool_zero_deposit_fails() {
    let ctx = setup();
    let c = ctx.client();
    let creator = Address::generate(&ctx.env);
    let lp = Address::generate(&ctx.env);

    assert!(c.try_pool_deposit(&lp, &creator, &ctx.token_id, &0).is_err());
}

#[test]
fn test_pools_are_isolated_per_creator() {
    let ctx = setup();
    let c = ctx.client();
    let creator_a = Address::generate(&ctx.env);
    let creator_b = Address::generate(&ctx.env);
    let lp = Address::generate(&ctx.env);
    ctx.mint(&lp, 100);
    c.pool_deposit(&lp, &creator_a, &ctx.token_id, &100);

    assert!(c.try_pool_draw(&creator_b, &ctx.token_id, &1).is_err());
    assert_eq!(c.get_creator_pool(&creator_b, &ctx.token_id).available, 0);
}

// ---------------------------------------------------------------------------
// Issue #859: Cross-contract invoice linking
// ---------------------------------------------------------------------------

#[test]
fn test_link_external_invoice_stores_link() {
    let ctx = setup();
    let c = ctx.client();
    let creator = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    let id = make_invoice(&ctx, &creator, &recipient, 100);
    let remote = Address::generate(&ctx.env);

    c.link_external_invoice(&creator, &id, &remote, &7);

    assert!(c.is_externally_linked(&id, &remote, &7));
    let links = c.get_external_links(&id);
    assert_eq!(links.len(), 1);
    let link = links.get(0).unwrap();
    assert_eq!(link.remote_contract, remote);
    assert_eq!(link.remote_invoice_id, 7);
    assert!(!link.verified);
    assert_eq!(link.linked_at, 1_000);
}

#[test]
fn test_link_duplicate_fails() {
    let ctx = setup();
    let c = ctx.client();
    let creator = Address::generate(&ctx.env);
    let id = make_invoice(&ctx, &creator, &Address::generate(&ctx.env), 100);
    let remote = Address::generate(&ctx.env);

    c.link_external_invoice(&creator, &id, &remote, &7);
    assert!(c.try_link_external_invoice(&creator, &id, &remote, &7).is_err());
}

#[test]
fn test_link_to_self_fails() {
    let ctx = setup();
    let c = ctx.client();
    let creator = Address::generate(&ctx.env);
    let id = make_invoice(&ctx, &creator, &Address::generate(&ctx.env), 100);

    assert!(c
        .try_link_external_invoice(&creator, &id, &ctx.contract_id, &id)
        .is_err());
}

#[test]
fn test_link_by_non_creator_fails() {
    let ctx = setup();
    let c = ctx.client();
    let creator = Address::generate(&ctx.env);
    let stranger = Address::generate(&ctx.env);
    let id = make_invoice(&ctx, &creator, &Address::generate(&ctx.env), 100);

    assert!(c
        .try_link_external_invoice(&stranger, &id, &Address::generate(&ctx.env), &1)
        .is_err());
}

#[test]
fn test_link_limit_enforced() {
    let ctx = setup();
    let c = ctx.client();
    let creator = Address::generate(&ctx.env);
    let id = make_invoice(&ctx, &creator, &Address::generate(&ctx.env), 100);
    let remote = Address::generate(&ctx.env);

    for i in 0..invoice_links::MAX_EXTERNAL_LINKS as u64 {
        c.link_external_invoice(&creator, &id, &remote, &(100 + i));
    }
    assert!(c.try_link_external_invoice(&creator, &id, &remote, &999).is_err());
}

#[test]
fn test_unlink_external_invoice() {
    let ctx = setup();
    let c = ctx.client();
    let creator = Address::generate(&ctx.env);
    let id = make_invoice(&ctx, &creator, &Address::generate(&ctx.env), 100);
    let remote = Address::generate(&ctx.env);

    c.link_external_invoice(&creator, &id, &remote, &7);
    c.unlink_external_invoice(&creator, &id, &remote, &7);

    assert!(!c.is_externally_linked(&id, &remote, &7));
    assert_eq!(c.get_external_links(&id).len(), 0);
    assert!(c.try_unlink_external_invoice(&creator, &id, &remote, &7).is_err());
}

#[test]
fn test_verify_link_against_remote_contract() {
    let ctx = setup();
    let c = ctx.client();
    let (remote_id, _) = register_split(&ctx.env, &ctx.token_id);
    let creator = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);

    let local = make_invoice(&ctx, &creator, &recipient, 100);
    let remote_invoice =
        make_invoice_on(&ctx.env, &remote_id, &ctx.token_id, &creator, &recipient, 50);

    c.link_external_invoice(&creator, &local, &remote_id, &remote_invoice);
    assert!(c.verify_external_link(&local, &remote_id, &remote_invoice));

    let link = c.get_external_links(&local).get(0).unwrap();
    assert!(link.verified);
    assert_eq!(link.remote_status, Some(InvoiceStatus::Pending));
}

#[test]
fn test_verify_link_to_missing_remote_invoice_is_unverified() {
    let ctx = setup();
    let c = ctx.client();
    let (remote_id, _) = register_split(&ctx.env, &ctx.token_id);
    let creator = Address::generate(&ctx.env);
    let local = make_invoice(&ctx, &creator, &Address::generate(&ctx.env), 100);

    c.link_external_invoice(&creator, &local, &remote_id, &424_242);
    assert!(!c.verify_external_link(&local, &remote_id, &424_242));

    let link = c.get_external_links(&local).get(0).unwrap();
    assert!(!link.verified);
    assert_eq!(link.remote_status, None);
}

#[test]
fn test_verify_link_to_invoice_in_same_contract() {
    let ctx = setup();
    let c = ctx.client();
    let creator = Address::generate(&ctx.env);
    let a = make_invoice(&ctx, &creator, &Address::generate(&ctx.env), 100);
    let b = make_invoice(&ctx, &creator, &Address::generate(&ctx.env), 200);

    c.link_external_invoice(&creator, &a, &ctx.contract_id, &b);
    assert!(c.verify_external_link(&a, &ctx.contract_id, &b));
}

#[test]
fn test_verify_unknown_link_fails() {
    let ctx = setup();
    let c = ctx.client();
    let creator = Address::generate(&ctx.env);
    let id = make_invoice(&ctx, &creator, &Address::generate(&ctx.env), 100);

    assert!(c
        .try_verify_external_link(&id, &Address::generate(&ctx.env), &1)
        .is_err());
}

// ---------------------------------------------------------------------------
// Issue #858: Recipient earnings insurance
// ---------------------------------------------------------------------------

/// Configure a 10% premium pool funded with `capital`.
fn setup_insurance(ctx: &Ctx, capital: i128) {
    let c = ctx.client();
    c.configure_earnings_insurance(&ctx.admin, &ctx.token_id, &1_000);
    if capital > 0 {
        let underwriter = Address::generate(&ctx.env);
        ctx.mint(&underwriter, capital);
        c.fund_earnings_insurance(&underwriter, &ctx.token_id, &capital);
    }
}

#[test]
fn test_buy_earnings_insurance_charges_premium_and_reserves() {
    let ctx = setup();
    let c = ctx.client();
    setup_insurance(&ctx, 1_000);
    let creator = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    ctx.mint(&recipient, 50);
    let id = make_invoice(&ctx, &creator, &recipient, 200);

    assert_eq!(c.quote_earnings_premium(&ctx.token_id, &200), 20);
    let premium = c.buy_earnings_insurance(&recipient, &id, &200);

    assert_eq!(premium, 20);
    assert_eq!(ctx.token().balance(&recipient), 30);
    let pool = c.get_earnings_insurance_pool(&ctx.token_id).unwrap();
    assert_eq!(pool.capital, 1_020);
    assert_eq!(pool.reserved, 200);
    let policy = c.get_earnings_policy(&id, &recipient).unwrap();
    assert_eq!(policy.coverage, 200);
    assert_eq!(policy.status, EarningsPolicyStatus::Active);
}

#[test]
fn test_claim_earnings_insurance_after_cancel() {
    let ctx = setup();
    let c = ctx.client();
    setup_insurance(&ctx, 1_000);
    let creator = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    ctx.mint(&recipient, 20);
    let id = make_invoice(&ctx, &creator, &recipient, 200);
    c.buy_earnings_insurance(&recipient, &id, &200);

    c.cancel_invoice(&creator, &id);
    let payout = c.claim_earnings_insurance(&recipient, &id);

    assert_eq!(payout, 200);
    assert_eq!(ctx.token().balance(&recipient), 200);
    let pool = c.get_earnings_insurance_pool(&ctx.token_id).unwrap();
    assert_eq!(pool.capital, 820);
    assert_eq!(pool.reserved, 0);
    assert_eq!(
        c.get_earnings_policy(&id, &recipient).unwrap().status,
        EarningsPolicyStatus::Claimed
    );
    // Cannot claim twice.
    assert!(c.try_claim_earnings_insurance(&recipient, &id).is_err());
}

#[test]
fn test_claim_while_invoice_pending_fails() {
    let ctx = setup();
    let c = ctx.client();
    setup_insurance(&ctx, 1_000);
    let creator = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    ctx.mint(&recipient, 20);
    let id = make_invoice(&ctx, &creator, &recipient, 200);
    c.buy_earnings_insurance(&recipient, &id, &200);

    assert!(c.try_claim_earnings_insurance(&recipient, &id).is_err());
}

#[test]
fn test_settle_earnings_insurance_after_release() {
    let ctx = setup();
    let c = ctx.client();
    setup_insurance(&ctx, 1_000);
    let creator = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    let payer = Address::generate(&ctx.env);
    ctx.mint(&recipient, 20);
    ctx.mint(&payer, 200);
    let id = make_invoice(&ctx, &creator, &recipient, 200);
    c.buy_earnings_insurance(&recipient, &id, &200);

    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);

    assert!(c.try_claim_earnings_insurance(&recipient, &id).is_err());
    c.settle_earnings_insurance(&id, &recipient);

    let pool = c.get_earnings_insurance_pool(&ctx.token_id).unwrap();
    assert_eq!(pool.capital, 1_020);
    assert_eq!(pool.reserved, 0);
    assert_eq!(
        c.get_earnings_policy(&id, &recipient).unwrap().status,
        EarningsPolicyStatus::Settled
    );
}

#[test]
fn test_buy_insurance_without_capacity_fails() {
    let ctx = setup();
    let c = ctx.client();
    setup_insurance(&ctx, 100);
    let creator = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    ctx.mint(&recipient, 50);
    let id = make_invoice(&ctx, &creator, &recipient, 200);

    assert!(c.try_buy_earnings_insurance(&recipient, &id, &200).is_err());
}

#[test]
fn test_buy_insurance_over_allocation_fails() {
    let ctx = setup();
    let c = ctx.client();
    setup_insurance(&ctx, 1_000);
    let creator = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    ctx.mint(&recipient, 50);
    let id = make_invoice(&ctx, &creator, &recipient, 200);

    assert!(c.try_buy_earnings_insurance(&recipient, &id, &201).is_err());
}

#[test]
fn test_buy_insurance_by_non_recipient_fails() {
    let ctx = setup();
    let c = ctx.client();
    setup_insurance(&ctx, 1_000);
    let creator = Address::generate(&ctx.env);
    let stranger = Address::generate(&ctx.env);
    ctx.mint(&stranger, 50);
    let id = make_invoice(&ctx, &creator, &Address::generate(&ctx.env), 200);

    assert!(c.try_buy_earnings_insurance(&stranger, &id, &100).is_err());
}

#[test]
fn test_buy_insurance_twice_fails() {
    let ctx = setup();
    let c = ctx.client();
    setup_insurance(&ctx, 1_000);
    let creator = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    ctx.mint(&recipient, 50);
    let id = make_invoice(&ctx, &creator, &recipient, 200);

    c.buy_earnings_insurance(&recipient, &id, &100);
    assert!(c.try_buy_earnings_insurance(&recipient, &id, &100).is_err());
}

#[test]
fn test_buy_insurance_unconfigured_token_fails() {
    let ctx = setup();
    let c = ctx.client();
    let creator = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    ctx.mint(&recipient, 50);
    let id = make_invoice(&ctx, &creator, &recipient, 200);

    assert!(c.try_buy_earnings_insurance(&recipient, &id, &100).is_err());
}

#[test]
fn test_withdraw_insurance_capital_limited_to_unreserved() {
    let ctx = setup();
    let c = ctx.client();
    setup_insurance(&ctx, 1_000);
    let creator = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    let to = Address::generate(&ctx.env);
    ctx.mint(&recipient, 50);
    let id = make_invoice(&ctx, &creator, &recipient, 200);
    c.buy_earnings_insurance(&recipient, &id, &200);

    // capital 1_020, reserved 200 → 820 free.
    assert!(c
        .try_withdraw_earnings_insurance(&ctx.admin, &ctx.token_id, &821, &to)
        .is_err());
    c.withdraw_earnings_insurance(&ctx.admin, &ctx.token_id, &820, &to);
    assert_eq!(ctx.token().balance(&to), 820);
}

#[test]
fn test_configure_insurance_rejects_excessive_premium() {
    let ctx = setup();
    let c = ctx.client();
    assert!(c
        .try_configure_earnings_insurance(&ctx.admin, &ctx.token_id, &5_001)
        .is_err());
}

#[test]
fn test_configure_insurance_by_non_admin_fails() {
    let ctx = setup();
    let c = ctx.client();
    let stranger = Address::generate(&ctx.env);
    assert!(c
        .try_configure_earnings_insurance(&stranger, &ctx.token_id, &100)
        .is_err());
}

// ---------------------------------------------------------------------------
// Issue #857: Dynamic fee adjustment
// ---------------------------------------------------------------------------

fn fee_config(reporter: &Address) -> DynamicFeeConfig {
    DynamicFeeConfig {
        base_bps: 100,
        min_bps: 50,
        max_bps: 300,
        target_volume: 10_000,
        volume_sensitivity_bps: 100,
        volatility_sensitivity_bps: 200,
        max_step_bps: 0,
        reporter: reporter.clone(),
    }
}

#[test]
fn test_configure_dynamic_fee_sets_base_fee() {
    let ctx = setup();
    let c = ctx.client();
    let reporter = Address::generate(&ctx.env);

    c.configure_dynamic_fee(&ctx.admin, &fee_config(&reporter));

    assert_eq!(c.get_platform_fee_bps(), 100);
    assert_eq!(c.get_dynamic_fee_config(), Some(fee_config(&reporter)));
}

#[test]
fn test_high_volume_raises_fee() {
    let ctx = setup();
    let c = ctx.client();
    let reporter = Address::generate(&ctx.env);
    c.configure_dynamic_fee(&ctx.admin, &fee_config(&reporter));

    // Volume 2× target → +100% deviation → +100 bps.
    let fee = c.report_market_conditions(&reporter, &20_000, &0);

    assert_eq!(fee, 200);
    assert_eq!(c.get_platform_fee_bps(), 200);
    let cond = c.get_market_conditions().unwrap();
    assert_eq!(cond.volume, 20_000);
    assert_eq!(cond.applied_fee_bps, 200);
}

#[test]
fn test_low_volume_lowers_fee_to_min() {
    let ctx = setup();
    let c = ctx.client();
    let reporter = Address::generate(&ctx.env);
    c.configure_dynamic_fee(&ctx.admin, &fee_config(&reporter));

    // Zero volume → −100 bps → 0, clamped to min 50.
    assert_eq!(c.report_market_conditions(&reporter, &0, &0), 50);
}

#[test]
fn test_extreme_volume_clamped_to_max() {
    let ctx = setup();
    let c = ctx.client();
    let reporter = Address::generate(&ctx.env);
    c.configure_dynamic_fee(&ctx.admin, &fee_config(&reporter));

    assert_eq!(c.report_market_conditions(&reporter, &1_000_000_000, &0), 300);
}

#[test]
fn test_volatility_raises_fee() {
    let ctx = setup();
    let c = ctx.client();
    let reporter = Address::generate(&ctx.env);
    c.configure_dynamic_fee(&ctx.admin, &fee_config(&reporter));

    // 50% volatility × 200 bps sensitivity → +100 bps.
    assert_eq!(c.report_market_conditions(&reporter, &10_000, &5_000), 200);
}

#[test]
fn test_fee_step_limited() {
    let ctx = setup();
    let c = ctx.client();
    let reporter = Address::generate(&ctx.env);
    let mut cfg = fee_config(&reporter);
    cfg.max_step_bps = 25;
    c.configure_dynamic_fee(&ctx.admin, &cfg);

    assert_eq!(c.report_market_conditions(&reporter, &1_000_000, &0), 125);
    assert_eq!(c.report_market_conditions(&reporter, &1_000_000, &0), 150);
    assert_eq!(c.report_market_conditions(&reporter, &0, &0), 125);
}

#[test]
fn test_preview_dynamic_fee_does_not_mutate() {
    let ctx = setup();
    let c = ctx.client();
    let reporter = Address::generate(&ctx.env);
    c.configure_dynamic_fee(&ctx.admin, &fee_config(&reporter));

    assert_eq!(c.preview_dynamic_fee(&20_000, &0), 200);
    assert_eq!(c.get_platform_fee_bps(), 100);
    assert_eq!(c.get_market_conditions(), None);
}

#[test]
fn test_admin_may_report_market_conditions() {
    let ctx = setup();
    let c = ctx.client();
    let reporter = Address::generate(&ctx.env);
    c.configure_dynamic_fee(&ctx.admin, &fee_config(&reporter));

    assert_eq!(c.report_market_conditions(&ctx.admin, &20_000, &0), 200);
}

#[test]
fn test_unauthorised_reporter_fails() {
    let ctx = setup();
    let c = ctx.client();
    let reporter = Address::generate(&ctx.env);
    let stranger = Address::generate(&ctx.env);
    c.configure_dynamic_fee(&ctx.admin, &fee_config(&reporter));

    assert!(c.try_report_market_conditions(&stranger, &20_000, &0).is_err());
}

#[test]
fn test_report_without_config_fails() {
    let ctx = setup();
    let c = ctx.client();
    assert!(c
        .try_report_market_conditions(&ctx.admin, &20_000, &0)
        .is_err());
}

#[test]
fn test_invalid_dynamic_fee_config_rejected() {
    let ctx = setup();
    let c = ctx.client();
    let reporter = Address::generate(&ctx.env);

    let mut cfg = fee_config(&reporter);
    cfg.min_bps = 200; // min > base
    assert!(c.try_configure_dynamic_fee(&ctx.admin, &cfg).is_err());

    let mut cfg = fee_config(&reporter);
    cfg.target_volume = 0;
    assert!(c.try_configure_dynamic_fee(&ctx.admin, &cfg).is_err());

    let mut cfg = fee_config(&reporter);
    cfg.max_bps = 10_001;
    assert!(c.try_configure_dynamic_fee(&ctx.admin, &cfg).is_err());
}

#[test]
fn test_disable_dynamic_fee_keeps_current_fee() {
    let ctx = setup();
    let c = ctx.client();
    let reporter = Address::generate(&ctx.env);
    c.configure_dynamic_fee(&ctx.admin, &fee_config(&reporter));
    c.report_market_conditions(&reporter, &20_000, &0);

    c.disable_dynamic_fee(&ctx.admin);

    assert_eq!(c.get_dynamic_fee_config(), None);
    assert_eq!(c.get_platform_fee_bps(), 200);
    assert!(c.try_report_market_conditions(&reporter, &0, &0).is_err());
}
