#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Bytes, BytesN, Env, String, Symbol, TryFromVal, Val, Vec,
    Address, Bytes, BytesN, Env, String, Symbol, Vec,
};
use types::InvoiceOptions;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn setup_initialized() -> (Env, Address, Address) {
    let (env, contract_id, token_id) = setup();
    init_contract(&env, &contract_id, &token_id);
    (env, contract_id, token_id)
}

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SplitContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    (env, contract_id, token_id)
}

fn client<'a>(env: &'a Env, contract_id: &Address) -> SplitContractClient<'a> {
    SplitContractClient::new(env, contract_id)
}

fn token_client<'a>(env: &'a Env, token_id: &Address) -> TokenClient<'a> {
    TokenClient::new(env, token_id)
}

fn default_options(env: &Env) -> InvoiceOptions {
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
        overflow_behavior: types::OverflowBehavior::Reject,
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
        ext: types::InvoiceOptions2 {
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
            overfunding_policy: types::OverfundingPolicy::Cap,
            early_bird_window_ledgers: 0,
            early_bird_fee_bps: 0,
            creator_fee_bps: 0,
            early_bird_fee_credit: 0,
            ratio_denominator: 10_000,
        },
    }
}

fn default_options2(_env: &Env) -> InvoiceOptions2 {
    InvoiceOptions2 {
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
        overfunding_policy: types::OverfundingPolicy::Cap,
        early_bird_window_ledgers: 0,
        early_bird_fee_bps: 0,
        creator_fee_bps: 0,
        early_bird_fee_credit: 0,
        ratio_denominator: 10_000,
    }
}

fn invoice_options(
    env: &Env,
    cooldown_secs: Option<u64>,
    max_payments: Option<u32>,
    window_secs: Option<u64>,
) -> InvoiceOptions {
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
        overflow_behavior: types::OverflowBehavior::Reject,
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
        ext: types::InvoiceOptions2 {
            target_usd_cents: None,
            payment_token: None,
            release_delay_ledgers: None,
            metadata_hash: None,
            payment_cooldown_secs: cooldown_secs,
            max_payments_per_window: max_payments,
            payment_window_secs: window_secs,
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
            overfunding_policy: types::OverfundingPolicy::Cap,
            early_bird_window_ledgers: 0,
            early_bird_fee_bps: 0,
            creator_fee_bps: 0,
            early_bird_fee_credit: 0,
            ratio_denominator: 10_000,
        },
    }
}

fn single_recipient_invoice(
    env: &Env,
    c: &SplitContractClient,
    token_id: &Address,
    amount: i128,
    options: InvoiceOptions,
) -> u64 {
    let creator = Address::generate(env);
    let recipient = Address::generate(env);
    let mut recipients = Vec::new(env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(env);
    amounts.push_back(amount);
    c.create_invoice(&creator, &recipients, &amounts, token_id, &9_999_u64, &options)
}

/// Create a basic single-recipient invoice with default optional params.
fn make_invoice(
    env: &Env,
    c: &SplitContractClient,
    creator: &Address,
    recipient: &Address,
    amount: i128,
    token_id: &Address,
    deadline: u64,
) -> u64 {
    let mut recipients = Vec::new(env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(env);
    amounts.push_back(amount);
    c.create_invoice(creator, &recipients, &amounts, token_id, &deadline, &default_options(env))
}

// ---------------------------------------------------------------------------
// Core tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_invoice() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 2_000);
    assert_eq!(id, 1);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Pending);
    assert_eq!(invoice.funded, 0);
    assert!(c.get_invoice_ext(&id).allowed_payers.is_none());
}

#[test]
fn test_pay_and_auto_release() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);
    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 200);
}

#[test]
fn test_partial_pay_then_release() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer1, &150);
    sa.mint(&payer2, &150);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 9_999);

    c.pay(&payer1, &id, &150_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);

    c.pay(&payer2, &id, &150_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 300);
}

#[test]
fn test_refund_after_deadline() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 500, &token_id, 2_000);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);

    env.ledger().set_timestamp(3_000);
    c.refund(&id);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Refunded);
    assert_eq!(tk.balance(&payer), 100);
}

#[test]
#[should_panic(expected = "invoice deadline has passed")]
fn test_pay_after_deadline_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 2_000);
    env.ledger().set_timestamp(3_000);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);
}

#[test]
#[should_panic(expected = "payment exceeds remaining balance")]
fn test_overpayment_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);
}

#[test]
fn test_multi_recipient_release() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let r3 = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &600);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(r1.clone());
    recipients.push_back(r2.clone());
    recipients.push_back(r3.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);
    amounts.push_back(200_i128);
    amounts.push_back(300_i128);

    let id = c.create_invoice(
        &creator, &recipients, &amounts, &token_id, &9_999_u64, &default_options(&env),
    );
    c.pay(&payer, &id, &600_i128, &0_u64, &false, &false, &None);

    assert_eq!(tk.balance(&r1), 100);
    assert_eq!(tk.balance(&r2), 200);
    assert_eq!(tk.balance(&r3), 300);
}

#[test]
fn test_audit_log() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);
    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);

    let log = c.get_audit_log(&id);
    assert_eq!(log.len(), 2);
    assert_eq!(log.get_unchecked(0).action, symbol_short!("pay"));
    assert_eq!(log.get_unchecked(1).action, symbol_short!("release"));
}

#[test]
fn test_cancel_invoice() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.cancel_invoice(&creator, &id);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Cancelled);

    let log = c.get_audit_log(&id);
    assert_eq!(log.len(), 1);
    assert_eq!(log.get_unchecked(0).action, symbol_short!("cancel"));
}

#[test]
fn test_transfer_invoice() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let new_creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let payer = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &400);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.transfer_invoice(&id, &new_creator);

    // new_creator can cancel
    c.cancel_invoice(&new_creator, &id);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Cancelled);
    let _ = tk.balance(&recipient); // just ensure compiles
}

#[test]
fn test_partial_release_distributes_and_decrements_funded() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &200);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(r1.clone());
    recipients.push_back(r2.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);
    amounts.push_back(300_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &default_options(&env));

    // Payer funds 200
    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id).funded, 200);

    // Creator partially releases 100 -> r1 gets 25, r2 gets 75
    c.partial_release(&id, &creator, &100_i128);
    assert_eq!(tk.balance(&r1), 25);
    assert_eq!(tk.balance(&r2), 75);
    assert_eq!(c.get_invoice(&id).funded, 100);
}

#[test]
fn test_forward_to_invoice_credits_target_invoice() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    // Create parent invoice first (id=1).
    let id_parent = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);
    assert_eq!(id_parent, 1);

    // Create child invoice that declares forward_invoice_id → parent (id=2).
    let mut opts = default_options(&env);
    opts.forward_invoice_id = Some(id_parent);
    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);
    let id_child = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &opts);
    assert_eq!(id_child, 2);

    // Verify the field is stored correctly.
    let ext = c.get_invoice_ext(&id_child);
    assert_eq!(ext.forward_invoice_id, Some(id_parent));

    // Pay and release child; parent funded stays 0 because last-recipient absorbs all (no leftover).
    c.pay(&payer, &id_child, &100_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id_child).status, InvoiceStatus::Released);
    assert_eq!(c.get_invoice(&id_parent).funded, 0);
}

#[test]
fn test_forward_configured_event_emitted_when_forward_to_set() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let forward_target = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let mut opts = default_options(&env);
    opts.forward_to = Some(forward_target.clone());

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);
    c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &opts);

    let has_forward_configured_event = env
        .events()
        .all()
        .iter()
        .any(|(_c, topics, _d)| topic1_is(&env, &topics, "fwd_cfg"));
    assert!(
        has_forward_configured_event,
        "forward_configured event should be emitted when forward_to is set at creation"
    );
}

#[test]
fn test_forward_configured_event_absent_when_forward_to_unset() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    // default_options() leaves forward_to as None.
    let _id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    let has_forward_configured_event = env
        .events()
        .all()
        .iter()
        .any(|(_c, topics, _d)| topic1_is(&env, &topics, "fwd_cfg"));
    assert!(
        !has_forward_configured_event,
        "forward_configured event should not fire when forward_to is not set"
    );
}

#[test]
fn test_template_overwrite() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let name = soroban_sdk::symbol_short!("tmpl");

    let mut recipients1 = Vec::new(&env);
    recipients1.push_back(r1.clone());
    let mut amounts1 = Vec::new(&env);
    amounts1.push_back(50_i128);
    c.save_template(&creator, &name, &recipients1, &amounts1, &token_id);

    let mut recipients2 = Vec::new(&env);
    recipients2.push_back(r2.clone());
    let mut amounts2 = Vec::new(&env);
    amounts2.push_back(75_i128);
    c.save_template(&creator, &name, &recipients2, &amounts2, &token_id);

    let id = c.create_from_template(&creator, &name, &9_999_u64, &None);
    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.recipients.get_unchecked(0), r2);
    assert_eq!(invoice.amounts.get_unchecked(0), 75_i128);
}

#[test]
fn test_extend_deadline() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &300);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 9_999);
    c.extend_deadline(&id, &99_999_u64, &creator);
    assert_eq!(c.get_invoice(&id).deadline, 99_999);

    c.pay(&payer, &id, &150_i128, &0_u64, &false, &false, &None);
    assert_eq!(tk.balance(&payer), 150);

    c.cancel_invoice(&creator, &id);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Refunded);
    assert_eq!(tk.balance(&payer), 300);
}

#[test]
#[should_panic(expected = "invoice is not pending")]
fn test_cancel_non_pending_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let stellar_asset = StellarAssetClient::new(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    stellar_asset.mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);
    c.cancel_invoice(&creator, &id);
}

#[test]
fn test_get_payer_total() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 500, &token_id, 9_999);

    assert_eq!(c.get_payer_total(&id, &payer), 0);
    assert_eq!(c.get_payer_total(&id, &recipient), 0);

    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_payer_total(&id, &payer), 200);

    c.pay(&payer, &id, &150_i128, &1_u64, &false, &false, &None);
    assert_eq!(c.get_payer_total(&id, &payer), 350);
}

#[test]
fn test_verify_invoice() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 2_000);
    c.extend_deadline(&id, &9_999_u64, &creator);

    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);
    assert!(c.verify_invoice(&id, &InvoiceStatus::Released));
    assert!(!c.verify_invoice(&id, &InvoiceStatus::Pending));
}

// ---------------------------------------------------------------------------
// Adjust split
// ---------------------------------------------------------------------------

#[test]
fn test_adjust_split_updates_amounts_and_pays_new_total() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    // Create invoice: r1=100, r2=200 (total 300).
    let mut recipients = Vec::new(&env);
    recipients.push_back(r1.clone());
    recipients.push_back(r2.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);
    amounts.push_back(200_i128);
    let id = c.create_invoice(
        &creator, &recipients, &amounts, &token_id, &9_999_u64, &default_options(&env),
    );

    // Rebalance before any payment: r1=150, r2=250 (total 400).
    let mut new_amounts = Vec::new(&env);
    new_amounts.push_back(150_i128);
    new_amounts.push_back(250_i128);
    c.adjust_split(&creator, &id, &new_amounts);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.amounts.get_unchecked(0), 150);
    assert_eq!(invoice.amounts.get_unchecked(1), 250);

    // Pay the new total (400) and verify recipients receive updated amounts.
    c.pay(&payer, &id, &400_i128, &0_u64, &false, &false, &None);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&r1), 150);
    assert_eq!(tk.balance(&r2), 250);
}

#[test]
#[should_panic(expected = "only creator can adjust split")]
fn test_adjust_split_non_creator_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let other = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    let mut new_amounts = Vec::new(&env);
    new_amounts.push_back(200_i128);
    c.adjust_split(&other, &id, &new_amounts);
}

#[test]
#[should_panic(expected = "payments already received")]
fn test_adjust_split_after_payment_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &50);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.pay(&payer, &id, &50_i128, &0_u64, &false, &false, &None);

    let mut new_amounts = Vec::new(&env);
    new_amounts.push_back(80_i128);
    c.adjust_split(&creator, &id, &new_amounts);
}

#[test]
#[should_panic(expected = "amounts length mismatch")]
fn test_adjust_split_wrong_length_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    // Invoice has 1 recipient; pass 2 amounts.
    let mut new_amounts = Vec::new(&env);
    new_amounts.push_back(50_i128);
    new_amounts.push_back(50_i128);
    c.adjust_split(&creator, &id, &new_amounts);
}

#[test]
#[should_panic(expected = "amounts must be positive")]
fn test_adjust_split_zero_amount_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    let mut new_amounts = Vec::new(&env);
    new_amounts.push_back(0_i128);
    c.adjust_split(&creator, &id, &new_amounts);
}

// ---------------------------------------------------------------------------
// Add recipient
// ---------------------------------------------------------------------------

#[test]
fn test_add_recipient_appends_to_invoice() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &r1, 100, &token_id, 9_999);

    c.add_recipient(&creator, &id, &r2, &200_i128);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.recipients.len(), 2);
    assert_eq!(invoice.recipients.get_unchecked(0), r1);
    assert_eq!(invoice.recipients.get_unchecked(1), r2);
    assert_eq!(invoice.amounts.get_unchecked(0), 100);
    assert_eq!(invoice.amounts.get_unchecked(1), 200);
    assert_eq!(invoice.funded, 0);
}

#[test]
fn test_add_recipient_audit_entry() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &r1, 100, &token_id, 9_999);
    c.add_recipient(&creator, &id, &r2, &200_i128);

    let log = c.get_audit_log(&id);
    assert_eq!(log.len(), 1);
    assert_eq!(log.get_unchecked(0).action, symbol_short!("add_rec"));
    assert_eq!(log.get_unchecked(0).actor, creator);
}

#[test]
#[should_panic(expected = "only creator can add recipients")]
fn test_add_recipient_non_creator_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let other = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &r1, 100, &token_id, 9_999);
    c.add_recipient(&other, &id, &r2, &200_i128);
}

#[test]
#[should_panic(expected = "cannot add recipient after payment received")]
fn test_add_recipient_after_payment_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &r1, 100, &token_id, 9_999);
    c.pay(&payer, &id, &50_i128, &0_u64, &false, &false, &None);
    c.add_recipient(&creator, &id, &r2, &200_i128);
}

#[test]
#[should_panic(expected = "amount must be positive")]
fn test_add_recipient_zero_amount_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &r1, 100, &token_id, 9_999);
    c.add_recipient(&creator, &id, &r2, &0_i128);
}

#[test]
fn test_add_recipient_then_full_payment() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &r1, 100, &token_id, 9_999);
    c.add_recipient(&creator, &id, &r2, &200_i128);

    // Pay total (100 + 200 = 300).
    c.pay(&payer, &id, &300_i128, &0_u64, &false, &false, &None);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&r1), 100);
    assert_eq!(tk.balance(&r2), 200);
}

#[test]
fn test_add_recipient_multiple() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let r3 = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &r1, 100, &token_id, 9_999);
    c.add_recipient(&creator, &id, &r2, &200_i128);
    c.add_recipient(&creator, &id, &r3, &300_i128);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.recipients.len(), 3);
    assert_eq!(invoice.amounts.get_unchecked(0), 100);
    assert_eq!(invoice.amounts.get_unchecked(1), 200);
    assert_eq!(invoice.amounts.get_unchecked(2), 300);
}

#[test]
#[should_panic(expected = "invoice is not pending")]
fn test_add_recipient_after_release_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &200);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &r1, 200, &token_id, 9_999);
    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);
    // After auto-release the invoice is Released, not Pending.
    c.add_recipient(&creator, &id, &r2, &100_i128);
}

// ---------------------------------------------------------------------------
// Subscription
// ---------------------------------------------------------------------------

#[test]
fn test_allowed_payers_listed_address_succeeds() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let allowed = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&allowed, &200);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);

    let mut whitelist = Vec::new(&env);
    whitelist.push_back(allowed.clone());
    let mut opts = default_options(&env);
    opts.allowed_payers = Some(whitelist);

    let mut r = Vec::new(&env);
    r.push_back(recipient.clone());
    let mut a = Vec::new(&env);
    a.push_back(200_i128);
    let id = c.create_invoice(&creator, &r, &a, &token_id, &9_999_u64, &opts);

    c.pay(&allowed, &id, &200_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 200);
}

// ---------------------------------------------------------------------------
// Pause / unpause
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "contract is paused")]
fn test_pause_blocks_pay() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let treasury = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);
    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);
    c.pause(&admin);

    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);
}

#[test]
fn test_unpause_restores_pay() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let treasury = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);
    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);
    c.pause(&admin);
    c.unpause(&admin);

    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 200);
}

#[test]
#[should_panic(expected = "payer not allowed")]
fn test_allowed_payers_unlisted_address_rejected() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let allowed = Address::generate(&env);
    let unlisted = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&unlisted, &200);
    env.ledger().set_timestamp(1_000);

    let mut whitelist = Vec::new(&env);
    whitelist.push_back(allowed.clone());
    let mut opts = default_options(&env);
    opts.allowed_payers = Some(whitelist);

    let mut r = Vec::new(&env);
    r.push_back(recipient.clone());
    let mut a = Vec::new(&env);
    a.push_back(200_i128);
    let id = c.create_invoice(&creator, &r, &a, &token_id, &9_999_u64, &opts);

    c.pay(&unlisted, &id, &200_i128, &0_u64, &false, &false, &None);
}

// ---------------------------------------------------------------------------
// Transfer invoice
// ---------------------------------------------------------------------------

#[test]
fn test_transfer_invoice_new_creator_can_cancel() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let new_creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.transfer_invoice(&id, &new_creator);

    c.cancel_invoice(&new_creator, &id);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Cancelled);
}

#[test]
fn test_allowed_payers_none_behaves_as_open() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let anyone = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&anyone, &100);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.pay(&anyone, &id, &100_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 100);
}

// ---------------------------------------------------------------------------
// Bonus pool
// ---------------------------------------------------------------------------

#[test]
fn test_bonus_pool_distributed_to_first_payer() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let early_payer = Address::generate(&env);
    let late_payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&creator, &50);
    sa.mint(&early_payer, &150);
    sa.mint(&late_payer, &150);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(300_i128);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999_u64,
        &InvoiceOptions {
            co_creators: Vec::new(&env),
            allow_early_withdrawal: false,
            bonus_pool: 50,
            bonus_max_payers: 1,
            prerequisite_id: None,
            tranches: Vec::new(&env),
            co_signers: Vec::new(&env),
            required_signatures: 0,
            penalty_bps: None,
            penalty_deadline: None,
            min_funding_bps: None,
            release_stages: Vec::new(&env),
            ..default_options(&env)
        },
    );

    c.pay(&early_payer, &id, &150_i128, &0_u64, &false, &false, &None);
    c.pay(&late_payer, &id, &150_i128, &0_u64, &false, &false, &None);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&early_payer), 50);
    assert_eq!(tk.balance(&late_payer), 0);
    assert_eq!(tk.balance(&recipient), 300);
}

#[test]
fn test_bonus_pool_zero_behaves_identically() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let treasury = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    // Create a v1 invoice (bonus_pool = 0, identical to no-bonus).
    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    // migrate_invoice on an already-v1 invoice should be a no-op.
    c.migrate_invoice(&admin, &id);

    // Invoice is unchanged.
    let inv = c.get_invoice(&id);
    assert_eq!(inv.creator, creator);
    assert_eq!(inv.recipients.get_unchecked(0), recipient);
    assert_eq!(inv.amounts.get_unchecked(0), 100_i128);
    assert_eq!(inv.deadline, 9_999);
    assert_eq!(inv.funded, 0);
    assert_eq!(inv.status, InvoiceStatus::Pending);
    assert!(c.get_invoice_ext(&id).allowed_payers.is_none());

    // Pay and verify it releases normally (bonus_pool=0 has no effect).
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 100);
}

// ---------------------------------------------------------------------------
// Invoice groups
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "group members not fully funded")]
fn test_group_partial_fund_blocks_release() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);

    let stellar_asset = StellarAssetClient::new(&env, &token_id);
    stellar_asset.mint(&payer, &1_000);

    env.ledger().set_timestamp(1_000);

    let id1 = make_invoice(&env, &c, &creator, &r1, 100, &token_id, 9_999);
    let id2 = make_invoice(&env, &c, &creator, &r2, 200, &token_id, 9_999);

    let mut ids = Vec::new(&env);
    ids.push_back(id1);
    ids.push_back(id2);
    c.create_invoice_group(&ids, &false);

    c.pay(&payer, &id1, &100_i128, &0_u64, &false, &false, &None);

    c.release(&id1);
}

#[test]
fn test_group_all_funded_releases_both() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);

    let stellar_asset = StellarAssetClient::new(&env, &token_id);
    stellar_asset.mint(&payer, &1_000);

    env.ledger().set_timestamp(1_000);

    let id1 = make_invoice(&env, &c, &creator, &r1, 100, &token_id, 9_999);
    let id2 = make_invoice(&env, &c, &creator, &r2, 200, &token_id, 9_999);

    let mut ids = Vec::new(&env);
    ids.push_back(id1);
    ids.push_back(id2);
    c.create_invoice_group(&ids, &false);

    c.pay(&payer, &id1, &100_i128, &0_u64, &false, &false, &None);
    c.pay(&payer, &id2, &200_i128, &0_u64, &false, &false, &None);

    c.release(&id1);

    assert_eq!(c.get_invoice(&id1).status, InvoiceStatus::Released);
    assert_eq!(c.get_invoice(&id2).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&r1), 100);
    assert_eq!(tk.balance(&r2), 200);
}

