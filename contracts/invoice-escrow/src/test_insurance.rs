//! Tests for the insurance pool / automatic refund protection (#853).

use crate::errors::Error;
use crate::insurance::{DEFAULT_POLICY_DURATION, DEFAULT_PREMIUM_BPS};
use crate::test_helpers::{setup, Ctx};
use crate::types::{EscrowStatus, InsuranceConfig, PolicyStatus};
use soroban_sdk::testutils::Events;
use soroban_sdk::{symbol_short, vec, Address, IntoVal, Val, Vec};

/// Provider seeds 5 000 of liquidity; creator opens a 1 000 invoice; payer
/// deposits 400 and buys protection (premium 8 at the default 2 %).
fn insured(ctx: &Ctx) -> (Address, Address, Address, u64) {
    let provider = ctx.funded_user(5_000);
    ctx.client
        .provide_insurance_liquidity(&provider, &ctx.token, &5_000);

    let creator = ctx.user();
    let id = ctx.invoice(&creator, 1_000);
    let payer = ctx.funded_user(1_000);
    ctx.client.deposit(&payer, &id, &400);
    let premium = ctx.client.buy_refund_protection(&payer, &id);
    assert_eq!(premium, 8);
    (provider, creator, payer, id)
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[test]
fn test_default_config() {
    let ctx = setup();
    assert_eq!(
        ctx.client.get_insurance_config(),
        InsuranceConfig {
            premium_bps: DEFAULT_PREMIUM_BPS,
            policy_duration: DEFAULT_POLICY_DURATION,
        }
    );
}

#[test]
fn test_set_config_and_premium_follows_it() {
    let ctx = setup();
    ctx.client.set_insurance_config(&500u32, &1_000u64);
    assert!(ctx.has_event2(symbol_short!("insurance"), symbol_short!("config")));
    assert_eq!(ctx.client.get_insurance_config().premium_bps, 500);

    let provider = ctx.funded_user(5_000);
    ctx.client
        .provide_insurance_liquidity(&provider, &ctx.token, &5_000);
    let creator = ctx.user();
    let id = ctx.invoice(&creator, 1_000);
    let payer = ctx.funded_user(1_000);
    ctx.client.deposit(&payer, &id, &400);
    assert_eq!(ctx.client.buy_refund_protection(&payer, &id), 20);
    let policy = ctx.client.get_insurance_policy(&id, &payer).unwrap();
    assert_eq!(policy.expires_at, 1_000);
}

#[test]
fn test_set_config_rejects_out_of_range() {
    let ctx = setup();
    for (bps, duration) in [(0u32, 10u64), (5_001u32, 10u64), (100u32, 0u64)] {
        assert_eq!(
            ctx.client.try_set_insurance_config(&bps, &duration),
            Err(Ok(Error::InvalidConfig))
        );
    }
}

// ---------------------------------------------------------------------------
// Liquidity
// ---------------------------------------------------------------------------

#[test]
fn test_provide_liquidity_mints_shares() {
    let ctx = setup();
    let provider = ctx.funded_user(1_000);
    let shares = ctx
        .client
        .provide_insurance_liquidity(&provider, &ctx.token, &1_000);
    assert_eq!(shares, 1_000);
    assert_eq!(ctx.client.get_insurance_shares(&ctx.token, &provider), 1_000);
    let expected: Vec<Val> = (
        symbol_short!("insurance"),
        symbol_short!("lp_add"),
        ctx.token.clone(),
    )
        .into_val(&ctx.env);
    assert!(ctx
        .env
        .events()
        .all()
        .iter()
        .any(|(_, topics, _)| topics == expected));

    let pool = ctx.client.get_insurance_pool(&ctx.token);
    assert_eq!(pool.total_liquidity, 1_000);
    assert_eq!(pool.total_shares, 1_000);
    assert_eq!(ctx.contract_balance(), 1_000);

    assert_eq!(
        ctx.client
            .try_provide_insurance_liquidity(&provider, &ctx.token, &0),
        Err(Ok(Error::InvalidAmount))
    );
}

#[test]
fn test_premiums_accrue_to_existing_providers() {
    let ctx = setup();
    let (provider, _, _, _) = insured(&ctx);

    // Pool is now 5 008 liquidity / 5 000 shares; a new provider depositing
    // 5 008 receives exactly 5 000 shares.
    let late = ctx.funded_user(5_008);
    let shares = ctx
        .client
        .provide_insurance_liquidity(&late, &ctx.token, &5_008);
    assert_eq!(shares, 5_000);

    let pool = ctx.client.get_insurance_pool(&ctx.token);
    assert_eq!(pool.premiums_collected, 8);
    assert_eq!(pool.total_liquidity, 10_016);
    assert_eq!(ctx.client.get_insurance_shares(&ctx.token, &provider), 5_000);
}

#[test]
fn test_withdraw_limited_to_unlocked_liquidity() {
    let ctx = setup();
    let (provider, _, _, _) = insured(&ctx);

    // 5 000 shares are worth 5 008 but 400 is locked by the policy.
    assert_eq!(
        ctx.client
            .try_withdraw_insurance_liquidity(&provider, &ctx.token, &5_000),
        Err(Ok(Error::InsufficientPoolLiquidity))
    );
    assert_eq!(
        ctx.client
            .try_withdraw_insurance_liquidity(&provider, &ctx.token, &6_000),
        Err(Ok(Error::InsufficientShares))
    );

    let out = ctx
        .client
        .withdraw_insurance_liquidity(&provider, &ctx.token, &2_500);
    assert_eq!(out, 2_504);
    assert_eq!(ctx.balance(&provider), 2_504);
    assert_eq!(ctx.client.get_insurance_shares(&ctx.token, &provider), 2_500);
}

// ---------------------------------------------------------------------------
// Buying protection
// ---------------------------------------------------------------------------

#[test]
fn test_buy_protection_locks_coverage() {
    let ctx = setup();
    let (_, _, payer, id) = insured(&ctx);
    assert!(ctx.has_event(symbol_short!("insurance"), symbol_short!("policy"), id));

    let policy = ctx.client.get_insurance_policy(&id, &payer).unwrap();
    assert_eq!(policy.coverage, 400);
    assert_eq!(policy.premium, 8);
    assert_eq!(policy.status, PolicyStatus::Active);
    assert_eq!(policy.expires_at, DEFAULT_POLICY_DURATION);

    let pool = ctx.client.get_insurance_pool(&ctx.token);
    assert_eq!(pool.locked_coverage, 400);
    assert_eq!(pool.total_liquidity, 5_008);
    assert_eq!(ctx.balance(&payer), 1_000 - 400 - 8);
}

#[test]
fn test_buy_protection_errors() {
    let ctx = setup();
    let (_, creator, payer, id) = insured(&ctx);

    assert_eq!(
        ctx.client.try_buy_refund_protection(&payer, &id),
        Err(Ok(Error::PolicyAlreadyExists))
    );
    assert_eq!(
        ctx.client.try_buy_refund_protection(&ctx.user(), &id),
        Err(Ok(Error::NoDepositToInsure))
    );

    // Coverage larger than the unlocked pool liquidity is refused.
    let big = ctx.invoice(&creator, 10_000);
    let whale = ctx.funded_user(10_000);
    ctx.client.deposit(&whale, &big, &9_000);
    assert_eq!(
        ctx.client.try_buy_refund_protection(&whale, &big),
        Err(Ok(Error::InsufficientPoolLiquidity))
    );
}

#[test]
fn test_cannot_buy_after_release() {
    let ctx = setup();
    let (_, _, _, id) = insured(&ctx);
    let payer2 = ctx.funded_user(600);
    ctx.client.deposit(&payer2, &id, &600);
    assert_eq!(ctx.client.get_invoice(&id).status, EscrowStatus::Released);
    assert_eq!(
        ctx.client.try_buy_refund_protection(&payer2, &id),
        Err(Ok(Error::InvalidStatus))
    );
}

// ---------------------------------------------------------------------------
// Automatic refunds on default
// ---------------------------------------------------------------------------

#[test]
fn test_default_automatically_refunds_insured_payers() {
    let ctx = setup();
    let (_, creator, payer, id) = insured(&ctx);
    let uninsured = ctx.funded_user(600);
    ctx.client.deposit(&uninsured, &id, &600);
    assert_eq!(ctx.balance(&creator), 1_000);

    let paid = ctx.client.declare_invoice_default(&id);
    assert_eq!(paid, 400);
    assert!(ctx.has_event(symbol_short!("insurance"), symbol_short!("claimed"), id));
    assert!(ctx.has_event(symbol_short!("insurance"), symbol_short!("default"), id));
    assert!(ctx.client.is_invoice_defaulted(&id));

    assert_eq!(ctx.balance(&payer), 1_000 - 8);
    assert_eq!(ctx.balance(&uninsured), 0);
    assert_eq!(
        ctx.client.get_insurance_policy(&id, &payer).unwrap().status,
        PolicyStatus::Claimed
    );

    let pool = ctx.client.get_insurance_pool(&ctx.token);
    assert_eq!(pool.locked_coverage, 0);
    assert_eq!(pool.claims_paid, 400);
    assert_eq!(pool.total_liquidity, 4_608);
    assert_eq!(ctx.contract_balance(), 4_608);
}

#[test]
fn test_default_pays_multiple_policies() {
    let ctx = setup();
    let provider = ctx.funded_user(5_000);
    ctx.client
        .provide_insurance_liquidity(&provider, &ctx.token, &5_000);
    let creator = ctx.user();
    let id = ctx.invoice(&creator, 1_000);
    let p1 = ctx.funded_user(1_000);
    let p2 = ctx.funded_user(1_000);
    ctx.client.deposit(&p1, &id, &300);
    ctx.client.buy_refund_protection(&p1, &id);
    ctx.client.deposit(&p2, &id, &500);
    ctx.client.buy_refund_protection(&p2, &id);
    ctx.client.deposit(&p2, &id, &200);

    assert_eq!(ctx.client.declare_invoice_default(&id), 800);
    assert_eq!(ctx.balance(&p1), 1_000 - 6);
    assert_eq!(ctx.balance(&p2), 1_000 - 200 - 10);
}

#[test]
fn test_default_requires_release_and_is_one_shot() {
    let ctx = setup();
    let (_, _, _, id) = insured(&ctx);
    assert_eq!(
        ctx.client.try_declare_invoice_default(&id),
        Err(Ok(Error::InvalidStatus))
    );

    let payer2 = ctx.funded_user(600);
    ctx.client.deposit(&payer2, &id, &600);
    ctx.client.declare_invoice_default(&id);
    assert_eq!(
        ctx.client.try_declare_invoice_default(&id),
        Err(Ok(Error::InvoiceAlreadyDefaulted))
    );
}

#[test]
fn test_expired_policy_is_not_paid_on_default() {
    let ctx = setup();
    let (_, _, payer, id) = insured(&ctx);
    let payer2 = ctx.funded_user(600);
    ctx.client.deposit(&payer2, &id, &600);

    ctx.set_time(DEFAULT_POLICY_DURATION + 1);
    assert_eq!(ctx.client.declare_invoice_default(&id), 0);
    assert!(ctx.has_event(symbol_short!("insurance"), symbol_short!("expired"), id));
    assert_eq!(
        ctx.client.get_insurance_policy(&id, &payer).unwrap().status,
        PolicyStatus::Expired
    );
    assert_eq!(ctx.client.get_insurance_pool(&ctx.token).locked_coverage, 0);
    assert_eq!(ctx.balance(&payer), 1_000 - 400 - 8);
}

// ---------------------------------------------------------------------------
// Expiry
// ---------------------------------------------------------------------------

#[test]
fn test_expire_after_invoice_refund_unlocks_coverage() {
    let ctx = setup();
    let (_, _, payer, id) = insured(&ctx);

    assert_eq!(
        ctx.client.try_expire_refund_protection(&id, &payer),
        Err(Ok(Error::InvalidStatus))
    );

    ctx.set_time(1_001);
    ctx.client.refund(&id, &vec![&ctx.env, payer.clone()]);
    ctx.client.expire_refund_protection(&id, &payer);
    assert!(ctx.has_event(symbol_short!("insurance"), symbol_short!("expired"), id));

    assert_eq!(
        ctx.client.get_insurance_policy(&id, &payer).unwrap().status,
        PolicyStatus::Expired
    );
    assert_eq!(ctx.client.get_insurance_pool(&ctx.token).locked_coverage, 0);
    // Payer got the escrow deposit back; only the premium was spent.
    assert_eq!(ctx.balance(&payer), 1_000 - 8);

    assert_eq!(
        ctx.client.try_expire_refund_protection(&id, &payer),
        Err(Ok(Error::PolicyNotActive))
    );
}

#[test]
fn test_expire_unknown_policy() {
    let ctx = setup();
    let (_, _, _, id) = insured(&ctx);
    assert_eq!(
        ctx.client.try_expire_refund_protection(&id, &ctx.user()),
        Err(Ok(Error::PolicyNotFound))
    );
}