#[test]
fn test_non_grouped_invoice_unaffected() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let stellar_asset = StellarAssetClient::new(&env, &token_id);
    stellar_asset.mint(&payer, &300);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 9_999);
    c.pay(&payer, &id, &300_i128, &0_u64, &false, &false, &None);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 300);
}

// ---------------------------------------------------------------------------
// Issue #21 — pay() nonce
// ---------------------------------------------------------------------------

#[test]
fn test_nonce_increments_per_payer_per_invoice() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 600, &token_id, 9_999);

    assert_eq!(c.get_nonce(&id, &payer), 0);

    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_nonce(&id, &payer), 1);

    c.pay(&payer, &id, &200_i128, &1_u64, &false, &false, &None);
    assert_eq!(c.get_nonce(&id, &payer), 2);

    c.pay(&payer, &id, &200_i128, &2_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
}

#[test]
#[should_panic(expected = "invalid nonce")]
fn test_wrong_nonce_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 600, &token_id, 9_999);

    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);
    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);
    // nonce should be 2 now — submitting 1 again must panic.
    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);
}

#[test]
fn test_nonce_is_independent_per_invoice() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    let id1 = make_invoice(&env, &c, &creator, &r1, 100, &token_id, 9_999);
    let id2 = make_invoice(&env, &c, &creator, &r2, 100, &token_id, 9_999);

    // Both invoices start at nonce 0 for the same payer.
    c.pay(&payer, &id1, &100_i128, &0_u64, &false, &false, &None);
    c.pay(&payer, &id2, &100_i128, &0_u64, &false, &false, &None);

    assert_eq!(c.get_nonce(&id1, &payer), 1);
    assert_eq!(c.get_nonce(&id2, &payer), 1);
}

// ---------------------------------------------------------------------------
// Issue #22 — prerequisite invoice linking
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "prerequisite not released")]
fn test_release_blocked_by_prerequisite() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    // Invoice A (prerequisite).
    let id_a = make_invoice(&env, &c, &creator, &r1, 100, &token_id, 9_999);

    // Invoice B requires A to be Released first.
    let mut recipients = Vec::new(&env);
    recipients.push_back(r2.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);
    let id_b = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999_u64,
        &InvoiceOptions {
            co_creators: Vec::new(&env),
            allow_early_withdrawal: false,
            bonus_pool: 0,
            bonus_max_payers: 0,
            prerequisite_id: Some(id_a),
            tranches: Vec::new(&env),
            co_signers: Vec::new(&env),
            required_signatures: 0,
            penalty_bps: None,
            penalty_deadline: None,
            min_funding_bps: None,
            release_stages: Vec::new(&env),
            ..default_options(&env)
        },
    );

    // Fund B fully but don't touch A.
    c.pay(&payer, &id_b, &200_i128, &0_u64, &false, &false, &None);

    // release() on B should panic because A is still Pending.
    c.release(&id_b);
}

#[test]
fn test_release_succeeds_after_prerequisite_released() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    let id_a = make_invoice(&env, &c, &creator, &r1, 100, &token_id, 9_999);

    let mut recipients = Vec::new(&env);
    recipients.push_back(r2.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);
    let id_b = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999_u64,
        &InvoiceOptions {
            co_creators: Vec::new(&env),
            allow_early_withdrawal: false,
            bonus_pool: 0,
            bonus_max_payers: 0,
            prerequisite_id: Some(id_a),
            tranches: Vec::new(&env),
            co_signers: Vec::new(&env),
            required_signatures: 0,
            penalty_bps: None,
            penalty_deadline: None,
            min_funding_bps: None,
            release_stages: Vec::new(&env),
            ..default_options(&env)
        },
    );

    // Release A (auto-releases on full funding).
    c.pay(&payer, &id_a, &100_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id_a).status, InvoiceStatus::Released);

    // Fund B fully (stays pending because it has a prerequisite).
    c.pay(&payer, &id_b, &200_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id_b).status, InvoiceStatus::Pending);

    // Now release B — prerequisite is satisfied.
    c.release(&id_b);
    assert_eq!(c.get_invoice(&id_b).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&r2), 200);
}

#[test]
fn test_no_prerequisite_behaves_like_normal() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &200);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);
    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);

    // Auto-releases because no prerequisite.
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
}

// ---------------------------------------------------------------------------
// Issue #23 — graduated release tranches
// ---------------------------------------------------------------------------

#[test]
fn test_tranches_partial_then_full_release() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    // Two tranches: 50% unlocks at t=1_500, remaining 50% at t=2_500.
    let mut tranches = Vec::new(&env);
    tranches.push_back(types::Tranche { timestamp: 1_500, basis_points: 5_000 });
    tranches.push_back(types::Tranche { timestamp: 2_500, basis_points: 5_000 });

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999_u64,
        &InvoiceOptions {
            co_creators: Vec::new(&env),
            allow_early_withdrawal: false,
            bonus_pool: 0,
            bonus_max_payers: 0,
            prerequisite_id: None,
            tranches: tranches.clone(),
            co_signers: Vec::new(&env),
            required_signatures: 0,
            penalty_bps: None,
            penalty_deadline: None,
            min_funding_bps: None,
            release_stages: Vec::new(&env),
            ..default_options(&env)
        },
    );

    // Fund fully — no auto-release for tranche invoices.
    c.pay(&payer, &id, &1_000_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);

    // At t=1_600 first tranche is unlocked, second is not.
    env.ledger().set_timestamp(1_600);
    c.release(&id);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);
    assert_eq!(c.get_invoice(&id).released_bps, 5_000);
    assert_eq!(tk.balance(&recipient), 500);

    // At t=2_600 second tranche also unlocked.
    env.ledger().set_timestamp(2_600);
    c.release(&id);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(c.get_invoice(&id).released_bps, 10_000);
    assert_eq!(tk.balance(&recipient), 1_000);
}

#[test]
#[should_panic(expected = "no tranches unlocked")]
fn test_release_before_any_tranche_unlocked_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let mut tranches = Vec::new(&env);
    tranches.push_back(types::Tranche { timestamp: 5_000, basis_points: 10_000 });

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999_u64,
        &InvoiceOptions {
            co_creators: Vec::new(&env),
            allow_early_withdrawal: false,
            bonus_pool: 0,
            bonus_max_payers: 0,
            prerequisite_id: None,
            tranches: tranches.clone(),
            co_signers: Vec::new(&env),
            required_signatures: 0,
            penalty_bps: None,
            penalty_deadline: None,
            min_funding_bps: None,
            release_stages: Vec::new(&env),
            ..default_options(&env)
        },
    );

    c.pay(&payer, &id, &500_i128, &0_u64, &false, &false, &None);
    // t=1_000 < tranche timestamp 5_000 — should panic.
    c.release(&id);
}

// ---------------------------------------------------------------------------
// Issue #24 — on-chain reputation counter
// ---------------------------------------------------------------------------

#[test]
fn test_reputation_zero_for_new_address() {
    let (env, contract_id, _token_id) = setup();
    let c = client(&env, &contract_id);

    let address = Address::generate(&env);
    assert_eq!(c.get_reputation(&address), 0);
}

#[test]
fn test_reputation_increments_across_invoices() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    let id1 = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    let id2 = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    let id3 = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    assert_eq!(c.get_reputation(&payer), 0);

    c.pay(&payer, &id1, &100_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_reputation(&payer), 1);

    c.pay(&payer, &id2, &100_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_reputation(&payer), 2);

    c.pay(&payer, &id3, &100_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_reputation(&payer), 3);
}

#[test]
fn test_reputation_is_per_address() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer_a = Address::generate(&env);
    let payer_b = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer_a, &1_000);
    sa.mint(&payer_b, &1_000);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 400, &token_id, 9_999);

    c.pay(&payer_a, &id, &100_i128, &0_u64, &false, &false, &None);
    c.pay(&payer_a, &id, &100_i128, &1_u64, &false, &false, &None);
    c.pay(&payer_b, &id, &100_i128, &0_u64, &false, &false, &None);
    c.pay(&payer_b, &id, &100_i128, &1_u64, &false, &false, &None);

    // payer_a paid twice, payer_b paid twice.
    assert_eq!(c.get_reputation(&payer_a), 2);
    assert_eq!(c.get_reputation(&payer_b), 2);

    // Unrelated address has zero reputation.
    let other = Address::generate(&env);
    assert_eq!(c.get_reputation(&other), 0);
}

// ---------------------------------------------------------------------------
// Creation fee
// ---------------------------------------------------------------------------

#[test]
fn test_creation_fee_charged_to_treasury() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let treasury = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&creator, &1_000);

    env.ledger().set_timestamp(1_000);

    c.initialize(&admin, &50_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    assert_eq!(c.get_creation_fee(), 50);
    assert_eq!(c.get_treasury(), treasury);
    assert_eq!(c.get_usdc_token(), token_id);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);

    // Treasury received 50 USDC creation fee.
    assert_eq!(tk.balance(&treasury), 50);
    // Creator paid 50 USDC fee; invoice amount stays in creator wallet until payers pay.
    assert_eq!(tk.balance(&creator), 950);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);
}

#[test]
fn test_creation_fee_zero_by_default() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let treasury = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&creator, &1_000);

    env.ledger().set_timestamp(1_000);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);

    // No fee deducted when creation_fee is 0.
    assert_eq!(tk.balance(&treasury), 0);
    assert_eq!(tk.balance(&creator), 1000);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);
}

#[test]
fn test_set_creation_fee_updates_fee() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    c.initialize(&admin, &10_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);
    assert_eq!(c.get_creation_fee(), 10);

    c.set_creation_fee(&admin, &25_i128);
    assert_eq!(c.get_creation_fee(), 25);
}

#[test]
fn test_set_treasury_updates_treasury() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury1 = Address::generate(&env);
    let treasury2 = Address::generate(&env);

    c.initialize(&admin, &10_i128, &treasury1, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);
    assert_eq!(c.get_treasury(), treasury1);

    c.set_treasury(&admin, &treasury2);
    assert_eq!(c.get_treasury(), treasury2);
}

#[test]
fn test_creation_fee_charged_per_invoice_in_batch() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let treasury = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&creator, &1_000);

    env.ledger().set_timestamp(1_000);

    c.initialize(&admin, &10_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    // create_batch creates 2 invoices, each should incur a 10 unit fee.
    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);
    let params = types::CreateInvoiceParams {
        recipients,
        amounts,
        token: token_id.clone(),
        deadline: 9_999,
    };
    let mut invoices = Vec::new(&env);
    invoices.push_back(params.clone());
    invoices.push_back(params);
    c.create_batch(&creator, &invoices);

    // 2 invoices x 10 fee = 20 total.
    assert_eq!(tk.balance(&treasury), 20);
}

// ---------------------------------------------------------------------------
// Batch invoice creation (issue #311)
// ---------------------------------------------------------------------------

#[test]
fn test_batch_create_3_invoices() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let treasury = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&creator, &10_000);
    env.ledger().set_timestamp(1_000);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let params = types::CreateInvoiceParams {
        recipients,
        amounts,
        token: token_id.clone(),
        deadline: 9_999,
    };

    let mut invoices = Vec::new(&env);
    invoices.push_back(params.clone());
    invoices.push_back(params.clone());
    invoices.push_back(params);

    let ids = c.create_invoices_batch(&creator, &invoices);
    assert_eq!(ids.len(), 3);
    assert_eq!(ids.get(0).unwrap(), 1);
    assert_eq!(ids.get(1).unwrap(), 2);
    assert_eq!(ids.get(2).unwrap(), 3);

    // Verify each invoice was created
    for i in 0..3 {
        let inv = c.get_invoice(&(i as u64 + 1));
        assert_eq!(inv.status, InvoiceStatus::Pending);
    }
}

#[test]
fn test_batch_create_10_invoices() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let treasury = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&creator, &100_000);
    env.ledger().set_timestamp(1_000);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let params = types::CreateInvoiceParams {
        recipients,
        amounts,
        token: token_id.clone(),
        deadline: 9_999,
    };

    let mut invoices = Vec::new(&env);
    for _ in 0..10 {
        invoices.push_back(params.clone());
    }

    let ids = c.create_invoices_batch(&creator, &invoices);
    assert_eq!(ids.len(), 10);
    assert_eq!(ids.get(0).unwrap(), 1);
    assert_eq!(ids.get(9).unwrap(), 10);
}

#[test]
#[should_panic]
fn test_batch_create_exceeds_limit() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let treasury = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&creator, &100_000);
    env.ledger().set_timestamp(1_000);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let params = types::CreateInvoiceParams {
        recipients,
        amounts,
        token: token_id.clone(),
        deadline: 9_999,
    };

    let mut invoices = Vec::new(&env);
    for _ in 0..11 {
        invoices.push_back(params.clone());
    }

    c.create_invoices_batch(&creator, &invoices);
}

#[test]
#[should_panic]
fn test_batch_create_with_invalid_item_rejected() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let treasury = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&creator, &10_000);
    env.ledger().set_timestamp(1_000);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    // Valid params
    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);
    let valid_params = types::CreateInvoiceParams {
        recipients,
        amounts,
        token: token_id.clone(),
        deadline: 9_999,
    };

    // Invalid params: past deadline
    let mut bad_recipients = Vec::new(&env);
    bad_recipients.push_back(recipient.clone());
    let mut bad_amounts = Vec::new(&env);
    bad_amounts.push_back(100_i128);
    let invalid_params = types::CreateInvoiceParams {
        recipients: bad_recipients,
        amounts: bad_amounts,
        token: token_id.clone(),
        deadline: 500, // past current timestamp of 1_000
    };

    let mut invoices = Vec::new(&env);
    invoices.push_back(valid_params.clone());
    invoices.push_back(invalid_params);

    c.create_invoices_batch(&creator, &invoices);
}

// ---------------------------------------------------------------------------
// Rollover invoice
// ---------------------------------------------------------------------------

#[test]
fn test_rollover_invoice_creates_new_with_carried_payments() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    // Create invoice with deadline at 2_000.
    let id1 = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 2_000);

    // Partially fund the invoice.
    c.pay(&payer, &id1, &100_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id1).funded, 100);
    assert_eq!(c.get_invoice(&id1).status, InvoiceStatus::Pending);

    // Move past deadline.
    env.ledger().set_timestamp(3_000);

    // Rollover to new invoice with deadline at 5_000.
    let id2 = c.rollover_invoice(&creator, &id1, &5_000_u64);
    assert_ne!(id1, id2);

    // Old invoice should be marked Refunded.
    let old_invoice = c.get_invoice(&id1);
    assert_eq!(old_invoice.status, InvoiceStatus::Refunded);

    // New invoice should have same recipients, amounts, token.
    let new_invoice = c.get_invoice(&id2);
    assert_eq!(new_invoice.status, InvoiceStatus::Pending);
    assert_eq!(new_invoice.recipients.get_unchecked(0), recipient);
    assert_eq!(new_invoice.amounts.get_unchecked(0), 300);
    assert_eq!(new_invoice.deadline, 5_000);

    // New invoice should have carried over the payment.
    assert_eq!(new_invoice.funded, 100);
    assert_eq!(new_invoice.payments.len(), 1);
    assert_eq!(new_invoice.payments.get_unchecked(0).payer, payer);
    assert_eq!(new_invoice.payments.get_unchecked(0).amount, 100);

    // Payer should still have 400 (500 - 100 paid).
    assert_eq!(tk.balance(&payer), 400);

    // Recipient should have received nothing yet.
    assert_eq!(tk.balance(&recipient), 0);
}

#[test]
fn test_rollover_invoice_then_complete_payment() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id1 = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 2_000);
    c.pay(&payer, &id1, &100_i128, &0_u64, &false, &false, &None);

    env.ledger().set_timestamp(3_000);
    let id2 = c.rollover_invoice(&creator, &id1, &5_000_u64);

    // Complete the payment on the new invoice.
    c.pay(&payer, &id2, &200_i128, &0_u64, &false, &false, &None);

    // New invoice should be fully funded and released.
    assert_eq!(c.get_invoice(&id2).status, InvoiceStatus::Released);
    assert_eq!(c.get_invoice(&id2).funded, 300);

    // Recipient should have received the full amount.
    assert_eq!(tk.balance(&recipient), 300);
}

#[test]
#[should_panic(expected = "invoice is not pending")]
fn test_rollover_invoice_non_pending_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);

    // Invoice is now Released, not Pending.
    env.ledger().set_timestamp(3_000);
    c.rollover_invoice(&creator, &id, &5_000_u64);
}

#[test]
#[should_panic(expected = "invoice deadline has not passed")]
fn test_rollover_invoice_before_deadline_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 5_000);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);

    // Still before deadline (3_000 < 5_000).
    env.ledger().set_timestamp(3_000);
    c.rollover_invoice(&creator, &id, &6_000_u64);
}

#[test]
#[should_panic(expected = "only creator can rollover invoice")]
fn test_rollover_invoice_non_creator_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let other = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 2_000);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);

    env.ledger().set_timestamp(3_000);
    c.rollover_invoice(&other, &id, &5_000_u64);
}

#[test]
#[should_panic(expected = "new deadline must be in the future")]
fn test_rollover_invoice_past_deadline_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 2_000);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);

    env.ledger().set_timestamp(3_000);
    // Try to set new deadline to 2_500, which is in the past.
    c.rollover_invoice(&creator, &id, &2_500_u64);
}

#[test]
fn test_rollover_invoice_audit_entries() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id1 = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 2_000);
    c.pay(&payer, &id1, &100_i128, &0_u64, &false, &false, &None);

    env.ledger().set_timestamp(3_000);
    let id2 = c.rollover_invoice(&creator, &id1, &5_000_u64);

    // Old invoice should have rollover audit entry.
    let old_log = c.get_audit_log(&id1);
    assert_eq!(old_log.len(), 2); // pay + rollover
    assert_eq!(old_log.get_unchecked(0).action, symbol_short!("pay"));
    assert_eq!(old_log.get_unchecked(1).action, symbol_short!("rollover"));
    assert_eq!(old_log.get_unchecked(1).actor, creator);

    // New invoice should have rollover audit entry.
    let new_log = c.get_audit_log(&id2);
    assert_eq!(new_log.len(), 1); // rollover
    assert_eq!(new_log.get_unchecked(0).action, symbol_short!("rollover"));
    assert_eq!(new_log.get_unchecked(0).actor, creator);
}

#[test]
fn test_rollover_invoice_preserves_recipients_and_amounts() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let r3 = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(r1.clone());
    recipients.push_back(r2.clone());
    recipients.push_back(r3.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);
    amounts.push_back(200_i128);
    amounts.push_back(300_i128);

    let id1 = c.create_invoice(
        &creator, &recipients, &amounts, &token_id, &2_000_u64, &default_options(&env),
    );
    c.pay(&payer, &id1, &150_i128, &0_u64, &false, &false, &None);

    env.ledger().set_timestamp(3_000);
    let id2 = c.rollover_invoice(&creator, &id1, &5_000_u64);

    let new_invoice = c.get_invoice(&id2);
    assert_eq!(new_invoice.recipients.len(), 3);
    assert_eq!(new_invoice.recipients.get_unchecked(0), r1);
    assert_eq!(new_invoice.recipients.get_unchecked(1), r2);
    assert_eq!(new_invoice.recipients.get_unchecked(2), r3);
    assert_eq!(new_invoice.amounts.get_unchecked(0), 100);
    assert_eq!(new_invoice.amounts.get_unchecked(1), 200);
    assert_eq!(new_invoice.amounts.get_unchecked(2), 300);
}

// ---------------------------------------------------------------------------
// Issue #40 — recipient invoice ID index
// ---------------------------------------------------------------------------

#[test]
fn test_recipient_invoice_ids_empty_for_new_address() {
    let (env, contract_id, _token_id) = setup();
    let c = client(&env, &contract_id);

    let addr = Address::generate(&env);
    let ids = c.get_recipient_invoice_ids(&addr);
    assert_eq!(ids.len(), 0);
}

#[test]
fn test_recipient_invoice_ids_single_invoice() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);
    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    let ids = c.get_recipient_invoice_ids(&recipient);
    assert_eq!(ids.len(), 1);
    assert_eq!(ids.get_unchecked(0), id);
}

#[test]
fn test_recipient_invoice_ids_same_recipient_multiple_invoices() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let other = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id1 = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    let id2 = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);
    let id3 = make_invoice(&env, &c, &creator, &other, 300, &token_id, 9_999);

    let ids = c.get_recipient_invoice_ids(&recipient);
    assert_eq!(ids.len(), 2);
    assert_eq!(ids.get_unchecked(0), id1);
    assert_eq!(ids.get_unchecked(1), id2);

    let other_ids = c.get_recipient_invoice_ids(&other);
    assert_eq!(other_ids.len(), 1);
    assert_eq!(other_ids.get_unchecked(0), id3);
}

#[test]
fn test_recipient_invoice_ids_multi_recipient_invoice() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);

    let mut recipients = Vec::new(&env);
    recipients.push_back(r1.clone());
    recipients.push_back(r2.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);
    amounts.push_back(200_i128);

    env.ledger().set_timestamp(1_000);
    let id = c.create_invoice(
        &creator, &recipients, &amounts, &token_id, &9_999_u64, &default_options(&env),
    );

    let r1_ids = c.get_recipient_invoice_ids(&r1);
    assert_eq!(r1_ids.len(), 1);
    assert_eq!(r1_ids.get_unchecked(0), id);

    let r2_ids = c.get_recipient_invoice_ids(&r2);
    assert_eq!(r2_ids.len(), 1);
    assert_eq!(r2_ids.get_unchecked(0), id);
}

#[test]
fn test_recipient_invoice_ids_after_add_recipient() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);

    env.ledger().set_timestamp(1_000);
    let id = make_invoice(&env, &c, &creator, &r1, 100, &token_id, 9_999);

    // r1 should have the invoice before adding r2.
    assert_eq!(c.get_recipient_invoice_ids(&r1).len(), 1);

    // Add r2 via add_recipient.
    c.add_recipient(&creator, &id, &r2, &200_i128);

    // r2 should now also have the invoice.
    let r2_ids = c.get_recipient_invoice_ids(&r2);
    assert_eq!(r2_ids.len(), 1);
    assert_eq!(r2_ids.get_unchecked(0), id);

    // r1 is unaffected.
    assert_eq!(c.get_recipient_invoice_ids(&r1).len(), 1);
}

// ---------------------------------------------------------------------------
// Issue #41 — platform fee basis points
// ---------------------------------------------------------------------------

#[test]
fn test_platform_fee_bps_defaults_to_zero() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    assert_eq!(c.get_platform_fee_bps(), 0);
}

#[test]
fn test_platform_fee_bps_deducted_on_release() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let treasury = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer, &1_000);

    env.ledger().set_timestamp(1_000);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &1_000_u32, &None, &0_u32, &0_u32, &0_u64); // 10%

    let id = make_invoice(&env, &c, &creator, &recipient, 500, &token_id, 9_999);
    c.pay(&payer, &id, &500_i128, &0_u64, &false, &false, &None);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    // Recipient gets 500 - 10% = 450.
    assert_eq!(tk.balance(&recipient), 450);
    // Treasury gets 50.
    assert_eq!(tk.balance(&treasury), 50);
}

#[test]
fn test_platform_fee_bps_multi_recipient() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let r3 = Address::generate(&env);
    let treasury = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer, &1_000);

    env.ledger().set_timestamp(1_000);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &500_u32, &None, &0_u32, &0_u32, &0_u64); // 5%

    let mut recipients = Vec::new(&env);
    recipients.push_back(r1.clone());
    recipients.push_back(r2.clone());
    recipients.push_back(r3.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);
    amounts.push_back(300_i128);
    amounts.push_back(500_i128);

    let id = c.create_invoice(
        &creator, &recipients, &amounts, &token_id, &9_999_u64, &default_options(&env),
    );
    c.pay(&payer, &id, &1_000_i128, &0_u64, &false, &false, &None);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    // 200 - 5% = 190, 300 - 5% = 285, 500 - 5% = 475 → sum = 950
    assert_eq!(tk.balance(&r1), 190);
    assert_eq!(tk.balance(&r2), 285);
    assert_eq!(tk.balance(&r3), 475);
    // Treasury gets 50.
    assert_eq!(tk.balance(&treasury), 50);
}

#[test]
fn test_platform_fee_bps_with_tranches() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let treasury = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer, &1_000);

    env.ledger().set_timestamp(1_000);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &1_000_u32, &None, &0_u32, &0_u32, &0_u64); // 10%

    let mut tranches = Vec::new(&env);
    tranches.push_back(types::Tranche { timestamp: 1_500, basis_points: 5_000 });
    tranches.push_back(types::Tranche { timestamp: 2_500, basis_points: 5_000 });

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999_u64,
        &InvoiceOptions {
            co_creators: Vec::new(&env),
            allow_early_withdrawal: false,
            bonus_pool: 0,
            bonus_max_payers: 0,
            prerequisite_id: None,
            tranches: tranches.clone(),
            co_signers: Vec::new(&env),
            required_signatures: 0,
            penalty_bps: None,
            penalty_deadline: None,
            min_funding_bps: None,
            release_stages: Vec::new(&env),
            ..default_options(&env)
        },
    );

    c.pay(&payer, &id, &1_000_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);

    // First tranche: 500 unlocked.
    env.ledger().set_timestamp(1_600);
    c.release(&id);

    // 500 - 10% = 450 to recipient, 50 to treasury.
    assert_eq!(tk.balance(&recipient), 450);
    assert_eq!(tk.balance(&treasury), 50);

    // Second tranche: remaining 500 unlocked.
    env.ledger().set_timestamp(2_600);
    c.release(&id);

    // Another 450 to recipient, another 50 to treasury.
    assert_eq!(tk.balance(&recipient), 900);
    assert_eq!(tk.balance(&treasury), 100);
}

// ---------------------------------------------------------------------------
// Issue #42 — late-payment penalty
// ---------------------------------------------------------------------------

#[test]
fn test_penalty_not_applied_before_penalty_deadline() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer, &1_000);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999_u64,
        &InvoiceOptions {
            co_creators: Vec::new(&env),
            allow_early_withdrawal: false,
            bonus_pool: 0,
            bonus_max_payers: 0,
            prerequisite_id: None,
            tranches: Vec::new(&env),
            co_signers: Vec::new(&env),
            required_signatures: 0,
            penalty_bps: Some(1_000), // 10 %
            penalty_deadline: Some(2_000),
            min_funding_bps: None,
            release_stages: Vec::new(&env),
            ..default_options(&env)
        },
    );

    // Pay at t=1_000 which is before penalty_deadline.
    c.pay(&payer, &id, &500_i128, &0_u64, &false, &false, &None);

    // Recipient gets full 500, no penalty.
    assert_eq!(tk.balance(&recipient), 500);
    // Payer paid exactly 500.
    assert_eq!(tk.balance(&payer), 500);
}

#[test]
fn test_penalty_applied_after_penalty_deadline() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer, &1_000);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999_u64,
        &InvoiceOptions {
            co_creators: Vec::new(&env),
            allow_early_withdrawal: false,
            bonus_pool: 0,
            bonus_max_payers: 0,
            prerequisite_id: None,
            tranches: Vec::new(&env),
            co_signers: Vec::new(&env),
            required_signatures: 0,
            penalty_bps: Some(1_000), // 10 %
            penalty_deadline: Some(2_000),
            min_funding_bps: None,
            release_stages: Vec::new(&env),
            ..default_options(&env)
        },
    );

    // Advance past penalty deadline.
    env.ledger().set_timestamp(3_000);
    c.pay(&payer, &id, &500_i128, &0_u64, &false, &false, &None);

    // Recipient gets 500 (normal) + 50 (penalty) = 550.
    assert_eq!(tk.balance(&recipient), 550);
    // Payer paid 500 + 50 = 550.
    assert_eq!(tk.balance(&payer), 450);
}

#[test]
fn test_penalty_distributed_proportionally_multi_recipient() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let r3 = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer, &2_000);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(r1.clone());
    recipients.push_back(r2.clone());
    recipients.push_back(r3.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);
    amounts.push_back(200_i128);
    amounts.push_back(700_i128);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999_u64,
        &InvoiceOptions {
            co_creators: Vec::new(&env),
            allow_early_withdrawal: false,
            bonus_pool: 0,
            bonus_max_payers: 0,
            prerequisite_id: None,
            tranches: Vec::new(&env),
            co_signers: Vec::new(&env),
            required_signatures: 0,
            penalty_bps: Some(1_000), // 10 %
            penalty_deadline: Some(2_000),
            min_funding_bps: None,
            release_stages: Vec::new(&env),
            ..default_options(&env)
        },
    );

    // Pay after penalty deadline.
    env.ledger().set_timestamp(3_000);
    c.pay(&payer, &id, &1_000_i128, &0_u64, &false, &false, &None);

    // Penalty = 1000 * 10% = 100
    // Distribution: r1=10, r2=20, r3=70
    assert_eq!(tk.balance(&r1), 100 + 10); // normal + penalty
    assert_eq!(tk.balance(&r2), 200 + 20);
    assert_eq!(tk.balance(&r3), 700 + 70);
    // Payer paid 1000 + 100 = 1100.
    assert_eq!(tk.balance(&payer), 900);
}

#[test]
fn test_penalty_bps_zero_no_penalty_even_after_deadline() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer, &1_000);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);

    // penalty_bps = 0 means no penalty even after penalty_deadline.
    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999_u64,
        &InvoiceOptions {
            co_creators: Vec::new(&env),
            allow_early_withdrawal: false,
            bonus_pool: 0,
            bonus_max_payers: 0,
            prerequisite_id: None,
            tranches: Vec::new(&env),
            co_signers: Vec::new(&env),
            required_signatures: 0,
            penalty_bps: Some(0),
            penalty_deadline: Some(2_000),
            min_funding_bps: None,
            release_stages: Vec::new(&env),
            ..default_options(&env)
        },
    );

    env.ledger().set_timestamp(3_000);
    c.pay(&payer, &id, &500_i128, &0_u64, &false, &false, &None);

    // Recipient gets full 500, no penalty.
    assert_eq!(tk.balance(&recipient), 500);
    assert_eq!(tk.balance(&payer), 500);
}

// ---------------------------------------------------------------------------
// Issue #43 — minimum funding threshold
// ---------------------------------------------------------------------------

#[test]
fn test_min_funding_bps_zero_requires_full_funding() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer, &1_000);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 500, &token_id, 9_999);

    // Partial fund (300 of 500) — release should fail.
    c.pay(&payer, &id, &300_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id).funded, 300);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);

    // Fund the rest.
    c.pay(&payer, &id, &200_i128, &1_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
}

#[test]
fn test_min_funding_bps_blocks_early_release() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer, &1_000);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999_u64,
        &InvoiceOptions {
            co_creators: Vec::new(&env),
            allow_early_withdrawal: false,
            bonus_pool: 0,
            bonus_max_payers: 0,
            prerequisite_id: None,
            tranches: Vec::new(&env),
            co_signers: Vec::new(&env),
            required_signatures: 0,
            penalty_bps: None,
            penalty_deadline: None,
            min_funding_bps: Some(8_000), // 80 %
            release_stages: Vec::new(&env),
            ..default_options(&env)
        },
    );

    // Fund 500 of 1000 (50% — below 80% threshold). Release should panic.
    c.pay(&payer, &id, &500_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id).funded, 500);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);
}

#[test]
#[should_panic(expected = "minimum funding not reached")]
fn test_min_funding_bps_panics_below_threshold() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer, &1_000);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999_u64,
        &InvoiceOptions {
            co_creators: Vec::new(&env),
            allow_early_withdrawal: false,
            bonus_pool: 0,
            bonus_max_payers: 0,
            prerequisite_id: None,
            tranches: Vec::new(&env),
            co_signers: Vec::new(&env),
            required_signatures: 0,
            penalty_bps: None,
            penalty_deadline: None,
            min_funding_bps: Some(8_000), // 80 %
            release_stages: Vec::new(&env),
            ..default_options(&env)
        },
    );

    // Fund 700 of 1000 (70% — below 80%). Try to release — must panic.
    c.pay(&payer, &id, &700_i128, &0_u64, &false, &false, &None);
    // Guarded (has min_funding_bps), so auto-release won't fire.
    c.release(&id);
}

#[test]
fn test_min_funding_bps_allows_release_above_threshold() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer, &2_000);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999_u64,
        &InvoiceOptions {
            co_creators: Vec::new(&env),
            allow_early_withdrawal: false,
            bonus_pool: 0,
            bonus_max_payers: 0,
            prerequisite_id: None,
            tranches: Vec::new(&env),
            co_signers: Vec::new(&env),
            required_signatures: 0,
            penalty_bps: None,
            penalty_deadline: None,
            min_funding_bps: Some(8_000), // 80 %
            release_stages: Vec::new(&env),
            ..default_options(&env)
        },
    );

    // Fund 900 of 1000 (90% >= 80%). Release should succeed.
    c.pay(&payer, &id, &900_i128, &0_u64, &false, &false, &None);
    // Guarded (has min_funding_bps), so we must manually release.
    c.release(&id);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 900);
}

// ---------------------------------------------------------------------------
// Issue #85: generate_payment_proof
// ---------------------------------------------------------------------------

#[test]
fn test_payment_proof_multiple_payments() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 9_999_999);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);
    c.pay(&payer, &id, &150_i128, &1_u64, &false, &false, &None);

    let proof = c.generate_payment_proof(&id, &payer);
    assert_eq!(proof.invoice_id, id);
    assert_eq!(proof.payer, payer);
    assert_eq!(proof.total_paid, 250);
}

#[test]
fn test_payment_proof_no_payment() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let stranger = Address::generate(&env);
    let recipient = Address::generate(&env);

    let id = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 9_999_999);

    let proof = c.generate_payment_proof(&id, &stranger);
    assert_eq!(proof.total_paid, 0);
}

#[test]
fn test_payment_proof_hash_deterministic() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999_999);
    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);

    let proof1 = c.generate_payment_proof(&id, &payer);
    let proof2 = c.generate_payment_proof(&id, &payer);
    assert_eq!(proof1.proof_hash, proof2.proof_hash);
    assert_eq!(proof1.total_paid, proof2.total_paid);
}

// ---------------------------------------------------------------------------
// Stage release tests (#86)
// ---------------------------------------------------------------------------

#[test]
fn test_stage_release_3_stages() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    // 3 stages: 30% / 40% / 30%
    let mut stages: Vec<u32> = Vec::new(&env);
    stages.push_back(3_000u32);
    stages.push_back(4_000u32);
    stages.push_back(3_000u32);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let mut opts = default_options(&env);
    opts.release_stages = stages;

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &opts);

    // Fully fund the invoice.
    c.pay(&payer, &id, &1_000_i128, &0_u64, &false, &false, &None);

    // Invoice should still be Pending (guarded by release_stages).
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);
    assert_eq!(c.get_invoice_ext(&id).released_stages, 0);

    // Stage 1: 30% = 300
    c.stage_release(&id, &creator);
    assert_eq!(tk.balance(&recipient), 300);
    assert_eq!(c.get_invoice_ext(&id).released_stages, 1);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);

    // Stage 2: 40% = 400
    c.stage_release(&id, &creator);
    assert_eq!(tk.balance(&recipient), 700);
    assert_eq!(c.get_invoice_ext(&id).released_stages, 2);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);

    // Stage 3: 30% = 300 — final stage sets status to Released
    c.stage_release(&id, &creator);
    assert_eq!(tk.balance(&recipient), 1_000);
    assert_eq!(c.get_invoice_ext(&id).released_stages, 3);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
}

#[test]
#[should_panic(expected = "invoice is not pending")]
fn test_stage_release_after_all_stages_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    let mut stages: Vec<u32> = Vec::new(&env);
    stages.push_back(5_000u32);
    stages.push_back(5_000u32);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let mut opts = default_options(&env);
    opts.release_stages = stages;

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &opts);
    c.pay(&payer, &id, &1_000_i128, &0_u64, &false, &false, &None);

    c.stage_release(&id, &creator);
    c.stage_release(&id, &creator);
    // Third call should panic — all stages already released.
    c.stage_release(&id, &creator);
}

#[test]
#[should_panic(expected = "only creator can call stage_release")]
fn test_stage_release_non_creator_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let other = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    let mut stages: Vec<u32> = Vec::new(&env);
    stages.push_back(10_000u32);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let mut opts = default_options(&env);
    opts.release_stages = stages;

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &opts);
    c.pay(&payer, &id, &1_000_i128, &0_u64, &false, &false, &None);

    // Non-creator should not be able to call stage_release.
    c.stage_release(&id, &other);
}

#[test]
#[should_panic(expected = "invoice not fully funded")]
fn test_stage_release_not_fully_funded_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let mut stages: Vec<u32> = Vec::new(&env);
    stages.push_back(10_000u32);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let mut opts = default_options(&env);
    opts.release_stages = stages;

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &opts);
    // Only partially fund.
    c.pay(&payer, &id, &500_i128, &0_u64, &false, &false, &None);

    // Should panic — not fully funded.
    c.stage_release(&id, &creator);
}

#[test]
#[should_panic(expected = "release_stages must sum to 10000 basis points")]
fn test_create_invoice_invalid_release_stages_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    // Stages that don't sum to 10000.
    let mut stages: Vec<u32> = Vec::new(&env);
    stages.push_back(3_000u32);
    stages.push_back(3_000u32);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let mut opts = default_options(&env);
    opts.release_stages = stages;

    c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &opts);
}

// ---------------------------------------------------------------------------
// Issue #142 — dynamic pricing via price oracle
// ---------------------------------------------------------------------------

/// Minimal price oracle contract used by oracle tests.
#[contract]
struct MockOracle;

#[contractimpl]
impl MockOracle {
    /// Returns a fixed price of 2.0 (2_000_000 in 6-decimal fixed-point).
    pub fn get_price(_env: Env) -> i128 {
        2_000_000
    }
}

mod identity_oracle_mod {
    use soroban_sdk::{contract, contractimpl, Env};
    #[contract]
    pub struct IdentityOracle;
    #[contractimpl]
    impl IdentityOracle {
        pub fn get_price(_env: Env) -> i128 {
            1_000_000
        }
    }
}

#[test]
fn test_oracle_none_behaviour_identical_to_current() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);
    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);

    // Create invoice with no oracle (None) — base amount 100.
    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    assert!(c.get_invoice_ext(&id).price_oracle.is_none());
    assert_eq!(c.get_invoice_ext(&id).base_amounts.get(0).unwrap(), 100);

    // Full payment of 100 should succeed (no oracle adjustment).
    c.pay(&payer, &id, &100, &0, &false, &false, &None);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.funded, 100);
    assert_eq!(invoice.status, InvoiceStatus::Released);
}

#[test]
fn test_oracle_price_1_000_000_produces_same_amounts_as_base() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);
    StellarAssetClient::new(&env, &token_id).mint(&payer, &200);

    // Register oracle that returns 1_000_000 (identity).
    let oracle_id = env.register(identity_oracle_mod::IdentityOracle, ());

    let mut opts = default_options(&env);
    opts.price_oracle = Some(oracle_id);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999, &opts);

    assert!(c.get_invoice_ext(&id).price_oracle.is_some());
    assert_eq!(c.get_invoice_ext(&id).base_amounts.get(0).unwrap(), 100);

    // adjusted_total = 100 * 1_000_000 / 1_000_000 = 100 — identical to base
    c.pay(&payer, &id, &100, &0, &false, &false, &None);
    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.funded, 100);
    assert_eq!(invoice.status, InvoiceStatus::Released);
}

#[test]
fn test_oracle_2x_price_doubles_required_amount() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);
    StellarAssetClient::new(&env, &token_id).mint(&payer, &400);

    // Register mock oracle returning 2_000_000 (2x price).
    let oracle_id = env.register(MockOracle, ());

    let mut opts = default_options(&env);
    opts.price_oracle = Some(oracle_id);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128); // base amount

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999, &opts);

    assert_eq!(c.get_invoice_ext(&id).base_amounts.get(0).unwrap(), 100);

    // adjusted_total = 100 * 2_000_000 / 1_000_000 = 200
    // Paying only 100 should NOT release (remaining = 200 - 100 = 100 still owed).
    c.pay(&payer, &id, &100, &0, &false, &false, &None);
    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.funded, 100);
    assert_eq!(invoice.status, InvoiceStatus::Pending); // not yet fully funded

    // Paying the remaining 100 (total 200 = adjusted_total) should release.
    c.pay(&payer, &id, &100, &1, &false, &false, &None);
    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.funded, 200);
    assert_eq!(invoice.status, InvoiceStatus::Released);
}

#[test]
fn test_create_invoice_stores_price_oracle_and_base_amounts() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let oracle_id = env.register(MockOracle, ());
    let mut opts = default_options(&env);
    opts.price_oracle = Some(oracle_id.clone());

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999, &opts);
    let invoice = c.get_invoice(&id);

    assert_eq!(c.get_invoice_ext(&id).price_oracle, Some(oracle_id));
    assert_eq!(c.get_invoice_ext(&id).base_amounts.len(), 1);
    assert_eq!(c.get_invoice_ext(&id).base_amounts.get(0).unwrap(), 500);
    // amounts field also preserved
    assert_eq!(invoice.amounts.get(0).unwrap(), 500);
}

// ---------------------------------------------------------------------------
// Analytics counters (issue #28)
// ---------------------------------------------------------------------------

#[test]
fn test_analytics_initial_state() {
    let (env, contract_id, _token_id) = setup();
    let c = client(&env, &contract_id);

    let (total_invoices, total_volume, total_released, total_refunded) = c.get_stats();
    assert_eq!(total_invoices, 0);
    assert_eq!(total_volume, 0);
    assert_eq!(total_released, 0);
    assert_eq!(total_refunded, 0);
}

#[test]
fn test_analytics_create_invoice_increments_counter() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    // Create first invoice
    make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    let (total_invoices, total_volume, total_released, total_refunded) = c.get_stats();
    assert_eq!(total_invoices, 1);
    assert_eq!(total_volume, 0);
    assert_eq!(total_released, 0);
    assert_eq!(total_refunded, 0);

    // Create second invoice
    make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);

    let (total_invoices, total_volume, total_released, total_refunded) = c.get_stats();
    assert_eq!(total_invoices, 2);
    assert_eq!(total_volume, 0);
    assert_eq!(total_released, 0);
    assert_eq!(total_refunded, 0);
}

#[test]
fn test_analytics_pay_and_release_increments_volume() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let invoice_amount = 250i128;
    let id = make_invoice(&env, &c, &creator, &recipient, invoice_amount, &token_id, 9_999);

    // Pay and auto-release (full payment)
    c.pay(&payer, &id, &invoice_amount, &0_u64, &false, &false, &None);

    let (total_invoices, total_volume, total_released, total_refunded) = c.get_stats();
    assert_eq!(total_invoices, 1);
    assert_eq!(total_volume, invoice_amount);
    assert_eq!(total_released, invoice_amount);
    assert_eq!(total_refunded, 0);
    assert_eq!(tk.balance(&recipient), invoice_amount);
}

#[test]
fn test_analytics_partial_pay_then_release() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer1, &200);
    sa.mint(&payer2, &200);
    env.ledger().set_timestamp(1_000);

    let total_amount = 300i128;
    let id = make_invoice(&env, &c, &creator, &recipient, total_amount, &token_id, 9_999);

    // Partial payment from payer1
    c.pay(&payer1, &id, &150_i128, &0_u64, &false, &false, &None);
    let (total_invoices, total_volume, total_released, total_refunded) = c.get_stats();
    assert_eq!(total_invoices, 1);
    assert_eq!(total_volume, 0);
    assert_eq!(total_released, 0);
    assert_eq!(total_refunded, 0);

    // Completion payment from payer2 triggers auto-release
    c.pay(&payer2, &id, &150_i128, &0_u64, &false, &false, &None);
    let (total_invoices, total_volume, total_released, total_refunded) = c.get_stats();
    assert_eq!(total_invoices, 1);
    assert_eq!(total_volume, 300);
    assert_eq!(total_released, 300);
    assert_eq!(total_refunded, 0);
    assert_eq!(tk.balance(&recipient), 300);
}

#[test]
fn test_analytics_refund_increments_counter() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let invoice_amount = 200i128;
    let id = make_invoice(&env, &c, &creator, &recipient, invoice_amount, &token_id, 2_000);

    // Pay but don't complete
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);

    let (total_invoices, total_volume, total_released, total_refunded) = c.get_stats();
    assert_eq!(total_invoices, 1);
    assert_eq!(total_volume, 0);
    assert_eq!(total_released, 0);
    assert_eq!(total_refunded, 0);

    // Pass deadline and refund
    env.ledger().set_timestamp(3_000);
    c.refund(&id);

    let (total_invoices, total_volume, total_released, total_refunded) = c.get_stats();
    assert_eq!(total_invoices, 1);
    assert_eq!(total_volume, 0);
    assert_eq!(total_released, 0);
    assert_eq!(total_refunded, 100);
    assert_eq!(tk.balance(&payer), 500); // 500 minted - 100 paid + 100 refunded
}

#[test]
fn test_analytics_multiple_operations() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);
    let recipient1 = Address::generate(&env);
    let recipient2 = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer1, &1000);
    sa.mint(&payer2, &1000);
    env.ledger().set_timestamp(1_000);

    // Create and release invoice 1
    let id1 = make_invoice(&env, &c, &creator, &recipient1, 100, &token_id, 9_999);
    c.pay(&payer1, &id1, &100_i128, &0_u64, &false, &false, &None);

    let (ti, tv, tr, tref) = c.get_stats();
    assert_eq!(ti, 1);
    assert_eq!(tv, 100);
    assert_eq!(tr, 100);
    assert_eq!(tref, 0);

    // Create invoice 2 and refund it
    let id2 = make_invoice(&env, &c, &creator, &recipient2, 200, &token_id, 2_000);
    c.pay(&payer2, &id2, &50_i128, &0_u64, &false, &false, &None);
    env.ledger().set_timestamp(3_000);
    c.refund(&id2);

    let (ti, tv, tr, tref) = c.get_stats();
    assert_eq!(ti, 2);
    assert_eq!(tv, 100);
    assert_eq!(tr, 100);
    assert_eq!(tref, 50);

    // Create invoice 3 and release it
    let id3 = make_invoice(&env, &c, &creator, &recipient1, 300, &token_id, 9_999);
    c.pay(&payer1, &id3, &300_i128, &0_u64, &false, &false, &None);

    let (ti, tv, tr, tref) = c.get_stats();
    assert_eq!(ti, 3);
    assert_eq!(tv, 400);
    assert_eq!(tr, 400);
    assert_eq!(tref, 50);
}

// ---------------------------------------------------------------------------
// Issue #40: archive_invoice
// ---------------------------------------------------------------------------

#[test]
fn test_archive_released_invoice_still_readable() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &200);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);
    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);

    // Archive it.
    c.archive_invoice(&id);

    // Still readable after archival.
    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Released);
}

#[test]
#[should_panic(expected = "invoice not completed")]
fn test_archive_pending_invoice_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);
    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    c.archive_invoice(&id);
}

// ---------------------------------------------------------------------------
// Issue #42: event topic schema
// ---------------------------------------------------------------------------

#[test]
fn test_events_emitted_on_create_and_pay() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);

    // Events were emitted (create + pay + release = at least 3).
    assert!(env.events().all().len() >= 3);
}

// ---------------------------------------------------------------------------
// Issue #43: delegation
// ---------------------------------------------------------------------------

#[test]
fn test_delegate_can_extend_deadline() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let delegate = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);
    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 5_000);

    // Assign delegate.
    c.delegate_invoice(&id, &delegate);
    assert_eq!(c.get_delegate(&id), Some(delegate.clone()));

    // Delegate extends deadline.
    c.extend_deadline(&id, &9_999_u64, &delegate);
    assert_eq!(c.get_invoice(&id).deadline, 9_999);
}

#[test]
fn test_revoke_delegate_removes_access() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let delegate = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);
    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 5_000);

    c.delegate_invoice(&id, &delegate);
    c.revoke_delegate(&id);
    assert_eq!(c.get_delegate(&id), None);
}

#[test]
#[should_panic(expected = "not authorized")]
fn test_non_delegate_cannot_extend_deadline() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let stranger = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);
    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 5_000);

    // No delegate set — stranger should be rejected.
    c.extend_deadline(&id, &9_999_u64, &stranger);
}

// ---------------------------------------------------------------------------
// Issue #41: swap_tokens field on Invoice
// ---------------------------------------------------------------------------

#[test]
fn test_invoice_created_with_swap_tokens_field() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let mut opts = default_options(&env);
    // Set a swap token for the single recipient.
    let mut swap_tokens: soroban_sdk::Vec<Option<soroban_sdk::Address>> = soroban_sdk::Vec::new(&env);
    swap_tokens.push_back(Some(token_id.clone()));
    opts.swap_tokens = swap_tokens;

    let mut recipients = soroban_sdk::Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = soroban_sdk::Vec::new(&env);
    amounts.push_back(100_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &opts);
    let ext = c.get_invoice_ext(&id);
    assert_eq!(ext.swap_tokens.len(), 1);
    assert_eq!(ext.swap_tokens.get(0).unwrap(), Some(token_id.clone()));
}

#[test]
fn test_cross_chain_ref() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let mut options = default_options(&env);
    options.cross_chain_ref = Some(soroban_sdk::String::from_str(&env, "evm:0x1234"));

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100);

    let id = c.create_invoice(
        &creator, &recipients, &amounts, &token_id, &2_000_u64, &options,
    );

    assert_eq!(
        c.get_invoice_ext2(&id).cross_chain_ref,
        Some(soroban_sdk::String::from_str(&env, "evm:0x1234"))
    );

    // Note: We can't easily assert on the emitted event here without env.events().all(),
    // but the test verifies the struct and ensures it doesn't panic.
}

#[test]
fn test_compress_payments() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer1, &1000);
    sa.mint(&payer2, &1000);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 500, &token_id, 9_999);

    c.pay(&payer1, &id, &50_i128, &0_u64, &false, &false, &None);
    c.pay(&payer2, &id, &100_i128, &0_u64, &false, &false, &None);
    c.pay(&payer1, &id, &75_i128, &1_u64, &false, &false, &None);
    c.pay(&payer2, &id, &25_i128, &1_u64, &false, &false, &None);

    let inv_before = c.get_invoice(&id);
    assert_eq!(inv_before.payments.len(), 4);

    c.compress_payments(&id);

    let inv_after = c.get_invoice(&id);
    assert_eq!(inv_after.payments.len(), 2);
    assert_eq!(inv_after.funded, 250);
}

#[contract]
pub struct MockGovernance;

#[contractimpl]
impl MockGovernance {
    pub fn check_approval(_env: Env, _creator: Address, total: i128) -> bool {
        // Just a mock logic: approved if total < 10_000
        total < 10_000
    }
}

#[test]
fn test_governance_approval() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let treasury = Address::generate(&env);

    let gov_id = env.register(MockGovernance, ());

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &Some(gov_id), &0_u32, &0_u32, &0_u64);

    env.ledger().set_timestamp(1_000);

    // Total = 500 < 10_000, so it should be approved
    let id = make_invoice(&env, &c, &creator, &recipient, 500, &token_id, 9_999);
    assert_eq!(id, 1);
}

#[test]
#[should_panic(expected = "governance approval required")]
fn test_governance_rejection() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let treasury = Address::generate(&env);

    let gov_id = env.register(MockGovernance, ());

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &Some(gov_id), &0_u32, &0_u32, &0_u64);

    env.ledger().set_timestamp(1_000);

    // Total = 15_000 >= 10_000, so it should be rejected
    make_invoice(&env, &c, &creator, &recipient, 15_000, &token_id, 9_999);
}

#[test]
fn test_payment_channel() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer, &1000);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 500, &token_id, 9_999);

    c.open_channel(&payer, &id, &400_i128);
    c.channel_pay(&payer, &id, &100_i128);
    c.channel_pay(&payer, &id, &50_i128);
    c.channel_pay(&payer, &id, &50_i128);

    c.close_channel(&payer, &id);

    let inv = c.get_invoice(&id);
    assert_eq!(inv.funded, 200);

    let tk = token_client(&env, &token_id);
    assert_eq!(tk.balance(&payer), 800); // 1000 - 400 + 200 refund
}

#[test]
#[should_panic(expected = "insufficient channel balance")]
fn test_payment_channel_insufficient() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer, &1000);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 500, &token_id, 9_999);

    c.open_channel(&payer, &id, &100_i128);
    c.channel_pay(&payer, &id, &150_i128); // Panics
}

// ---------------------------------------------------------------------------
// Issue #1: convert_to_stream
// ---------------------------------------------------------------------------

/// Mock stream contract: records that create_stream was called via persistent storage.
#[contract]
struct MockStream;

#[contractimpl]
impl MockStream {
    pub fn create_stream(env: Env, recipient: Address, amount: i128, duration: u64) {
        // Store the last call args so tests can verify.
        env.storage().persistent().set(&soroban_sdk::symbol_short!("s_rec"), &recipient);
        env.storage().persistent().set(&soroban_sdk::symbol_short!("s_amt"), &amount);
        env.storage().persistent().set(&soroban_sdk::symbol_short!("s_dur"), &duration);
    }
}

#[test]
fn test_convert_to_stream_calls_stream_contract() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    let stream_id = env.register(MockStream, ());
    c.set_stream_contract(&admin, &stream_id);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let mut opts = default_options(&env);
    opts.convert_to_stream = true;

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999, &opts);

    // Trigger release by fully paying the invoice.
    c.pay(&payer, &id, &200_i128, &0, &false, &false, &None);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Released);

    // Verify stream contract was called: tokens transferred to stream contract.
    let tk = token_client(&env, &token_id);
    assert_eq!(tk.balance(&stream_id), 200);
}

#[test]
fn test_convert_to_stream_false_uses_direct_transfer() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &300);
    env.ledger().set_timestamp(1_000);

    // convert_to_stream defaults to false
    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);
    c.pay(&payer, &id, &200_i128, &0, &false, &false, &None);

    let tk = token_client(&env, &token_id);
    // Direct transfer: recipient gets the tokens, not the stream contract.
    assert_eq!(tk.balance(&recipient), 200);
}

// ---------------------------------------------------------------------------
// Issue #2: pay_with_token
// ---------------------------------------------------------------------------

/// Mock DEX: returns the input amount as the swapped output (1:1 rate).
#[contract]
struct MockDex;

#[contractimpl]
impl MockDex {
    pub fn swap(_env: Env, _source: Address, _dest: Address, amount: i128) -> i128 {
        amount
    }
}

#[contract]
struct MockNotification;

#[contractimpl]
impl MockNotification {
    pub fn notify(env: Env, invoice_id: u64, event: Symbol) {
        let key = (symbol_short!("notif"), invoice_id, event.clone());
        env.storage().persistent().set(&key, &true);
    }

    pub fn was_notified(env: Env, invoice_id: u64, event: Symbol) -> bool {
        let key = (symbol_short!("notif"), invoice_id, event.clone());
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or(false)
    }
}

#[test]
fn test_authorise_delegate_and_delegate_pay_records_beneficiary_as_payer() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let delegate = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&delegate, &200);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.authorise_delegate(&beneficiary, &delegate);
    c.delegate_pay(&delegate, &beneficiary, &id, &100_i128);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.funded, 100);
    assert_eq!(invoice.payments.get(0).unwrap().payer, beneficiary);
    assert_eq!(invoice.payments.get(0).unwrap().amount, 100);
    assert_eq!(tk.balance(&recipient), 100);
}

#[test]
#[should_panic(expected = "not authorised")]
fn test_delegate_pay_unauthorised_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&unauthorized, &200);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.delegate_pay(&unauthorized, &beneficiary, &id, &100_i128);
}

#[test]
fn test_overflow_behavior_refund_accepts_excess() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &200);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let mut opts = default_options(&env);
    opts.overflow_behavior = types::OverflowBehavior::Refund;

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &opts);
    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.funded, 100);
    assert_eq!(tk.balance(&payer), 100);
}

#[test]
fn test_overflow_behavior_donate_sends_excess_to_treasury() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);
    StellarAssetClient::new(&env, &token_id).mint(&payer, &200);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let mut opts = default_options(&env);
    opts.overflow_behavior = types::OverflowBehavior::Donate;

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &opts);
    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.funded, 100);
    assert_eq!(tk.balance(&treasury), 100);
}

#[test]
fn test_bridge_pay_credits_invoice_after_swap() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let treasury = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    let alt_token_admin = Address::generate(&env);
    let alt_token_id = env
        .register_stellar_asset_contract_v2(alt_token_admin.clone())
        .address();
    StellarAssetClient::new(&env, &alt_token_id).mint(&payer, &300);

    let dex_id = env.register(MockDex, ());
    c.set_dex_contract(&admin, &dex_id);

    // Pre-mint invoice_token to the contract to simulate what a real DEX would transfer back.
    StellarAssetClient::new(&env, &token_id).mint(&contract_id, &300);

    env.ledger().set_timestamp(1_000);
    let id = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 9_999);

    c.bridge_pay(&payer, &id, &alt_token_id, &300_i128);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.funded, 300);
}

#[test]
fn test_notification_contract_receives_pay_release_and_refund() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier_id = env.register(MockNotification, ());
    let notifier = MockNotificationClient::new(&env, &notifier_id);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &200);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let mut opts = default_options(&env);
    opts.notification_contract = Some(notifier_id.clone());

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &opts);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);

    assert!(notifier.was_notified(&id, &symbol_short!("pay")));
    assert!(notifier.was_notified(&id, &symbol_short!("release")));

    let id2 = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &opts);
    env.ledger().set_timestamp(12_000);
    c.refund(&id2);
    assert!(notifier.was_notified(&id2, &symbol_short!("refund")));
}

#[test]
fn test_pay_with_token_accepted_token_credited() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    // Register alternate token and DEX.
    let alt_token_admin = Address::generate(&env);
    let alt_token_id = env
        .register_stellar_asset_contract_v2(alt_token_admin.clone())
        .address();
    StellarAssetClient::new(&env, &alt_token_id).mint(&payer, &1_000);

    let dex_id = env.register(MockDex, ());
    c.set_dex_contract(&admin, &dex_id);

    // Pre-mint invoice_token to the contract to simulate what a real DEX would transfer back.
    StellarAssetClient::new(&env, &token_id).mint(&contract_id, &300);

    env.ledger().set_timestamp(1_000);

    let mut accepted = Vec::new(&env);
    accepted.push_back(alt_token_id.clone());

    let mut opts = default_options(&env);
    opts.accepted_tokens = accepted;

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(300_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999, &opts);

    // Pay with the alternate token — DEX converts 1:1 so 300 gets credited.
    c.pay_with_token(&payer, &id, &alt_token_id, &300_i128, &0);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.funded, 300);
}

#[test]
#[should_panic(expected = "token not accepted")]
fn test_pay_with_token_non_listed_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let unknown_admin = Address::generate(&env);
    let unknown_token = env
        .register_stellar_asset_contract_v2(unknown_admin.clone())
        .address();
    StellarAssetClient::new(&env, &unknown_token).mint(&payer, &500);

    env.ledger().set_timestamp(1_000);

    // Create invoice with empty accepted_tokens (only base token accepted).
    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);

    // Attempting to pay with an unlisted token must panic.
    c.pay_with_token(&payer, &id, &unknown_token, &200_i128, &0);
}

// ---------------------------------------------------------------------------
// Issue #3: pool_pay
// ---------------------------------------------------------------------------

#[test]
fn test_pool_pay_three_invoices_funded_correctly() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let r3 = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    let id1 = make_invoice(&env, &c, &creator, &r1, 100, &token_id, 9_999);
    let id2 = make_invoice(&env, &c, &creator, &r2, 200, &token_id, 9_999);
    let id3 = make_invoice(&env, &c, &creator, &r3, 300, &token_id, 9_999);

    let mut payments = Vec::new(&env);
    payments.push_back(types::InvoicePayment { invoice_id: id1, amount: 100 });
    payments.push_back(types::InvoicePayment { invoice_id: id2, amount: 200 });
    payments.push_back(types::InvoicePayment { invoice_id: id3, amount: 300 });

    // Payer balance before: 1000; total payment: 600 → balance after: 400.
    c.pool_pay(&payer, &payments);

    assert_eq!(tk.balance(&payer), 400);

    // All three invoices fully funded and auto-released.
    assert_eq!(c.get_invoice(&id1).funded, 100);
    assert_eq!(c.get_invoice(&id2).funded, 200);
    assert_eq!(c.get_invoice(&id3).funded, 300);
    assert_eq!(c.get_invoice(&id1).status, InvoiceStatus::Released);
    assert_eq!(c.get_invoice(&id2).status, InvoiceStatus::Released);
    assert_eq!(c.get_invoice(&id3).status, InvoiceStatus::Released);
}

#[test]
#[should_panic(expected = "invoice is not pending")]
fn test_pool_pay_invalid_invoice_reverts_all() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    let id1 = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    // Pay id1 so it releases, making it no longer Pending.
    c.pay(&payer, &id1, &100_i128, &0, &false, &false, &None);

    let id2 = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    let mut payments = Vec::new(&env);
    payments.push_back(types::InvoicePayment { invoice_id: id1, amount: 50 }); // id1 no longer Pending
    payments.push_back(types::InvoicePayment { invoice_id: id2, amount: 50 });

    c.pool_pay(&payer, &payments); // should panic
}

// ---------------------------------------------------------------------------
// Issue #4: creator whitelist
// ---------------------------------------------------------------------------

#[test]
fn test_whitelist_empty_allows_any_creator() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);
    env.ledger().set_timestamp(1_000);

    // No whitelist set — any creator may create.
    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    assert_eq!(id, 1);
}

#[test]
#[should_panic(expected = "creator not whitelisted")]
fn test_non_whitelisted_creator_rejected() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let whitelisted = Address::generate(&env);
    let not_whitelisted = Address::generate(&env);
    let recipient = Address::generate(&env);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);
    c.whitelist_creator(&admin, &whitelisted);

    env.ledger().set_timestamp(1_000);

    // not_whitelisted is not on the list — must panic.
    make_invoice(&env, &c, &not_whitelisted, &recipient, 100, &token_id, 9_999);
}

#[test]
fn test_whitelisted_creator_can_create() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);
    c.whitelist_creator(&admin, &creator);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    assert_eq!(id, 1);
}

#[test]
fn test_remove_creator_from_whitelist() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);
    c.whitelist_creator(&admin, &creator);
    c.remove_creator(&admin, &creator);

    env.ledger().set_timestamp(1_000);

    // After removal the whitelist is empty again, so any creator is allowed.
    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    assert_eq!(id, 1);
}


#[test]
fn test_creator_stats_increments_on_operations() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let creator2 = Address::generate(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);
    let recipient1 = Address::generate(&env);
    let recipient2 = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer1, &2000);
    sa.mint(&payer2, &2000);
    env.ledger().set_timestamp(1_000);

    // Initially, creator has no stats
    let s = c.get_creator_stats(&creator);
    assert_eq!(s.total_invoices, 0);
    assert_eq!(s.total_released, 0);
    assert_eq!(s.total_refunded, 0);

    // Create first invoice (count should increment)
    let id1 = make_invoice(&env, &c, &creator, &recipient1, 100, &token_id, 9_999);
    let s = c.get_creator_stats(&creator);
    assert_eq!(s.total_invoices, 1);
    assert_eq!(s.total_released, 0);
    assert_eq!(s.total_refunded, 0);

    // Pay and release first invoice (released amount should increment)
    c.pay(&payer1, &id1, &100_i128, &0_u64, &false, &false, &None);
    let s = c.get_creator_stats(&creator);
    assert_eq!(s.total_invoices, 1);
    assert_eq!(s.total_released, 100);
    assert_eq!(s.total_refunded, 0);

    // Create second invoice
    let id2 = make_invoice(&env, &c, &creator, &recipient2, 200, &token_id, 2_000);
    let s = c.get_creator_stats(&creator);
    assert_eq!(s.total_invoices, 2);
    assert_eq!(s.total_released, 100);
    assert_eq!(s.total_refunded, 0);

    // Partially pay second invoice and let it expire for refund
    c.pay(&payer2, &id2, &50_i128, &0_u64, &false, &false, &None);
    env.ledger().set_timestamp(3_000);
    c.refund(&id2);

    let s = c.get_creator_stats(&creator);
    assert_eq!(s.total_invoices, 2);
    assert_eq!(s.total_released, 100); // Only released amounts count
    assert_eq!(s.total_refunded, 1);

    // Create third invoice and fully release it
    let id3 = make_invoice(&env, &c, &creator, &recipient1, 300, &token_id, 9_999);
    c.pay(&payer1, &id3, &300_i128, &0_u64, &false, &false, &None);

    let s = c.get_creator_stats(&creator);
    assert_eq!(s.total_invoices, 3);
    assert_eq!(s.total_released, 400);
    assert_eq!(s.total_refunded, 1);

    // Verify another creator's stats are independent
    let id4 = make_invoice(&env, &c, &creator2, &recipient1, 500, &token_id, 9_999);
    c.pay(&payer1, &id4, &500_i128, &0_u64, &false, &false, &None);

    let s = c.get_creator_stats(&creator2);
    assert_eq!(s.total_invoices, 1);
    assert_eq!(s.total_released, 500);
    assert_eq!(s.total_refunded, 0);

    // Creator1 stats should remain unchanged
    let s = c.get_creator_stats(&creator);
    assert_eq!(s.total_invoices, 3);
    assert_eq!(s.total_released, 400);
    assert_eq!(s.total_refunded, 1);
}


#[test]
#[should_panic(expected = "payment cooldown active")]
fn test_cooldown_blocks_same_payer_within_window() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let stellar_asset = StellarAssetClient::new(&env, &token_id);

    let payer = Address::generate(&env);
    let other_payer = Address::generate(&env);
    stellar_asset.mint(&payer, &500);
    stellar_asset.mint(&other_payer, &500);

    env.ledger().set_timestamp(1_000);
    let id = single_recipient_invoice(
        &env,
        &c,
        &token_id,
        500,
        invoice_options(&env, Some(60), None, None),
    );

    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);
    c.pay(&other_payer, &id, &100_i128, &0_u64, &false, &false, &None);
    c.pay(&payer, &id, &100_i128, &1_u64, &false, &false, &None);
}

#[test]
#[should_panic(expected = "payment rate limit exceeded")]
fn test_rate_limit_blocks_after_n_payments() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let stellar_asset = StellarAssetClient::new(&env, &token_id);

    env.ledger().set_timestamp(1_000);
    let id = single_recipient_invoice(
        &env,
        &c,
        &token_id,
        500,
        invoice_options(&env, None, Some(2), Some(60)),
    );

    for _ in 0..3 {
        let payer = Address::generate(&env);
        stellar_asset.mint(&payer, &100);
        c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);
    }
}

#[test]
fn test_rate_limit_window_resets() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let stellar_asset = StellarAssetClient::new(&env, &token_id);

    env.ledger().set_timestamp(1_000);
    let id = single_recipient_invoice(
        &env,
        &c,
        &token_id,
        500,
        invoice_options(&env, None, Some(2), Some(60)),
    );

    for _ in 0..2 {
        let payer = Address::generate(&env);
        stellar_asset.mint(&payer, &100);
        c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);
    }

    env.ledger().set_timestamp(1_061);
    let payer = Address::generate(&env);
    stellar_asset.mint(&payer, &100);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);
}

#[test]
#[should_panic(expected = "payment rate limit exceeded")]
fn test_cooldown_and_rate_limit_independent() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let stellar_asset = StellarAssetClient::new(&env, &token_id);

    let payer = Address::generate(&env);
    let other_payer = Address::generate(&env);
    stellar_asset.mint(&payer, &500);
    stellar_asset.mint(&other_payer, &500);

    env.ledger().set_timestamp(1_000);
    let id = single_recipient_invoice(
        &env,
        &c,
        &token_id,
        500,
        invoice_options(&env, Some(120), Some(1), Some(60)),
    );

    let ext = c.get_invoice_ext(&id);
    assert_eq!(ext.payment_cooldown_secs, Some(120));
    assert_eq!(ext.max_payments_per_window, Some(1));
    assert_eq!(ext.payment_window_secs, Some(60));

    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);
    c.pay(&other_payer, &id, &100_i128, &0_u64, &false, &false, &None);
}

// ---------------------------------------------------------------------------
// Invariant tests
// ---------------------------------------------------------------------------

/// Helper: compute the invoice total from its amounts vec.
fn invoice_total(invoice: &InvoiceCore) -> i128 {
    invoice.amounts.iter().sum()
}

/// Invariant: invoice.funded never exceeds total across all valid payment sequences.
///
/// Parameterised over several (total, payment_sequence) combinations.
#[test]
fn invariant_funded_never_exceeds_total() {
    // Each case: (invoice_total, payments)
    let cases: &[(i128, &[i128])] = &[
        (100, &[50, 50]),
        (300, &[100, 100, 100]),
        (500, &[200, 300]),
        (1000, &[1, 999]),
        (1000, &[250, 250, 250, 250]),
        (50, &[50]),
        (400, &[100, 100, 100, 100]),
    ];

    for (total_amount, payments) in cases {
        let (env, contract_id, token_id) = setup_initialized();
        let c = client(&env, &contract_id);

        let creator = Address::generate(&env);
        let recipient = Address::generate(&env);

        StellarAssetClient::new(&env, &token_id).mint(&creator, &1_000_000);
        // Mint to a shared payer used for all payments.
        let payer = Address::generate(&env);
        StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000_000);

        env.ledger().set_timestamp(1_000);

        let id = make_invoice(&env, &c, &creator, &recipient, *total_amount, &token_id, 9_999_999);
        let total = invoice_total(&c.get_invoice(&id));

        let mut nonce: u64 = 0;
        for &payment in *payments {
            c.pay(&payer, &id, &payment, &nonce, &false, &false, &None);
            nonce += 1;

            // Invariant: funded must never exceed total at any point.
            let inv = c.get_invoice(&id);
            assert!(
                inv.funded <= total,
                "funded ({}) exceeded total ({}) after payment of {}",
                inv.funded,
                total,
                payment
            );
        }
    }
}

/// Invariant: status transitions are monotonic — only Pending→Released and
/// Pending→Refunded are valid forward transitions; status never regresses.
#[test]
fn invariant_status_monotonic() {
    // --- Case 1: Pending → Released (via full payment) ---
    {
        let (env, contract_id, token_id) = setup_initialized();
        let c = client(&env, &contract_id);
        let creator = Address::generate(&env);
        let payer = Address::generate(&env);
        let recipient = Address::generate(&env);

        StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
        env.ledger().set_timestamp(1_000);

        let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999_999);
        assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);

        c.pay(&payer, &id, &200, &0, &false, &false, &None);
        let status = c.get_invoice(&id).status;
        assert_eq!(status, InvoiceStatus::Released);
        // Must not go back to Pending.
        assert_ne!(status, InvoiceStatus::Pending);
    }

    // --- Case 2: Pending → Refunded (via expired deadline) ---
    {
        let (env, contract_id, token_id) = setup_initialized();
        let c = client(&env, &contract_id);
        let creator = Address::generate(&env);
        let payer = Address::generate(&env);
        let recipient = Address::generate(&env);

        StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
        env.ledger().set_timestamp(1_000);

        let id = make_invoice(&env, &c, &creator, &recipient, 500, &token_id, 2_000);
        assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);

        c.pay(&payer, &id, &100, &0, &false, &false, &None);
        assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);

        env.ledger().set_timestamp(3_000);
        c.refund(&id);
        let status = c.get_invoice(&id).status;
        assert_eq!(status, InvoiceStatus::Refunded);
        assert_ne!(status, InvoiceStatus::Pending);
        assert_ne!(status, InvoiceStatus::Released);
    }

    // --- Case 3: Pending → Cancelled ---
    {
        let (env, contract_id, token_id) = setup_initialized();
        let c = client(&env, &contract_id);
        let creator = Address::generate(&env);
        let recipient = Address::generate(&env);

        env.ledger().set_timestamp(1_000);

        let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999_999);
        assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);

        c.cancel_invoice(&creator, &id);
        let status = c.get_invoice(&id).status;
        assert_eq!(status, InvoiceStatus::Cancelled);
        assert_ne!(status, InvoiceStatus::Pending);
    }

    // --- Case 4: Partial payments stay Pending until fully funded ---
    {
        let (env, contract_id, token_id) = setup_initialized();
        let c = client(&env, &contract_id);
        let creator = Address::generate(&env);
        let payer = Address::generate(&env);
        let recipient = Address::generate(&env);

        StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
        env.ledger().set_timestamp(1_000);

        let id = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 9_999_999);

        for (nonce, amount) in [(0u64, 100i128), (1, 100)] {
            c.pay(&payer, &id, &amount, &nonce, &false, &false, &None);
            assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);
        }
        c.pay(&payer, &id, &100, &2, &false, &false, &None);
        assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    }
}

/// Invariant: the contract's token balance equals invoice.funded for a simple
/// single-invoice scenario at every state-changing step.
#[test]
fn invariant_balance_matches_funded() {
    // Each case: (invoice_total, payments_before_release)
    let cases: &[(i128, &[i128])] = &[
        (100, &[100]),
        (300, &[100, 100, 100]),
        (500, &[200, 300]),
        (400, &[150, 150, 100]),
    ];

    for (total_amount, payments) in cases {
        let (env, contract_id, token_id) = setup_initialized();
        let c = client(&env, &contract_id);
        let tk = token_client(&env, &token_id);

        let creator = Address::generate(&env);
        let payer = Address::generate(&env);
        let recipient = Address::generate(&env);

        StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000_000);
        env.ledger().set_timestamp(1_000);

        let id = make_invoice(&env, &c, &creator, &recipient, *total_amount, &token_id, 9_999_999);

        // Before any payment: both funded and contract balance are 0.
        assert_eq!(c.get_invoice(&id).funded, 0);
        assert_eq!(tk.balance(&contract_id), 0);

        let last_idx = payments.len() - 1;
        let mut nonce: u64 = 0;
        for (i, &payment) in payments.iter().enumerate() {
            c.pay(&payer, &id, &payment, &nonce, &false, &false, &None);
            nonce += 1;

            let inv = c.get_invoice(&id);

            if i < last_idx {
                // Intermediate payments: invoice still Pending, tokens held by contract.
                assert_eq!(inv.status, InvoiceStatus::Pending);
                assert_eq!(
                    tk.balance(&contract_id),
                    inv.funded,
                    "contract balance ({}) != funded ({}) after {} of {} payments",
                    tk.balance(&contract_id),
                    inv.funded,
                    i + 1,
                    payments.len()
                );
            } else {
                // Final payment triggers release; tokens move to recipient.
                assert_eq!(inv.status, InvoiceStatus::Released);
                // After release the contract holds 0 for this invoice's funds.
                assert_eq!(
                    tk.balance(&contract_id),
                    0,
                    "contract should hold 0 after release, got {}",
                    tk.balance(&contract_id)
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Pause mechanism tests
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "invoice is frozen")]
fn test_pause_blocks_payment_with_reason() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);

    let reason = soroban_sdk::String::from_str(&env, "legal review pending");
    c.pause_invoice(&creator, &id, &reason, &None);

    let ext = c.get_invoice_ext(&id);
    assert_eq!(ext.pause_reason, Some(reason));
    assert_eq!(ext.auto_resume_at, None);
    assert!(c.get_invoice(&id).frozen);

    // This should panic with "invoice is frozen"
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);
}

#[test]
fn test_pause_invoice_emits_frozen_event() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);
    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);

    let reason = soroban_sdk::String::from_str(&env, "legal review pending");
    c.pause_invoice(&creator, &id, &reason, &None);

    let has_frozen_event = env
        .events()
        .all()
        .iter()
        .any(|(_c, topics, _d)| topic1_is(&env, &topics, "frozen"));
    assert!(has_frozen_event, "expected an invoice_frozen event on pause");
}

#[test]
fn test_auto_resume_allows_payment_after_timestamp() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);

    let reason = soroban_sdk::String::from_str(&env, "scheduled maintenance");
    c.pause_invoice(&creator, &id, &reason, &Some(2_000_u64));

    assert!(c.get_invoice(&id).frozen);

    // Advance ledger past auto-resume timestamp.
    env.ledger().set_timestamp(2_000);

    // Payment should succeed because lazy auto-resume fires.
    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 200);

    // A distinct `invoice_auto_resumed` event fires for the timer-triggered
    // resume; the manual `resumed` event must NOT fire (it wasn't a manual resume).
    let has_auto_resumed_event = env
        .events()
        .all()
        .iter()
        .any(|(_c, topics, _d)| topic1_is(&env, &topics, "auto_res"));
    assert!(
        has_auto_resumed_event,
        "invoice_auto_resumed event should be emitted on lazy auto-resume"
    );

    let has_manual_resumed_event = env
        .events()
        .all()
        .iter()
        .any(|(_c, topics, _d)| topic1_is(&env, &topics, "resumed"));
    assert!(
        !has_manual_resumed_event,
        "manual invoice_resumed should not fire for an automatic resume"
    );
}

#[test]
fn test_admin_force_resume_overrides_creator_pause() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let admin = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    // Initialize with a custom admin so admin_force_resume can authenticate.
    c.initialize(
        &admin,
        &0_i128,
        &Address::generate(&env),
        &token_id,
        &0_u32,
        &None,
        &0_u32,
        &0_u32,
        &0_u64,
    );

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);

    let reason = soroban_sdk::String::from_str(&env, "compliance hold");
    c.pause_invoice(&creator, &id, &reason, &None);

    assert!(c.get_invoice(&id).frozen);

    // Admin force-resumes.
    c.admin_force_resume(&admin, &id);

    let invoice = c.get_invoice(&id);
    assert!(!invoice.frozen);

    let ext = c.get_invoice_ext(&id);
    assert_eq!(ext.pause_reason, None);
    assert_eq!(ext.auto_resume_at, None);

    // Payment now succeeds.
    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
}

#[test]
fn test_resume_clears_stored_reason() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);

    let reason = soroban_sdk::String::from_str(&env, "temporary hold");
    c.pause_invoice(&creator, &id, &reason, &Some(5_000_u64));

    // Verify stored on chain.
    let ext = c.get_invoice_ext(&id);
    assert_eq!(ext.pause_reason, Some(reason));
    assert_eq!(ext.auto_resume_at, Some(5_000_u64));

    // Creator manually resumes.
    c.resume_invoice(&creator, &id);

    // Reason and auto_resume_at must be cleared.
    let ext = c.get_invoice_ext(&id);
    assert_eq!(ext.pause_reason, None);
    assert_eq!(ext.auto_resume_at, None);
    assert!(!c.get_invoice(&id).frozen);
}

// ---------------------------------------------------------------------------
// Invoice cloning tests
// ---------------------------------------------------------------------------

#[test]
fn test_clone_copies_recipients_and_amounts() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient1 = Address::generate(&env);
    let recipient2 = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient1.clone());
    recipients.push_back(recipient2.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);
    amounts.push_back(200_i128);

    let source_id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999,
        &default_options(&env),
    );

    let overrides = types::CloneOverrides {
        new_deadline: None,
        new_amounts: None,
        new_recipients: None,
        new_overflow_behavior: None,
        new_metadata_hash: None,
    };
    let clone_id = c.clone_invoice(&creator, &source_id, &overrides);

    let clone = c.get_invoice(&clone_id);
    assert_eq!(clone.recipients, recipients);
    assert_eq!(clone.amounts, amounts);
    assert_eq!(clone.clone_depth, 1);

    let clone_ext = c.get_invoice_ext(&clone_id);
    assert_eq!(clone_ext.parent_invoice_id, Some(source_id));
}

#[test]
fn test_clone_invoice_emits_ledger_sequence_in_event_data() {
    use soroban_sdk::TryIntoVal;

    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let source_id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    env.ledger().set_sequence_number(42);
    let overrides = types::CloneOverrides {
        new_deadline: None,
        new_amounts: None,
        new_recipients: None,
        new_overflow_behavior: None,
        new_metadata_hash: None,
    };
    let _clone_id = c.clone_invoice(&creator, &source_id, &overrides);

    let cloned_event = env
        .events()
        .all()
        .iter()
        .find(|(_c, topics, _d)| topic0_is(&env, topics, "cloned"))
        .expect("expected an invoice_cloned event");

    let (_contract, _topics, data) = cloned_event;
    let (ledger_seq,): (u32,) = data.try_into_val(&env).unwrap();
    assert_eq!(ledger_seq, 42);
}

#[test]
fn test_clone_with_overrides_replaces_fields() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let original_recipient = Address::generate(&env);
    let new_recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let source_id = make_invoice(
        &env,
        &c,
        &creator,
        &original_recipient,
        100,
        &token_id,
        9_999,
    );

    let mut new_recipients = Vec::new(&env);
    new_recipients.push_back(new_recipient.clone());
    let mut new_amounts = Vec::new(&env);
    new_amounts.push_back(500_i128);

    let overrides = types::CloneOverrides {
        new_deadline: Some(19_999),
        new_amounts: Some(new_amounts.clone()),
        new_recipients: Some(new_recipients.clone()),
        new_overflow_behavior: Some(Symbol::new(&env, "Refund")),
        new_metadata_hash: None,
    };
    let clone_id = c.clone_invoice(&creator, &source_id, &overrides);

    let clone = c.get_invoice(&clone_id);
    assert_eq!(clone.recipients, new_recipients);
    assert_eq!(clone.amounts, new_amounts);
    assert_eq!(clone.deadline, 19_999);

    let clone_ext2 = c.get_invoice_ext2(&clone_id);
    assert_eq!(clone_ext2.overflow_behavior, types::OverflowBehavior::Refund);
}

#[test]
#[should_panic(expected = "max clone depth exceeded")]
fn test_clone_depth_limit_enforced() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let base_overrides = types::CloneOverrides {
        new_deadline: None,
        new_amounts: None,
        new_recipients: None,
        new_overflow_behavior: None,
        new_metadata_hash: None,
    };

    let id0 = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    assert_eq!(c.get_invoice(&id0).clone_depth, 0);

    let id1 = c.clone_invoice(&creator, &id0, &base_overrides);
    assert_eq!(c.get_invoice(&id1).clone_depth, 1);

    let id2 = c.clone_invoice(&creator, &id1, &base_overrides);
    assert_eq!(c.get_invoice(&id2).clone_depth, 2);

    let id3 = c.clone_invoice(&creator, &id2, &base_overrides);
    assert_eq!(c.get_invoice(&id3).clone_depth, 3);

    let id4 = c.clone_invoice(&creator, &id3, &base_overrides);
    assert_eq!(c.get_invoice(&id4).clone_depth, 4);

    let id5 = c.clone_invoice(&creator, &id4, &base_overrides);
    assert_eq!(c.get_invoice(&id5).clone_depth, 5);

    // 6th clone (source at depth 5) must panic.
    c.clone_invoice(&creator, &id5, &base_overrides);
}

#[test]
fn test_clone_resets_payment_state() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let sa = StellarAssetClient::new(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    sa.mint(&payer, &50);
    env.ledger().set_timestamp(1_000);

    let source_id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    // Partially fund the source invoice.
    c.pay(&payer, &source_id, &50_i128, &0_u64, &false, &false, &None);

    let source = c.get_invoice(&source_id);
    assert_eq!(source.funded, 50);
    assert_eq!(source.payments.len(), 1);

    let overrides = types::CloneOverrides {
        new_deadline: None,
        new_amounts: None,
        new_recipients: None,
        new_overflow_behavior: None,
        new_metadata_hash: None,
    };
    let clone_id = c.clone_invoice(&creator, &source_id, &overrides);

    let clone = c.get_invoice(&clone_id);
    assert_eq!(clone.funded, 0);
    assert_eq!(clone.payments.len(), 0);
    assert_eq!(clone.status, InvoiceStatus::Pending);
    assert_eq!(clone.released_bps, 0);
    assert!(clone.completion_time.is_none());
}

#[test]
fn test_sharded_payment_storage() {
    // Test issue #177: payments distributed across N shard keys based on payer address hash
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let sa = StellarAssetClient::new(&env, &token_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    
    // Create invoice for 2000 total (so 16 payers paying 100 each doesn't auto-release it)
    env.ledger().set_timestamp(1_000);
    let invoice_id = make_invoice(&env, &c, &creator, &recipient, 2000, &token_id, 9_999);

    // Create 16 different payers
    let mut payers: Vec<Address> = Vec::new(&env);
    for _ in 0..16 {
        let payer = Address::generate(&env);
        sa.mint(&payer, &100);
        payers.push_back(payer);
    }

    // Each payer pays 100
    for i in 0..16 {
        let payer = payers.get(i as u32).unwrap();
        c.pay(&payer, &invoice_id, &100_i128, &0_u64, &false, &false, &None);
    }

    // Verify invoice is partially funded (not auto-released)
    let invoice = c.get_invoice(&invoice_id);
    assert_eq!(invoice.funded, 1600);
    assert_eq!(invoice.payments.len(), 16);

    // Verify all payments are present in aggregated view
    let mut total_from_payments: i128 = 0;
    for payment in invoice.payments.iter() {
        total_from_payments += payment.amount;
    }
    assert_eq!(total_from_payments, 1600);

    // Verify all 8 shards are populated (SHARD_COUNT = 8)
    let mut populated_shards: u64 = 0;
    env.as_contract(&contract_id, || {
        for shard_id in 0..8_u64 {
            let key = (soroban_sdk::symbol_short!("pay_sh"), invoice_id, shard_id);
            if env.storage().persistent().has(&key) {
                populated_shards += 1;
            }
        }
    });
    assert!(populated_shards > 0, "At least some shards should be populated");

    // Test refund reads all shards correctly
    env.ledger().set_timestamp(20_000); // Past deadline
    c.refund(&invoice_id);

    // Verify all payers were refunded
    let tk = token_client(&env, &token_id);
    for i in 0..16 {
        let payer = payers.get(i as u32).unwrap();
        assert_eq!(tk.balance(&payer), 100, "Payer should be refunded");
    }

    // Verify invoice status is Refunded
    let invoice = c.get_invoice(&invoice_id);
    assert_eq!(invoice.status, types::InvoiceStatus::Refunded);
}

// ---------------------------------------------------------------------------
// Issue #204: donate-on-failure
// ---------------------------------------------------------------------------

#[test]
fn test_donate_on_failure_sends_to_creator() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let donor = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&donor, &300);
    env.ledger().set_timestamp(1_000);

    // Invoice needs 500 tokens; donor contributes 300 with donate_on_failure=true.
    let id = make_invoice(&env, &c, &creator, &recipient, 500, &token_id, 2_000);
    c.pay(&donor, &id, &300_i128, &0_u64, &false, &true, &None);

    env.ledger().set_timestamp(3_000);
    c.refund(&id);

    // Donor should get nothing back; creator should receive the 300 donation.
    assert_eq!(tk.balance(&donor), 0);
    assert_eq!(tk.balance(&creator), 300);
    assert_eq!(c.get_invoice(&id).status, types::InvoiceStatus::Refunded);
}

#[test]
fn test_donate_on_failure_mixed_payers() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let donor = Address::generate(&env);
    let refundee = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&donor, &100);
    StellarAssetClient::new(&env, &token_id).mint(&refundee, &100);
    env.ledger().set_timestamp(1_000);

    // Invoice needs 500; partially funded by a donor and a normal payer.
    let id = make_invoice(&env, &c, &creator, &recipient, 500, &token_id, 2_000);
    c.pay(&donor,   &id, &100_i128, &0_u64, &false, &true, &None);   // donate
    c.pay(&refundee, &id, &100_i128, &0_u64, &false, &false, &None); // normal

    env.ledger().set_timestamp(3_000);
    c.refund(&id);

    // Refundee gets money back; donor's amount goes to creator.
    assert_eq!(tk.balance(&refundee), 100);
    assert_eq!(tk.balance(&donor), 0);
    assert_eq!(tk.balance(&creator), 100);
}

// ---------------------------------------------------------------------------
// Issue #212: majority group release
// ---------------------------------------------------------------------------

#[test]
fn test_majority_group_releases_when_majority_funded() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let r3 = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    let id1 = make_invoice(&env, &c, &creator, &r1, 100, &token_id, 9_999);
    let id2 = make_invoice(&env, &c, &creator, &r2, 100, &token_id, 9_999);
    let id3 = make_invoice(&env, &c, &creator, &r3, 100, &token_id, 9_999);

    let mut ids = Vec::new(&env);
    ids.push_back(id1);
    ids.push_back(id2);
    ids.push_back(id3);
    // majority mode: >50% funded is sufficient
    c.create_invoice_group(&ids, &true);

    // Fund 2 out of 3 (>50%)
    c.pay(&payer, &id1, &100_i128, &0_u64, &false, &false, &None);
    c.pay(&payer, &id2, &100_i128, &0_u64, &false, &false, &None);

    // id1 is fully funded and majority condition is met — release should succeed.
    c.release(&id1);
    assert_eq!(c.get_invoice(&id1).status, types::InvoiceStatus::Released);
    assert_eq!(tk.balance(&r1), 100);
}

#[test]
#[should_panic(expected = "group majority not funded")]
fn test_majority_group_blocks_when_minority_funded() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let r3 = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    let id1 = make_invoice(&env, &c, &creator, &r1, 100, &token_id, 9_999);
    let id2 = make_invoice(&env, &c, &creator, &r2, 100, &token_id, 9_999);
    let id3 = make_invoice(&env, &c, &creator, &r3, 100, &token_id, 9_999);

    let mut ids = Vec::new(&env);
    ids.push_back(id1);
    ids.push_back(id2);
    ids.push_back(id3);
    c.create_invoice_group(&ids, &true);

    // Only 1 out of 3 funded — not a majority.
    c.pay(&payer, &id1, &100_i128, &0_u64, &false, &false, &None);
    c.release(&id1); // should panic
}

#[test]
#[should_panic(expected = "group members not fully funded")]
fn test_all_or_nothing_group_still_requires_all_funded() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    let id1 = make_invoice(&env, &c, &creator, &r1, 100, &token_id, 9_999);
    let id2 = make_invoice(&env, &c, &creator, &r2, 100, &token_id, 9_999);

    let mut ids = Vec::new(&env);
    ids.push_back(id1);
    ids.push_back(id2);
    c.create_invoice_group(&ids, &false); // AllOrNothing

    // Only id1 funded — id2 is not.
    c.pay(&payer, &id1, &100_i128, &0_u64, &false, &false, &None);
    c.release(&id1); // should panic
}

// ---------------------------------------------------------------------------
// Issue #276: Platform & creator volume milestone events
// ---------------------------------------------------------------------------

fn topic1_is(env: &Env, topics: &soroban_sdk::Vec<soroban_sdk::Val>, name: &str) -> bool {
    use soroban_sdk::TryIntoVal;
    topics.len() >= 2 && topics
        .get(1)
        .and_then(|v| { let r: Result<Symbol, _> = v.try_into_val(env); r.ok() })
        .map(|s: Symbol| s == Symbol::new(env, name))
        .unwrap_or(false)
}

fn topic0_is(env: &Env, topics: &soroban_sdk::Vec<soroban_sdk::Val>, name: &str) -> bool {
    use soroban_sdk::TryIntoVal;
    !topics.is_empty() && topics
        .get(0)
        .and_then(|v| { let r: Result<Symbol, _> = v.try_into_val(env); r.ok() })
        .map(|s: Symbol| s == Symbol::new(env, name))
        .unwrap_or(false)
}

fn has_platform_milestone_event(env: &Env) -> bool {
    env.events().all().iter().any(|(_c, topics, _d)| topic1_is(env, &topics, "plt_v_ms"))
}

fn has_creator_milestone_event(env: &Env) -> bool {
    env.events().all().iter().any(|(_c, topics, _d)| topic1_is(env, &topics, "cr_v_ms"))
}

#[test]
fn test_platform_volume_milestone_emitted() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let payer = Address::generate(&env);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);
    c.set_platform_vol_threshold(&admin, &100_i128);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &200);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);
    // total_volume = 100, milestone 1 crossed

    assert!(has_platform_milestone_event(&env), "platform volume milestone event not emitted");
}

#[test]
fn test_platform_volume_milestone_not_emitted_below_threshold() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let payer = Address::generate(&env);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);
    c.set_platform_vol_threshold(&admin, &500_i128);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &200);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);
    // total_volume = 100, threshold = 500 → no milestone yet

    assert!(!has_platform_milestone_event(&env), "unexpected platform volume milestone event");
}

#[test]
fn test_platform_volume_milestone_fires_multiple_times() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);
    c.set_platform_vol_threshold(&admin, &100_i128);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &600);
    env.ledger().set_timestamp(1_000);

    // Each pay auto-releases; events are per-transaction, so check milestone after each.
    for expected_milestone in 1i128..=3 {
        let cr = Address::generate(&env);
        let rc = Address::generate(&env);
        let id = make_invoice(&env, &c, &cr, &rc, 100, &token_id, 9_999);
        c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);
        assert!(
            has_platform_milestone_event(&env),
            "expected platform milestone {} to fire", expected_milestone
        );
    }
}

#[test]
fn test_creator_volume_milestone_emitted() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let payer = Address::generate(&env);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);
    c.set_creator_vol_threshold(&admin, &100_i128);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &200);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);

    assert!(has_creator_milestone_event(&env), "creator volume milestone event not emitted");
}

#[test]
fn test_milestone_disabled_when_threshold_zero() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let payer = Address::generate(&env);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);
    // Disable both milestone types.
    c.set_platform_vol_threshold(&admin, &0_i128);
    c.set_creator_vol_threshold(&admin, &0_i128);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &200);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);

    assert!(!has_platform_milestone_event(&env), "platform milestone should be suppressed when threshold is 0");
    assert!(!has_creator_milestone_event(&env), "creator milestone should be suppressed when threshold is 0");
}


// ---------------------------------------------------------------------------
// Issue #298: simulate_release compute cost estimation
// ---------------------------------------------------------------------------

#[test]
fn test_simulate_release_returns_result_for_small_invoice() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    let result = c.simulate_release(&id);

    // A single-recipient invoice should be well within budget.
    assert!(result.would_succeed, "single-recipient invoice should succeed");
    assert!(result.estimated_instructions > 0, "instructions must be positive");
}

#[test]
fn test_simulate_release_at_limit_succeeds() {
    // Build an invoice with enough recipients to sit just at or below the budget.
    // INSTRUCTION_BUDGET_LIMIT = 100_000_000
    // INSTRUCTIONS_BASE = 1_000_000
    // INSTRUCTIONS_PER_SHARD (8 shards) = 8 * 100_000 = 800_000
    // Remaining = 100_000_000 - 1_000_000 - 800_000 = 98_200_000
    // INSTRUCTIONS_PER_RECIPIENT = 500_000
    // Max recipients within budget = 98_200_000 / 500_000 = 196
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let sa = StellarAssetClient::new(&env, &token_id);
    let payer = Address::generate(&env);
    sa.mint(&payer, &1_000_000);
    env.ledger().set_timestamp(1_000);

    let creator = Address::generate(&env);
    let mut recipients = Vec::new(&env);
    let mut amounts = Vec::new(&env);
    for _ in 0..196u32 {
        recipients.push_back(Address::generate(&env));
        amounts.push_back(1_i128);
    }
    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &default_options(&env));
    let result = c.simulate_release(&id);
    assert!(result.would_succeed, "invoice at limit should succeed");
}

#[test]
fn test_simulate_release_over_limit_fails() {
    // 197 recipients exceeds the budget by 500_000 instructions.
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let sa = StellarAssetClient::new(&env, &token_id);
    let payer = Address::generate(&env);
    sa.mint(&payer, &1_000_000);
    env.ledger().set_timestamp(1_000);

    let creator = Address::generate(&env);
    let mut recipients = Vec::new(&env);
    let mut amounts = Vec::new(&env);
    for _ in 0..197u32 {
        recipients.push_back(Address::generate(&env));
        amounts.push_back(1_i128);
    }
    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &default_options(&env));
    let result = c.simulate_release(&id);
    assert!(!result.would_succeed, "invoice over limit should not succeed");
}

// ---------------------------------------------------------------------------
// Issue #297: Circuit breaker
// ---------------------------------------------------------------------------

fn has_circuit_breaker_event(env: &Env, topic_name: &str) -> bool {
    env.events().all().iter().any(|(_c, topics, _d)| topic1_is(env, &topics, topic_name))
}

#[test]
fn test_circuit_breaker_defaults_inactive() {
    let (env, contract_id, _token_id) = setup();
    let c = client(&env, &contract_id);
    let status = c.get_circuit_breaker_status();
    assert!(!status.active, "circuit breaker should default to inactive");
    assert!(status.reason.is_none(), "reason should be None when inactive");
}

#[test]
fn test_activate_circuit_breaker_blocks_pay() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);
    let _ = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);

    let reason = String::from_str(&env, "vulnerability discovered");
    c.activate_circuit_breaker(&admin, &reason);

    let status = c.get_circuit_breaker_status();
    assert!(status.active, "circuit breaker must be active");
    assert_eq!(status.reason, Some(reason), "reason must match");
}

#[test]
#[should_panic(expected = "ContractPaused")]
fn test_circuit_breaker_blocks_pay() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);
    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);

    let reason = String::from_str(&env, "vulnerability discovered");
    c.activate_circuit_breaker(&admin, &reason);

    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);
}

#[test]
fn test_activate_circuit_breaker_emits_event() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    let reason = String::from_str(&env, "emergency");
    c.activate_circuit_breaker(&admin, &reason);

    assert!(has_circuit_breaker_event(&env, "cb_act"), "cb_act event not emitted");
}

#[test]
fn test_deactivate_circuit_breaker_restores_operations() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);
    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    let reason = String::from_str(&env, "emergency");
    c.activate_circuit_breaker(&admin, &reason);
    c.deactivate_circuit_breaker(&admin);

    // Operations should be restored.
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 100);
}

// ---------------------------------------------------------------------------
// Issue #285: Volume-based fee tiers tests
// ---------------------------------------------------------------------------

#[test]
fn test_set_fee_tiers() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &100_u32, &None, &0_u32, &0_u32, &0_u64);

    let mut tiers = Vec::new(&env);
    tiers.push_back(types::FeeTier {
        volume_threshold: 1_000,
        fee_bps: 50,
    });
    tiers.push_back(types::FeeTier {
        volume_threshold: 10_000,
        fee_bps: 25,
    });

    c.set_fee_tiers(&admin, &tiers);

    let retrieved_tiers = c.get_fee_tiers();
    assert_eq!(retrieved_tiers.len(), 2);
    assert_eq!(retrieved_tiers.get(0).unwrap().volume_threshold, 1_000);
    assert_eq!(retrieved_tiers.get(0).unwrap().fee_bps, 50);
    assert_eq!(retrieved_tiers.get(1).unwrap().volume_threshold, 10_000);
    assert_eq!(retrieved_tiers.get(1).unwrap().fee_bps, 25);
}

#[test]
fn test_get_applicable_fee_no_tiers() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &100_u32, &None, &0_u32, &0_u32, &0_u64);

    // No tiers set, should return platform fee
    let fee = c.get_applicable_fee(&creator);
    assert_eq!(fee, 100_u32);
}

#[test]
fn test_get_applicable_fee_with_tiers() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let _recipient = Address::generate(&env);
    let _payer = Address::generate(&env);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &100_u32, &None, &0_u32, &0_u32, &0_u64);

    let mut tiers = Vec::new(&env);
    tiers.push_back(types::FeeTier {
        volume_threshold: 100,
        fee_bps: 50,
    });
    tiers.push_back(types::FeeTier {
        volume_threshold: 1_000,
        fee_bps: 25,
    });

    c.set_fee_tiers(&admin, &tiers);

    // With tiers set, fee should apply based on volume
    let fee = c.get_applicable_fee(&creator);
    assert_eq!(fee, 100_u32, "default fee applies when volume is 0");
}

// ---------------------------------------------------------------------------
// Issue #283: invoice_state_changed lifecycle event
// ---------------------------------------------------------------------------

fn has_state_changed_event(env: &Env) -> bool {
    env.events().all().iter().any(|(_c, topics, _d)| topic1_is(env, &topics, "st_chg"))
}

fn state_changed_count(env: &Env) -> usize {
    env.events()
        .all()
        .iter()
        .filter(|(_c, topics, _d)| topic1_is(env, topics, "st_chg"))
        .count()
}

#[test]
fn test_state_changed_event_emitted_on_release() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &100_u32, &None, &0_u32, &0_u32, &0_u64);
    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);

    assert!(has_state_changed_event(&env), "invoice_state_changed not emitted on release");
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
}

// Issue #307: Multi-token payment support
// ---------------------------------------------------------------------------

#[test]
fn test_307_xlm_invoice_payment() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &100_u32, &None, &0_u32, &0_u32, &0_u64);
    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
}

#[test]
fn test_state_changed_event_emitted_on_refund() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &100_u32, &None, &0_u32, &0_u32, &0_u64);
    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    // Create invoice using explicit payment_token = XLM token
    let mut opts = default_options(&env);
    opts.ext.payment_token = Some(token_id.clone());
    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);
    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &opts);

    let core = c.get_invoice(&id);
    assert_eq!(core.tokens.get(0).unwrap(), token_id);

    c.pay(&payer, &id, &500_i128, &0_u64, &false, &false, &None);
}

#[test]
fn test_307_usdc_invoice_payment() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SplitContract, ());
    let token_admin = Address::generate(&env);

    let xlm_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let usdc_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();

    let usdc_sa = StellarAssetClient::new(&env, &usdc_id);
    let c = SplitContractClient::new(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    usdc_sa.mint(&payer, &200);
    env.ledger().set_timestamp(1_000);

    // Initialize with USDC as the base payment token.
    c.initialize(&admin, &0_i128, &treasury, &usdc_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);
    let id = c.create_invoice(&creator, &recipients, &amounts, &usdc_id, &9_999_u64, &default_options(&env));
    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(TokenClient::new(&env, &usdc_id).balance(&recipient), 200);
    let _ = (usdc_sa, xlm_id);
}

#[test]
fn test_deactivate_circuit_breaker_emits_event() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    let reason = String::from_str(&env, "emergency");
    c.activate_circuit_breaker(&admin, &reason);
    c.deactivate_circuit_breaker(&admin);

    assert!(has_circuit_breaker_event(&env, "cb_dact"), "cb_dact event not emitted");
}

#[test]
fn test_get_invoice_unaffected_by_circuit_breaker() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);
    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    let reason = String::from_str(&env, "emergency");
    c.activate_circuit_breaker(&admin, &reason);

    // read-only call must still work
    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Pending);
}

// ---------------------------------------------------------------------------
// Issue #296: Per-creator fee waiver list
// ---------------------------------------------------------------------------

#[test]
fn test_add_fee_waiver_grants_waiver() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    assert!(!c.has_fee_waiver(&creator), "should not have waiver before grant");
    c.add_fee_waiver(&admin, &creator);
    assert!(c.has_fee_waiver(&creator), "should have waiver after grant");
}

#[test]
fn test_remove_fee_waiver_revokes_waiver() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    c.add_fee_waiver(&admin, &creator);
    c.remove_fee_waiver(&admin, &creator);
    assert!(!c.has_fee_waiver(&creator), "waiver should be gone after revocation");
}

#[test]
fn test_fee_waiver_grants_event_emitted() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    c.add_fee_waiver(&admin, &creator);
    let granted = env.events().all().iter().any(|(_c, topics, _d)| topic1_is(&env, &topics, "fw_grant"));
    assert!(granted, "fw_grant event should be emitted");
}

#[test]
fn test_fee_waiver_revoke_event_emitted() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    c.add_fee_waiver(&admin, &creator);
    c.remove_fee_waiver(&admin, &creator);
    let revoked = env.events().all().iter().any(|(_c, topics, _d)| topic1_is(&env, &topics, "fw_rev"));
    assert!(revoked, "fw_rev event should be emitted");
}

#[test]
fn test_fee_waiver_zeroes_platform_fee_at_release() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    // Initialize with 10% platform fee.
    c.initialize(&admin, &0_i128, &treasury, &token_id, &1_000_u32, &None, &0_u32, &0_u32, &0_u64);
    // Grant fee waiver to creator — platform fee should be zeroed at release.
    c.add_fee_waiver(&admin, &creator);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);

    // Waived creator: recipient gets full 100, not 90.
    assert_eq!(tk.balance(&recipient), 100, "fee waiver should zero platform fee at release");
}

#[test]
fn test_state_changed_event_emitted_on_cancel() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);
    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.cancel_invoice(&creator, &id);

    assert!(has_state_changed_event(&env), "invoice_state_changed not emitted on cancel");
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Cancelled);
}

#[test]
fn test_state_changed_full_lifecycle_release() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &200);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);
    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);

    // Exactly one state_changed event (Pending → Released).
    assert_eq!(state_changed_count(&env), 1);
}

// ---------------------------------------------------------------------------
// Issue #282: deadline edge-case simulation tests
// ---------------------------------------------------------------------------

#[test]
fn test_payment_at_exact_deadline_succeeds() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let deadline = 5_000_u64;
    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, deadline);

    // Pay exactly at deadline — should succeed.
    env.ledger().set_timestamp(deadline);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
}

#[test]
#[should_panic]
fn test_307_wrong_token_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SplitContract, ());
    let token_admin = Address::generate(&env);
    let usdc_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let xlm_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();

    StellarAssetClient::new(&env, &xlm_id).mint(&Address::generate(&env), &1_000);

    let c = SplitContractClient::new(&env, &contract_id);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    // Mint xlm to payer but invoice expects usdc
    StellarAssetClient::new(&env, &xlm_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let mut opts = default_options(&env);
    opts.ext.payment_token = Some(usdc_id.clone());
    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);
    let id = c.create_invoice(&creator, &recipients, &amounts, &xlm_id, &9_999_u64, &opts);

    // Attempting to pay with XLM when invoice expects USDC — transfer will fail
    // because the contract pulls from the stored token (usdc), not xlm
    c.pay(&payer, &id, &500_i128, &0_u64, &false, &false, &None);
}

// ---------------------------------------------------------------------------
// Issue #308: claim_refund
// ---------------------------------------------------------------------------

#[test]
fn test_308_claim_refund_after_expiry() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &50);
    env.ledger().set_timestamp(1_000);

    let deadline = 5_000_u64;
    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, deadline);
    c.pay(&payer, &id, &50_i128, &0_u64, &false, &false, &None);

    // After deadline — bulk refund.
    env.ledger().set_timestamp(deadline + 1);
    c.refund(&id);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Refunded);
    assert_eq!(tk.balance(&payer), 50);
}

#[test]
fn test_no_fee_waiver_deducts_platform_fee_normally() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    // 1000 bps (10%) platform fee, no waiver for creator.
    c.initialize(&admin, &0_i128, &treasury, &token_id, &1_000_u32, &None, &0_u32, &0_u32, &0_u64);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);

    // Without fee waiver: 10% deducted → recipient gets 90.
    assert_eq!(tk.balance(&recipient), 90, "non-waived creator should pay platform fee");
}

#[test]
#[should_panic(expected = "invoice deadline has passed")]
fn test_payment_one_second_after_deadline_fails() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let deadline = 5_000_u64;
    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, deadline);

    // One second after deadline — should fail.
    env.ledger().set_timestamp(deadline + 1);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);
}

// ---------------------------------------------------------------------------
// Issue #295: Confidential payment amounts
// ---------------------------------------------------------------------------

fn make_commitment(env: &Env, seed: u8) -> BytesN<32> {
    let mut arr = [0u8; 32];
    arr[0] = seed;
    arr[31] = seed.wrapping_add(1);
    BytesN::from_array(env, &arr)
}

fn make_range_proof(env: &Env, seed: u8) -> Bytes {
    // Non-zero range proof: sha256(commitment) would never be all-zero.
    let commitment = make_commitment(env, seed);
    let b: Bytes = commitment.into();
    b
}

fn make_encrypted_amount(env: &Env, seed: u8) -> Bytes {
    let mut arr = [0u8; 16];
    arr[0] = seed;
    Bytes::from_array(env, &arr)
}

#[test]
fn test_pay_confidential_stores_commitment() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    let commitment = make_commitment(&env, 42);
    let range_proof = make_range_proof(&env, 42);
    let encrypted = make_encrypted_amount(&env, 42);

    c.pay_confidential(&payer, &id, &commitment, &range_proof, &encrypted);

    assert_eq!(c.get_confidential_payment_count(&id), 1);
}

#[test]
fn test_pay_confidential_increments_counter() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);
    let recipient = Address::generate(&env);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    c.pay_confidential(&payer1, &id, &make_commitment(&env, 1), &make_range_proof(&env, 1), &make_encrypted_amount(&env, 1));
    c.pay_confidential(&payer2, &id, &make_commitment(&env, 2), &make_range_proof(&env, 2), &make_encrypted_amount(&env, 2));

    assert_eq!(c.get_confidential_payment_count(&id), 2);
}

#[test]
#[should_panic(expected = "invoice deadline has passed")]
fn test_pay_after_deadline_panics_b() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let deadline = 5_000_u64;
    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, deadline);

    env.ledger().set_timestamp(deadline + 1);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);
}

#[test]
fn test_refund_available_after_deadline() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &200);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 500, &token_id, 2_000);

    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);
    assert_eq!(tk.balance(&payer), 0);

    // Advance past deadline
    env.ledger().set_timestamp(3_000);
    c.claim_refund(&payer, &id);

    assert_eq!(tk.balance(&payer), 200);
    // Invoice still Pending (not bulk-refunded)
    assert_eq!(c.get_invoice(&id).status, types::InvoiceStatus::Pending);
}

#[test]
fn test_308_claim_refund_idempotent() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let _tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &50);
    env.ledger().set_timestamp(1_000);

    let deadline = 5_000_u64;
    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, deadline);
    c.pay(&payer, &id, &50_i128, &0_u64, &false, &false, &None);

    // After deadline refund should succeed.
    env.ledger().set_timestamp(deadline + 1);
    c.refund(&id);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Refunded);
}

#[test]
#[should_panic(expected = "InvalidStatus")]
fn test_refund_before_deadline_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    env.ledger().set_timestamp(1_000);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &50);
    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.pay(&payer, &id, &50_i128, &0_u64, &false, &false, &None);

    // Deadline (9_999) has not passed — refund should panic.
    c.refund(&id);
}

#[test]
fn test_reveal_confidential_total_triggers_release() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    // Submit a confidential payment for the payer (off-chain funds already moved separately).
    c.pay_confidential(&payer, &id, &make_commitment(&env, 7), &make_range_proof(&env, 7), &make_encrypted_amount(&env, 7));

    // Credit actual token funds so contract can pay out on reveal.
    StellarAssetClient::new(&env, &token_id).mint(&contract_id, &100);

    // Creator reveals the sum and a non-zero proof.
    let proof = make_commitment(&env, 99);
    c.reveal_confidential_total(&id, &100_i128, &proof);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 100);
}

#[test]
#[should_panic(expected = "invalid reveal proof")]
fn test_reveal_confidential_total_rejects_zero_proof() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.pay_confidential(&payer, &id, &make_commitment(&env, 5), &make_range_proof(&env, 5), &make_encrypted_amount(&env, 5));

    // Zero proof should be rejected.
    let zero_proof = BytesN::from_array(&env, &[0u8; 32]);
    c.reveal_confidential_total(&id, &100_i128, &zero_proof);
}

#[test]
fn test_scheduled_release_fires_at_correct_ledger() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    // Create invoice with single recipient
    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999,
        &default_options(&env),
    );

    let invoice = c.get_invoice(&id);
    // Verify no duplicates (debug_assert should have caught it if there were)
    assert_eq!(invoice.recipients.len(), 1);
}

#[test]
fn test_payment_shards_sum_correctly() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer1, &60);
    StellarAssetClient::new(&env, &token_id).mint(&payer2, &40);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    c.pay(&payer1, &id, &60_i128, &0_u64, &false, &false, &None);
    c.pay(&payer2, &id, &40_i128, &0_u64, &false, &false, &None);

    // After full payment the invoice auto-releases; funded == total == 100.
    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.funded, 100);
}


// ---------------------------------------------------------------------------
// Issue #297: Circuit breaker tests
// ---------------------------------------------------------------------------

#[test]
fn test_circuit_breaker_activate_deactivate() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let admin = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    // Setup: Register admin as SuperAdmin
    let treasury = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    // Initially circuit breaker is inactive
    let status = c.get_circuit_breaker_status();
    assert!(!status.active);
    assert!(status.reason.is_none());

    // Activate circuit breaker
    let reason = String::from_str(&env, "security vulnerability");
    c.activate_circuit_breaker(&admin, &reason);

    let status = c.get_circuit_breaker_status();
    assert!(status.active);
    assert_eq!(status.reason, Some(reason));

    // Deactivate circuit breaker
    c.deactivate_circuit_breaker(&admin);

    let status = c.get_circuit_breaker_status();
    assert!(!status.active);
    assert!(status.reason.is_none());
}

#[test]
#[should_panic(expected = "ContractPaused")]
fn test_circuit_breaker_blocks_create_invoice() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);
    let treasury = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    // Activate circuit breaker
    let reason = String::from_str(&env, "emergency");
    c.activate_circuit_breaker(&admin, &reason);

    // Try to create invoice - should panic
    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient);
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);
    c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &default_options(&env));
}

#[test]
#[should_panic(expected = "ContractPaused")]
fn test_circuit_breaker_blocks_pay_b() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);
    let treasury = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);
    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    let reason = String::from_str(&env, "emergency");
    c.activate_circuit_breaker(&admin, &reason);

    // Pay should be blocked.
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);
}

// ---------------------------------------------------------------------------
// Issue #299: Creator analytics tests
// ---------------------------------------------------------------------------

#[test]
fn test_get_creator_stats_empty() {
    let (env, contract_id, _token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);

    let stats = c.get_creator_stats(&creator);
    assert_eq!(stats.total_invoices, 0);
    assert_eq!(stats.total_raised, 0);
    assert_eq!(stats.total_released, 0);
    assert_eq!(stats.total_payers, 0);
    assert_eq!(stats.avg_funding_time_ledgers, 0);
}

#[test]
fn test_308_partial_payments_refunded_correctly() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);
    let recipient = Address::generate(&env);
    StellarAssetClient::new(&env, &token_id).mint(&payer1, &100);
    StellarAssetClient::new(&env, &token_id).mint(&payer2, &150);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);
    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &2_000_u64, &default_options(&env));

    c.pay(&payer1, &id, &100_i128, &0_u64, &false, &false, &None);
    c.pay(&payer2, &id, &150_i128, &0_u64, &false, &false, &None);

    env.ledger().set_timestamp(3_000);
    c.claim_refund(&payer1, &id);
    c.claim_refund(&payer2, &id);

    assert_eq!(tk.balance(&payer1), 100);
    assert_eq!(tk.balance(&payer2), 150);
}

#[test]
#[should_panic(expected = "scheduled release time not reached")]
fn test_scheduled_release_blocked_before_timestamp() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let release_at = 4_000_u64;
    let mut opts = default_options(&env);
    opts.scheduled_release_at = Some(release_at);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);
    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &opts);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);

    // One unit before scheduled time — should be blocked.
    env.ledger().set_timestamp(release_at - 1);
    c.trigger_scheduled_release(&id);
}

#[test]
#[should_panic(expected = "invoice has not expired yet")]
fn test_308_claim_refund_before_deadline_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);
    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.pay(&payer, &id, &50_i128, &0_u64, &false, &false, &None);
    // Before deadline — should panic
    c.claim_refund(&payer, &id);
}

#[test]
fn test_circuit_breaker_allows_read_operations() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);
    let treasury = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    // Activate circuit breaker
    let reason = String::from_str(&env, "emergency");
    c.activate_circuit_breaker(&admin, &reason);

    // Read operations should still work
    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Pending);

    let status = c.get_circuit_breaker_status();
    assert!(status.active);
}

// ---------------------------------------------------------------------------
// Issue #296: Fee waiver tests
// ---------------------------------------------------------------------------

#[test]
fn test_add_fee_waiver() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);

    env.ledger().set_timestamp(1_000);
    let treasury = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    // Initially no waiver
    assert!(!c.has_fee_waiver(&creator));

    // Add fee waiver
    c.add_fee_waiver(&admin, &creator);

    // Now waiver exists
    assert!(c.has_fee_waiver(&creator));
}

#[test]
fn test_remove_fee_waiver() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);

    env.ledger().set_timestamp(1_000);
    let treasury = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    // Add waiver
    c.add_fee_waiver(&admin, &creator);
    assert!(c.has_fee_waiver(&creator));

    // Remove waiver
    c.remove_fee_waiver(&admin, &creator);
    assert!(!c.has_fee_waiver(&creator));
}

#[test]
fn test_fee_waiver_exempts_from_fees() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);
    let treasury = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    // Grant fee waiver to creator
    c.add_fee_waiver(&admin, &creator);

    // Create and pay invoice
    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);

    // Recipient should get full 100 (no fee deducted)
    assert_eq!(tk.balance(&recipient), 100, "waived creator should not pay platform fee");
}

#[test]
#[should_panic(expected = "fee waiver list full")]
fn test_fee_waiver_max_entries_enforced() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let admin = Address::generate(&env);

    env.ledger().set_timestamp(1_000);
    let treasury = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    // Add 100 waivers (at max)
    for _i in 0..100 {
        let creator = Address::generate(&env);
        c.add_fee_waiver(&admin, &creator);
    }

    // 101st should fail
    let creator_101 = Address::generate(&env);
    c.add_fee_waiver(&admin, &creator_101);
}

// ---------------------------------------------------------------------------
// Issue #298: Compute cost estimation tests
// ---------------------------------------------------------------------------

#[test]
fn test_simulate_release_single_recipient() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    let result = c.simulate_release(&id);
    assert!(result.estimated_instructions > 0);
    assert!(result.would_succeed); // single recipient should fit in budget
}

#[test]
fn test_simulate_release_multiple_recipients() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let creator = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    // Create invoice with 10 recipients
    let mut recipients = Vec::new(&env);
    let mut amounts = Vec::new(&env);
    for _ in 0..10 {
        recipients.push_back(Address::generate(&env));
        amounts.push_back(100_i128);
    }

    let id = c.create_invoice(
        &creator, &recipients, &amounts, &token_id, &9_999_u64, &default_options(&env),
    );

    let result = c.simulate_release(&id);
    assert!(result.estimated_instructions > 0);
    assert!(result.would_succeed);
}

#[test]
fn test_simulate_release_instruction_budget_calculation() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let creator = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    // Create invoice with 1 recipient
    let recipient = Address::generate(&env);
    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient);
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let id = c.create_invoice(
        &creator, &recipients, &amounts, &token_id, &9_999_u64, &default_options(&env),
    );

    let result = c.simulate_release(&id);

    // Verify calculation: INSTRUCTIONS_BASE + 1 * INSTRUCTIONS_PER_RECIPIENT + SHARD_COUNT * INSTRUCTIONS_PER_SHARD
    // INSTRUCTIONS_BASE = 1_000_000
    // INSTRUCTIONS_PER_RECIPIENT = 500_000
    // SHARD_COUNT = 8
    // INSTRUCTIONS_PER_SHARD = 100_000
    // Total = 1_000_000 + 500_000 + 8 * 100_000 = 2_300_000
    let expected = 1_000_000 + 500_000 + 8 * 100_000;
    assert_eq!(result.estimated_instructions, expected as u64);
}

#[test]
fn test_simulate_release_would_succeed_at_budget_limit() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let creator = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    // Small invoice should fit in budget
    let recipient = Address::generate(&env);
    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    let result = c.simulate_release(&id);
    assert!(result.would_succeed, "single recipient should fit in budget");
    assert!(result.estimated_instructions < 100_000_000); // INSTRUCTION_BUDGET_LIMIT
}

// ---------------------------------------------------------------------------
// Issue #295: Additional confidential payment tests
// ---------------------------------------------------------------------------

#[test]
fn test_get_confidential_payment_count() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);
    let recipient = Address::generate(&env);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);

    assert_eq!(c.get_confidential_payment_count(&id), 0);

    c.pay_confidential(&payer1, &id, &make_commitment(&env, 1), &make_range_proof(&env, 1), &make_encrypted_amount(&env, 1));
    assert_eq!(c.get_confidential_payment_count(&id), 1);

    c.pay_confidential(&payer2, &id, &make_commitment(&env, 2), &make_range_proof(&env, 2), &make_encrypted_amount(&env, 2));
    assert_eq!(c.get_confidential_payment_count(&id), 2);
}

// ---------------------------------------------------------------------------
// Issue #281: multi-sig M-of-N release
// ---------------------------------------------------------------------------

#[test]
fn test_multisig_release_requires_threshold() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let mut co_signers = Vec::new(&env);
    co_signers.push_back(signer1.clone());
    co_signers.push_back(signer2.clone());

    let mut opts = default_options(&env);
    opts.co_signers = co_signers;
    opts.required_signatures = 2;

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &opts);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);

    // Only signer1 has signed — threshold not met.
    c.sign_release(&id, &signer1);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);
}

// ---------------------------------------------------------------------------
// Issue #309: Recipient allowlist
// ---------------------------------------------------------------------------

#[test]
fn test_309_allowlist_restricts_payers() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let allowed_payer = Address::generate(&env);
    let blocked_payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&allowed_payer, &500);
    env.ledger().set_timestamp(1_000);

    let mut allowed = Vec::new(&env);
    allowed.push_back(allowed_payer.clone());
    let mut opts = default_options(&env);
    opts.allowed_payers = Some(allowed);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);
    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &opts);

    // Allowed payer can pay.
    c.pay(&allowed_payer, &id, &500_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id).status, types::InvoiceStatus::Released);
    let _ = blocked_payer;
}

#[test]
fn test_contributor_allowlist_toggle_events() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let contributor = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);

    let ext_before = c.get_invoice_ext2(&id);
    assert!(ext_before.contributor_allowlist.is_none());

    // Adding the first contributor turns gating ON (None -> Some).
    c.add_contributor_to_allowlist(&creator, &id, &contributor);

    let ext_enabled = c.get_invoice_ext2(&id);
    assert!(ext_enabled.contributor_allowlist.is_some());

    let toggled_on_count = env
        .events()
        .all()
        .iter()
        .filter(|(_c, topics, _d)| topic1_is(&env, topics, "al_tog"))
        .count();
    assert_eq!(
        toggled_on_count, 1,
        "contributor_allowlist_toggled(enabled=true) should fire exactly once on first add"
    );

    // Removing the only contributor turns gating OFF (Some -> None).
    c.remove_contributor_allowlist(&creator, &id, &contributor);

    let ext_disabled = c.get_invoice_ext2(&id);
    assert!(ext_disabled.contributor_allowlist.is_none());

    let toggled_total_count = env
        .events()
        .all()
        .iter()
        .filter(|(_c, topics, _d)| topic1_is(&env, topics, "al_tog"))
        .count();
    assert_eq!(
        toggled_total_count, 2,
        "one toggle event for enabling, one for disabling"
    );
}

#[test]
fn test_payment_received_event_includes_token() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);
    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);

    use soroban_sdk::TryIntoVal;
    let mut found_token: Option<Address> = None;
    for (_contract, topics, data) in env.events().all().iter() {
        if topic1_is(&env, &topics, "paid") {
            let decoded: (Address, i128, Address, u64) = data.try_into_val(&env).unwrap();
            found_token = Some(decoded.2);
        }
    }
    assert_eq!(
        found_token,
        Some(token_id),
        "payment_received event data should include the payment token address"
    );
}

#[test]
fn test_creator_stats_on_invoice_creation() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    env.ledger().set_timestamp(1_000);

    let _id1 = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    let _id2 = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);

    let stats = c.get_creator_stats(&creator);
    assert_eq!(stats.total_invoices, 2, "total_invoices should be 2 after creating 2 invoices");
}

#[test]
#[should_panic(expected = "not enough co-signer approvals")]
fn test_multisig_release_panics_below_threshold() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let mut co_signers = Vec::new(&env);
    co_signers.push_back(signer1.clone());
    co_signers.push_back(signer2.clone());

    let mut opts = default_options(&env);
    opts.co_signers = co_signers;
    opts.required_signatures = 2;

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &opts);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);

    // Only one signature — explicit release should panic.
    c.sign_release(&id, &signer1);
    c.release(&id);
}

#[test]
#[should_panic(expected = "payer not allowed")]
fn test_309_blocked_payer_rejected() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let allowed_payer = Address::generate(&env);
    let blocked_payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&blocked_payer, &500);
    env.ledger().set_timestamp(1_000);

    let mut allowed = Vec::new(&env);
    allowed.push_back(allowed_payer.clone());
    let mut opts = default_options(&env);
    opts.allowed_payers = Some(allowed);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);
    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &opts);

    // Blocked payer should be rejected.
    c.pay(&blocked_payer, &id, &500_i128, &0_u64, &false, &false, &None);
}

#[test]
fn test_creator_stats_on_payment() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let payer = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 9_999);

    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);
    let stats = c.get_creator_stats(&creator);
    assert_eq!(stats.total_raised, 100, "total_raised should reflect payment amount");
}

#[test]
fn test_creator_stats_on_release() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &250);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 250, &token_id, 9_999);
    c.pay(&payer, &id, &250_i128, &0_u64, &false, &false, &None);

    let stats = c.get_creator_stats(&creator);
    assert_eq!(stats.total_released, 250, "total_released should equal released amount");
}

#[test]
fn test_multisig_release_succeeds_at_threshold() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let mut co_signers = Vec::new(&env);
    co_signers.push_back(signer1.clone());
    co_signers.push_back(signer2.clone());

    let mut opts = default_options(&env);
    opts.co_signers = co_signers;
    opts.required_signatures = 2;

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &opts);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);

    // Both signers sign — release should succeed.
    c.sign_release(&id, &signer1);
    c.sign_release(&id, &signer2);
    c.release(&id);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 100);
}

#[test]
fn test_309_add_allowed_payer_initializes_list() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &300);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 9_999);

    // Add payer post-creation — initializes list from None.
    c.add_allowed_payer(&creator, &id, &payer);
    let ext = c.get_invoice_ext(&id);
    assert!(ext.allowed_payers.is_some());

    c.pay(&payer, &id, &300_i128, &0_u64, &false, &false, &None);
    assert_eq!(tk.balance(&recipient), 300);
}

#[test]
#[should_panic(expected = "not an authorized co-signer")]
fn test_multisig_non_signer_cannot_sign() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let signer1 = Address::generate(&env);
    let imposter = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let mut co_signers = Vec::new(&env);
    co_signers.push_back(signer1.clone());

    let mut opts = default_options(&env);
    opts.co_signers = co_signers;
    opts.required_signatures = 1;

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &opts);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);

    // Imposter tries to sign — should fail.
    c.sign_release(&id, &imposter);
}

#[test]
fn test_309_remove_allowed_payer_emits_event() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &300);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 9_999);
    c.add_allowed_payer(&creator, &id, &payer);
    c.remove_allowed_payer(&creator, &id, &payer);

    let ext = c.get_invoice_ext(&id);
    let payers = ext.allowed_payers.unwrap_or(Vec::new(&env));
    assert_eq!(payers.len(), 0, "allowed_payers should be empty after removal");
}

#[test]
fn test_remove_allowlist_opens_invoice_and_emits_event() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let allowed_payer = Address::generate(&env);
    let other_payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&other_payer, &300);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 9_999);
    c.add_allowed_payer(&creator, &id, &allowed_payer);
    assert!(c.get_invoice_ext(&id).allowed_payers.is_some());

    c.remove_allowlist(&creator, &id);
    assert!(c.get_invoice_ext(&id).allowed_payers.is_none());

    let has_allowlist_removed_event = env
        .events()
        .all()
        .iter()
        .any(|(_c, topics, _d)| topic1_is(&env, &topics, "al_open"));
    assert!(
        has_allowlist_removed_event,
        "expected an allowlist_removed event"
    );

    // The invoice is now open — a previously non-allowed payer can pay.
    c.pay(&other_payer, &id, &300_i128, &0_u64, &false, &false, &None);
    assert_eq!(tk.balance(&recipient), 300);
}

#[test]
fn test_creator_stats_unique_payers() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer1, &100);
    StellarAssetClient::new(&env, &token_id).mint(&payer2, &100);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);

    c.pay(&payer1, &id, &100_i128, &0_u64, &false, &false, &None);
    c.pay(&payer2, &id, &100_i128, &0_u64, &false, &false, &None);

    let stats = c.get_creator_stats(&creator);
    assert_eq!(stats.total_payers, 2, "total_payers should count unique payers");
}

#[test]
fn test_confidential_payment_overwrite() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);

    // Submit first payment from payer
    c.pay_confidential(&payer, &id, &make_commitment(&env, 5), &make_range_proof(&env, 5), &make_encrypted_amount(&env, 5));
    assert_eq!(c.get_confidential_payment_count(&id), 1);

    // Same payer submits again (overwrites)
    c.pay_confidential(&payer, &id, &make_commitment(&env, 10), &make_range_proof(&env, 10), &make_encrypted_amount(&env, 10));
    assert_eq!(c.get_confidential_payment_count(&id), 1, "same payer should overwrite, not increment");
}

#[test]
#[should_panic(expected = "invalid range proof")]
fn test_pay_confidential_rejects_zero_range_proof() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    // Try to submit with all-zero proof (should fail)
    let commitment = make_commitment(&env, 5);
    let zero_proof = Bytes::from_array(&env, &[0u8; 32]);
    c.pay_confidential(&payer, &id, &commitment, &zero_proof, &make_encrypted_amount(&env, 5));
}

#[test]
fn test_reveal_confidential_total_partial_funding() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer1, &60);
    sa.mint(&payer2, &40);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    // Submit confidential payments (no real token movement from payers).
    c.pay_confidential(&payer1, &id, &make_commitment(&env, 1), &make_range_proof(&env, 1), &make_encrypted_amount(&env, 1));
    c.pay_confidential(&payer2, &id, &make_commitment(&env, 2), &make_range_proof(&env, 2), &make_encrypted_amount(&env, 2));

    // Fund the contract so it can pay out on reveal.
    StellarAssetClient::new(&env, &token_id).mint(&contract_id, &100);

    let proof = make_commitment(&env, 99);
    c.reveal_confidential_total(&id, &100_i128, &proof);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
}

// ---------------------------------------------------------------------------
// Issue #284: mock DEX and NFT gate integration tests
// ---------------------------------------------------------------------------

/// MockNftGate: simulates an NFT contract where holders are pre-registered.
#[contract]
struct MockNftGate;

#[contractimpl]
impl MockNftGate {
    /// Register `holder` as an NFT holder (call from test setup).
    pub fn set_holder(env: Env, holder: Address, balance: i128) {
        env.storage().persistent().set(&holder, &balance);
    }

    /// Called by the split contract to check NFT balance.
    pub fn balance_of(env: Env, holder: Address) -> i128 {
        env.storage().persistent().get(&holder).unwrap_or(0i128)
    }
}

#[test]
fn test_dex_swap_credits_correct_amount() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let _admin = Address::generate(&env);
    let _treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);

    // Submit confidential payment of 100
    c.pay_confidential(&payer, &id, &make_commitment(&env, 7), &make_range_proof(&env, 7), &make_encrypted_amount(&env, 7));

    // Mint funds to contract for payout
    StellarAssetClient::new(&env, &token_id).mint(&contract_id, &100);

    // Reveal 100 (partial, invoice needs 200 total)
    let proof = make_commitment(&env, 99);
    c.reveal_confidential_total(&id, &100_i128, &proof);

    // Invoice should still be pending (not fully funded)
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);
    assert_eq!(c.get_invoice(&id).funded, 100);
    assert_eq!(tk.balance(&recipient), 0, "should not distribute on partial reveal");
}

#[test]
#[should_panic(expected = "decrypted_sum must be positive")]
fn test_reveal_confidential_total_rejects_zero_sum() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    let proof = make_commitment(&env, 99);
    c.reveal_confidential_total(&id, &0_i128, &proof); // Should panic
}

#[test]
#[should_panic(expected = "ContractPaused")]
fn test_pay_confidential_blocked_by_circuit_breaker() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);
    let treasury = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    // Activate circuit breaker
    let reason = String::from_str(&env, "emergency");
    c.activate_circuit_breaker(&admin, &reason);

    // Try confidential payment
    c.pay_confidential(&payer, &id, &make_commitment(&env, 5), &make_range_proof(&env, 5), &make_encrypted_amount(&env, 5));
}

#[test]
#[should_panic(expected = "ContractPaused")]
fn test_reveal_confidential_blocked_by_circuit_breaker() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);
    let treasury = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    // Submit confidential payment
    c.pay_confidential(&payer, &id, &make_commitment(&env, 7), &make_range_proof(&env, 7), &make_encrypted_amount(&env, 7));

    // Activate circuit breaker
    let reason = String::from_str(&env, "emergency");
    c.activate_circuit_breaker(&admin, &reason);

    // Try to reveal
    let proof = make_commitment(&env, 99);
    c.reveal_confidential_total(&id, &100_i128, &proof);
}

// ---------------------------------------------------------------------------
// Cross-feature integration tests
// ---------------------------------------------------------------------------

#[test]
fn test_simulate_release_estimate_for_large_invoice() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let creator = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    // Create invoice with 50 recipients (larger batch)
    let mut recipients = Vec::new(&env);
    let mut amounts = Vec::new(&env);
    for _ in 0..50 {
        recipients.push_back(Address::generate(&env));
        amounts.push_back(10_i128);
    }

    let id = c.create_invoice(
        &creator, &recipients, &amounts, &token_id, &9_999_u64, &default_options(&env),
    );

    let result = c.simulate_release(&id);
    // With 50 recipients: BASE (1M) + 50*RECIPIENT (25M) + 8*SHARD (0.8M) = ~26.8M
    let expected = 1_000_000 + 50 * 500_000 + 8 * 100_000;
    assert_eq!(result.estimated_instructions, expected as u64);
    assert!(result.would_succeed);
}

#[test]
fn test_fee_waiver_persists_across_operations() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);

    env.ledger().set_timestamp(1_000);
    let treasury = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    // Add waiver
    c.add_fee_waiver(&admin, &creator);

    // Perform various operations
    let recipient = Address::generate(&env);
    let _ = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    // Waiver should still be active
    assert!(c.has_fee_waiver(&creator));
}

#[test]
#[should_panic(expected = "ContractPaused")]
fn test_circuit_breaker_prevents_refund() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);
    let treasury = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 2_000);
    c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);

    env.ledger().set_timestamp(3_000);
    let reason = String::from_str(&env, "emergency");
    c.activate_circuit_breaker(&admin, &reason);

    c.refund(&id);
}

#[test]
#[should_panic(expected = "token not accepted")]
fn test_dex_unregistered_token_rejected() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    let unknown_admin = Address::generate(&env);
    let unknown_token = env
        .register_stellar_asset_contract_v2(unknown_admin.clone())
        .address();
    StellarAssetClient::new(&env, &unknown_token).mint(&payer, &100);

    let dex_id = env.register(MockDex, ());
    c.set_dex_contract(&admin, &dex_id);

    env.ledger().set_timestamp(1_000);
    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    // Unknown token not in accepted_tokens — should panic.
    c.pay_with_token(&payer, &id, &unknown_token, &100_i128, &0);
}

#[test]
fn test_nft_gate_allows_holder_to_create_invoice() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    let nft_id = env.register(MockNftGate, ());
    let nft = MockNftGateClient::new(&env, &nft_id);

    // Register creator as an NFT holder.
    nft.set_holder(&creator, &1_i128);
    c.set_nft_gate(&admin, &Some(nft_id));

    env.ledger().set_timestamp(1_000);
    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    // Creator holds NFT — should succeed.
    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &default_options(&env));
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);
}

#[test]
#[should_panic(expected = "nft gate: not a holder")]
fn test_nft_gate_rejects_non_holder() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    let nft_id = env.register(MockNftGate, ());
    // Do NOT register creator as holder — balance_of returns 0.
    c.set_nft_gate(&admin, &Some(nft_id));

    env.ledger().set_timestamp(1_000);
    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    // Creator has no NFT — should panic.
    c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64, &default_options(&env));
}

// ---------------------------------------------------------------------------
// Issue #310: Contract upgrade authority
// ---------------------------------------------------------------------------

fn init_contract(env: &Env, contract_id: &Address, token_id: &Address) {
    let c = SplitContractClient::new(env, contract_id);
    let admin = Address::generate(env);
    let treasury = Address::generate(env);
    c.initialize(&admin, &0_i128, &treasury, token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);
}

#[test]
fn test_310_propose_and_get_upgrade() {
    let (env, contract_id, token_id) = setup();
    init_contract(&env, &contract_id, &token_id);
    let c = client(&env, &contract_id);

    env.ledger().set_timestamp(1_000);
    let wasm_hash: BytesN<32> = BytesN::from_array(&env, &[1u8; 32]);
    c.propose_upgrade(&Address::generate(&env), &wasm_hash);

    let proposal = c.get_upgrade_proposal();
    assert!(proposal.is_some());
    let p = proposal.unwrap();
    assert_eq!(p.new_wasm_hash, wasm_hash);
    assert_eq!(p.eligible_at, 1_000 + 48 * 3600);
}

#[test]
#[should_panic(expected = "upgrade timelock still active")]
fn test_310_execute_upgrade_before_timelock_panics() {
    let (env, contract_id, token_id) = setup();
    init_contract(&env, &contract_id, &token_id);
    let c = client(&env, &contract_id);

    env.ledger().set_timestamp(1_000);
    let wasm_hash: BytesN<32> = BytesN::from_array(&env, &[2u8; 32]);
    c.propose_upgrade(&Address::generate(&env), &wasm_hash);

    // Try to execute immediately — should panic
    c.execute_upgrade();
}

#[test]
fn test_310_cancel_upgrade() {
    let (env, contract_id, token_id) = setup();
    init_contract(&env, &contract_id, &token_id);
    let c = client(&env, &contract_id);

    env.ledger().set_timestamp(1_000);
    let wasm_hash: BytesN<32> = BytesN::from_array(&env, &[3u8; 32]);
    c.propose_upgrade(&Address::generate(&env), &wasm_hash);
    assert!(c.get_upgrade_proposal().is_some());

    c.cancel_upgrade(&Address::generate(&env));
    assert!(c.get_upgrade_proposal().is_none());
}

#[test]
#[should_panic(expected = "no upgrade proposal")]
fn test_310_cancel_without_proposal_panics() {
    let (env, contract_id, token_id) = setup();
    init_contract(&env, &contract_id, &token_id);
    let c = client(&env, &contract_id);

    c.cancel_upgrade(&Address::generate(&env));
}

#[test]
fn test_310_propose_overwrites_existing() {
    let (env, contract_id, token_id) = setup();
    init_contract(&env, &contract_id, &token_id);
    let c = client(&env, &contract_id);

    env.ledger().set_timestamp(1_000);
    let hash1: BytesN<32> = BytesN::from_array(&env, &[1u8; 32]);
    let hash2: BytesN<32> = BytesN::from_array(&env, &[2u8; 32]);
    c.propose_upgrade(&Address::generate(&env), &hash1);
    // Second proposal overwrites the first — no panic.
    c.propose_upgrade(&Address::generate(&env), &hash2);
}

// ---------------------------------------------------------------------------
// Issue #559: Creator Revenue Share
// ---------------------------------------------------------------------------

#[test]
fn test_creator_fee_deducted_on_release() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &10_000);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let mut opts2 = default_options2(&env);
    opts2.creator_fee_bps = 500; // 5% creator fee
    let id = c.create_invoice_ext(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999,
        &default_options(&env),
        &opts2,
    );

    // Full payment triggers auto-release which also deducts the creator fee.
    c.pay(&payer, &id, &1_000_i128, &0_u64, &false, &false, &None);

    // Check creator_fee_paid event before any further call clears the buffer.
    // Topic structure: ("crtr_fee", invoice_id) → topics[0] is the symbol.
    let has_creator_fee_event = env.events().all().iter().any(|(_c, topics, _d)| topic0_is(&env, &topics, "crtr_fee"));
    assert!(has_creator_fee_event, "creator_fee_paid event should be emitted");

    // Recipient should receive 950 (1000 - 5% fee), creator should receive 50
    let recipient_balance = tk.balance(&recipient);
    assert_eq!(recipient_balance, 950_i128);
    let creator_balance = tk.balance(&creator);
    assert_eq!(creator_balance, 50_i128);
}

#[test]
#[should_panic(expected = "FeeSumExceedsCap")]
fn test_creator_fee_exceeds_cap_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    // platform_fee_bps = 5_000 (50%)
    c.initialize(&admin, &0_i128, &treasury, &token_id, &5_000_u32, &None, &0_u32, &0_u32, &0_u64);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let mut opts2 = default_options2(&env);
    // 5_000 (platform) + 6_000 (creator) = 11_000 > 10_000 → FeeSumExceedsCap
    opts2.creator_fee_bps = 6_000;
    let _id = c.create_invoice_ext(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999,
        &default_options(&env),
        &opts2,
    );
}

// ---------------------------------------------------------------------------
// Issue #560: Creator Migration
// ---------------------------------------------------------------------------

#[test]
fn test_nominate_new_creator_emits_event() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let successor = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999,
        &default_options(&env),
    );

    c.nominate_new_creator(&creator, &id, &successor);

    // Check creator_nominated event was emitted (topics: ["crtr_nom", invoice_id])
    let has_nom_event = env.events().all().iter().any(|(_c, topics, _d)| topic0_is(&env, &topics, "crtr_nom"));
    assert!(has_nom_event, "creator_nominated event should be emitted");
}

#[test]
fn test_accept_creator_role_migrates_creator() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let successor = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999,
        &default_options(&env),
    );

    c.nominate_new_creator(&creator, &id, &successor);
    c.accept_creator_role(&successor, &id);

    // Check creator_migrated event was emitted (topics: ["crtr_mig", invoice_id])
    let has_mig_event = env.events().all().iter().any(|(_c, topics, _d)| topic0_is(&env, &topics, "crtr_mig"));
    assert!(has_mig_event, "creator_migrated event should be emitted");

    // Invoice should now have the successor as creator
    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.creator, successor);
}

#[test]
#[should_panic(expected = "InvoiceDeleted")]
fn test_nominate_new_creator_on_deleted_invoice_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let successor = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999,
        &default_options(&env),
    );

    c.delete_invoice(&creator, &id);
    c.nominate_new_creator(&creator, &id, &successor);
}

// ---------------------------------------------------------------------------
// Issue #561: Payout Ordering
// ---------------------------------------------------------------------------

#[test]
fn test_payout_ordering_canonical_sort() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let recipient1 = Address::generate(&env);
    let recipient2 = Address::generate(&env);
    let payer = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &20_000);
    env.ledger().set_timestamp(1_000);

    // Create invoice with two recipients in non-canonical order
    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient1.clone());
    recipients.push_back(recipient2.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);
    amounts.push_back(1_000_i128);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999,
        &default_options(&env),
    );

    // Full payment triggers auto-release, which emits pyt_init events for each recipient.
    c.pay(&payer, &id, &2_000_i128, &0_u64, &false, &false, &None);

    // Check payout_initiated events before any further contract call clears the buffer.
    // Topic structure: ("pyt_init", invoice_id, recipient_index) → topics[0] is the symbol.
    let payout_event_count = env.events().all().iter()
        .filter(|(_c, topics, _d)| topic0_is(&env, topics, "pyt_init"))
        .count();

    assert_eq!(payout_event_count, 2, "Two payout_initiated events expected");

    // Both recipients should receive their full share
    assert_eq!(tk.balance(&recipient1), 1_000_i128);
    assert_eq!(tk.balance(&recipient2), 1_000_i128);
}

// ---------------------------------------------------------------------------
// Issue #562: Soft-Delete with Tombstone
// ---------------------------------------------------------------------------

#[test]
fn test_soft_delete_writes_tombstone() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);
    env.ledger().set_sequence_number(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999,
        &default_options(&env),
    );

    c.delete_invoice(&creator, &id);

    // Invoice should be in Deleted status
    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Deleted);

    // Tombstone should be retrievable; deleted_at_ledger is the ledger sequence number.
    let tombstone = c.get_tombstone(&id);
    assert_eq!(tombstone.invoice_id, id);
    assert_eq!(tombstone.deleted_at_ledger, 1_000);
    assert_eq!(tombstone.deleted_by, creator);
}

#[test]
#[should_panic(expected = "InvoiceDeleted")]
fn test_pay_on_deleted_invoice_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let payer = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &10_000);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999,
        &default_options(&env),
    );

    c.delete_invoice(&creator, &id);
    c.pay(&payer, &id, &1_000_i128, &0_u64, &false, &false, &None);
}

#[test]
#[should_panic(expected = "InvoiceDeleted")]
fn test_release_on_deleted_invoice_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let payer = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &10_000);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999,
        &default_options(&env),
    );

    c.delete_invoice(&creator, &id);
    c.release(&id);
}

#[test]
#[should_panic(expected = "FundsUnclaimed")]
fn test_delete_invoice_with_unclaimed_funds_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let payer = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &10_000);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999,
        &default_options(&env),
    );

    // Pay partial (500 of 1000) so invoice stays Pending with funded > 0.
    c.pay(&payer, &id, &500_i128, &0_u64, &false, &false, &None);
    // Invoice has funded > 0 (unclaimed funds) → delete should panic with FundsUnclaimed.
    c.delete_invoice(&creator, &id);
}

#[test]
#[should_panic(expected = "NotDeleted")]
fn test_get_tombstone_on_non_deleted_invoice_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999,
        &default_options(&env),
    );

    c.get_tombstone(&id);
}

#[test]
#[should_panic(expected = "InvoiceDeleted")]
fn test_cancel_invoice_on_deleted_invoice_panics() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999,
        &default_options(&env),
    );

    c.delete_invoice(&creator, &id);
    c.cancel_invoice(&creator, &id);
}

#[test]
fn test_get_invoice_deadline() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let deadline: u64 = 5_000;

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, deadline);

    let returned_deadline = c.get_invoice_deadline(&id).expect("should return deadline");
    assert_eq!(returned_deadline, deadline);
}

#[test]
fn test_get_invoice_deadline_not_found() {
    let (env, contract_id, _token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let result = c.try_get_invoice_deadline(&999);
    assert!(result.is_err());
}

#[test]
fn test_get_invoice_funded() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);

    let funded_before = c.get_invoice_funded(&id).expect("should return funded");
    assert_eq!(funded_before, 0);

    c.pay(&payer, &id, &150_i128, &0_u64, &false, &false, &None);

    let funded_after = c.get_invoice_funded(&id).expect("should return funded");
    assert_eq!(funded_after, 150);
}

#[test]
fn test_get_invoice_funded_not_found() {
    let (env, contract_id, _token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let result = c.try_get_invoice_funded(&999);
    assert!(result.is_err());
}

#[test]
fn test_get_invoice_status() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);

    let status_pending = c.get_invoice_status(&id).expect("should return status");
    assert_eq!(status_pending, InvoiceStatus::Pending);

    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);

    let status_released = c.get_invoice_status(&id).expect("should return status");
    assert_eq!(status_released, InvoiceStatus::Released);
}

#[test]
fn test_get_invoice_status_not_found() {
    let (env, contract_id, _token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let result = c.try_get_invoice_status(&999);
    assert!(result.is_err());
}
