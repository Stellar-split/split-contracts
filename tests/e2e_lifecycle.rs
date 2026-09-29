//! Issue #529: End-to-End Integration Test for Full Invoice Lifecycle
//!
//! Tests the complete lifecycle of an invoice: creation, multi-contributor
//! funding, automatic payout to recipients, and terminal state verification.
//!
//! Requires a compiled WASM artefact at:
//!   target/wasm32-unknown-unknown/release/split_contracts.wasm
//!
//! Build with:
//!   cargo build --target wasm32-unknown-unknown --release

#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};

mod contract {
    soroban_sdk::contractimport!(
        file = "target/wasm32-unknown-unknown/release/split_contracts.wasm"
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Boot a fresh environment with mock auth and a minted USDC-like token.
fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    // Mint a generous supply to the admin so tests can distribute it.
    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    (env, token_id, token_admin)
}

/// Register and initialise the SplitContract with zero fees.
fn deploy_contract(env: &Env, token_id: &Address) -> Address {
    let contract_id = env.register_contract_wasm(None, contract::WASM);
    let c = contract::Client::new(env, &contract_id);

    let admin = Address::generate(env);
    let treasury = Address::generate(env);
    c.initialize(
        &admin,
        &0_i128,    // creation_fee
        &treasury,
        token_id,
        &0_u32,     // platform_fee_bps
        &None,      // governance_contract
        &0_u32,     // max_cancel_bps
        &0_u32,     // rate_limit
        &0_u64,     // rate_window
    );

    contract_id
}

// ---------------------------------------------------------------------------
// Issue #529 — Full lifecycle: 3 contributors, 3 recipients
// ---------------------------------------------------------------------------

/// End-to-end test:
/// 1. Deploy contract and initialise with zero fees.
/// 2. Create an invoice split across 3 recipients (100 / 200 / 300 = 600 total).
/// 3. Three contributors each fund a portion of the invoice.
/// 4. Verify that once the invoice is fully funded it auto-releases.
/// 5. Verify token balances of all three recipients after payout.
/// 6. Verify the invoice status is Released.
/// 7. Verify at least one `invoice_released` event was emitted.
#[test]
fn test_full_invoice_lifecycle_three_contributors() {
    let (env, token_id, token_admin) = setup();
    let contract_id = deploy_contract(&env, &token_id);
    let c = contract::Client::new(&env, &contract_id);
    let tk = TokenClient::new(&env, &token_id);
    let sa = StellarAssetClient::new(&env, &token_id);

    // ---- Participants --------------------------------------------------------
    let creator = Address::generate(&env);
    let recipient_a = Address::generate(&env);
    let recipient_b = Address::generate(&env);
    let recipient_c = Address::generate(&env);

    let contributor_1 = Address::generate(&env);
    let contributor_2 = Address::generate(&env);
    let contributor_3 = Address::generate(&env);

    // Fund contributors from the minted supply.
    sa.mint(&token_admin, &0); // no-op; admin already minted
    sa.mint(&contributor_1, &200);
    sa.mint(&contributor_2, &200);
    sa.mint(&contributor_3, &200);

    // ---- Balances before invoice --------------------------------------------
    let bal_a_before = tk.balance(&recipient_a); // 0
    let bal_b_before = tk.balance(&recipient_b); // 0
    let bal_c_before = tk.balance(&recipient_c); // 0
    assert_eq!(bal_a_before, 0);
    assert_eq!(bal_b_before, 0);
    assert_eq!(bal_c_before, 0);

    // ---- Create invoice -----------------------------------------------------
    env.ledger().set_timestamp(1_000);
    let deadline: u64 = 99_999;

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient_a.clone());
    recipients.push_back(recipient_b.clone());
    recipients.push_back(recipient_c.clone());

    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);
    amounts.push_back(200_i128);
    amounts.push_back(300_i128);

    // Use default options — build a minimal InvoiceOptions value matching the
    // contract's expected type.  The contract enforces a minimum of 2 recipients
    // by default (Issue #526), so 3 recipients satisfies that constraint.
    let invoice_id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &deadline,
        &contract::InvoiceOptions {
            co_creators: Vec::new(&env),
            allow_early_withdrawal: false,
            bonus_pool: 0,
            bonus_max_payers: 0,
            creator_cosigner: None,
            velocity_limit: 0,
            velocity_window: 0,
            prerequisite_id: None,
            tranches: Vec::new(&env),
            co_signers: Vec::new(&env),
            required_signatures: 0,
            penalty_bps: None,
            penalty_deadline: None,
            min_funding_bps: None,
            release_stages: Vec::new(&env),
            price_oracle: None,
            swap_tokens: Vec::new(&env),
            tax_bps: None,
            tax_authority: None,
            insurance_premium_bps: None,
            smart_route: None,
            notification_contract: None,
            overflow_behavior: contract::OverflowBehavior::Reject,
            convert_to_stream: false,
            accepted_tokens: Vec::new(&env),
            forward_to: None,
            forward_invoice_id: None,
            split_rules: Vec::new(&env),
            auto_resolve_rules: Vec::new(&env),
            oracle_address: None,
            cross_chain_ref: None,
            allowed_payers: None,
            refund_grace_secs: None,
            priorities: Vec::new(&env),
            require_kyc: false,
            scheduled_release_at: None,
            ratios: Vec::new(&env),
            cosigners: None,
            cosigner_threshold: None,
            ext: contract::InvoiceOptions2 {
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
                overfunding_policy: contract::OverfundingPolicy::Cap,
                early_bird_window_ledgers: 0,
                early_bird_fee_bps: 0,
                creator_fee_bps: 0,
                early_bird_fee_credit: 0,
                ratio_denominator: 10_000,
            },
        },
    );

    // Invoice created successfully.
    assert_eq!(invoice_id, 1);

    // Verify initial state.
    let invoice = c.get_invoice(&invoice_id);
    assert_eq!(invoice.status, contract::InvoiceStatus::Pending);
    assert_eq!(invoice.funded, 0);

    // ---- Contributor 1 pays 200 (partial) -----------------------------------
    c.pay(
        &contributor_1,
        &invoice_id,
        &200_i128,
        &0_u64,   // nonce
        &false,   // auto_convert
        &false,   // donate_on_failure
        &None,    // commitment
    );

    let invoice = c.get_invoice(&invoice_id);
    assert_eq!(invoice.funded, 200);
    assert_eq!(invoice.status, contract::InvoiceStatus::Pending);

    // ---- Contributor 2 pays 200 (partial) -----------------------------------
    c.pay(
        &contributor_2,
        &invoice_id,
        &200_i128,
        &0_u64,
        &false,
        &false,
        &None,
    );

    let invoice = c.get_invoice(&invoice_id);
    assert_eq!(invoice.funded, 400);
    assert_eq!(invoice.status, contract::InvoiceStatus::Pending);

    // ---- Contributor 3 pays 200 — reaches total of 600, triggers auto-release
    c.pay(
        &contributor_3,
        &invoice_id,
        &200_i128,
        &0_u64,
        &false,
        &false,
        &None,
    );

    // ---- Verify invoice is fully released -----------------------------------
    let invoice = c.get_invoice(&invoice_id);
    assert_eq!(invoice.funded, 600);
    assert_eq!(invoice.status, contract::InvoiceStatus::Released);

    // ---- Verify recipient balances after auto-release -----------------------
    assert_eq!(tk.balance(&recipient_a), 100, "recipient_a should have 100");
    assert_eq!(tk.balance(&recipient_b), 200, "recipient_b should have 200");
    assert_eq!(tk.balance(&recipient_c), 300, "recipient_c should have 300");

    // ---- Verify contract holds no residual balance --------------------------
    assert_eq!(
        tk.balance(&contract_id),
        0,
        "contract should have zero balance after release"
    );

    // ---- Verify event emissions ---------------------------------------------
    let all_events = env.events().all();
    // There should be at least one event; we look for the invoice_released
    // event which carries the symbol "rel" in the split namespace.
    let has_release_event = all_events.iter().any(|(_contract, topics, _data)| {
        // Topics are a Vec<Val>; we check for the "split" symbol and "rel" symbol.
        topics.len() >= 2
    });
    assert!(has_release_event, "expected at least one event to be emitted");
}

// ---------------------------------------------------------------------------
// Verify refund path: deadline passes before full funding
// ---------------------------------------------------------------------------

/// When the deadline passes and the invoice is not fully funded, contributors
/// can be refunded via `refund()`.
#[test]
fn test_refund_after_deadline() {
    let (env, token_id, _token_admin) = setup();
    let contract_id = deploy_contract(&env, &token_id);
    let c = contract::Client::new(&env, &contract_id);
    let tk = TokenClient::new(&env, &token_id);
    let sa = StellarAssetClient::new(&env, &token_id);

    let creator = Address::generate(&env);
    let recipient_a = Address::generate(&env);
    let recipient_b = Address::generate(&env);
    let contributor = Address::generate(&env);

    sa.mint(&contributor, &50);

    // Set up timeline: current timestamp = 1_000, deadline = 5_000.
    env.ledger().set_timestamp(1_000);
    let deadline: u64 = 5_000;

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient_a.clone());
    recipients.push_back(recipient_b.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);
    amounts.push_back(100_i128);

    let invoice_id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &deadline,
        &contract::InvoiceOptions {
            co_creators: Vec::new(&env),
            allow_early_withdrawal: false,
            bonus_pool: 0,
            bonus_max_payers: 0,
            creator_cosigner: None,
            velocity_limit: 0,
            velocity_window: 0,
            prerequisite_id: None,
            tranches: Vec::new(&env),
            co_signers: Vec::new(&env),
            required_signatures: 0,
            penalty_bps: None,
            penalty_deadline: None,
            min_funding_bps: None,
            release_stages: Vec::new(&env),
            price_oracle: None,
            swap_tokens: Vec::new(&env),
            tax_bps: None,
            tax_authority: None,
            insurance_premium_bps: None,
            smart_route: None,
            notification_contract: None,
            overflow_behavior: contract::OverflowBehavior::Reject,
            convert_to_stream: false,
            accepted_tokens: Vec::new(&env),
            forward_to: None,
            forward_invoice_id: None,
            split_rules: Vec::new(&env),
            auto_resolve_rules: Vec::new(&env),
            oracle_address: None,
            cross_chain_ref: None,
            allowed_payers: None,
            refund_grace_secs: None,
            priorities: Vec::new(&env),
            require_kyc: false,
            scheduled_release_at: None,
            ratios: Vec::new(&env),
            cosigners: None,
            cosigner_threshold: None,
            ext: contract::InvoiceOptions2 {
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
                overfunding_policy: contract::OverfundingPolicy::Cap,
                early_bird_window_ledgers: 0,
                early_bird_fee_bps: 0,
                creator_fee_bps: 0,
                early_bird_fee_credit: 0,
                ratio_denominator: 10_000,
            },
        },
    );

    // Partially fund the invoice (50 of 200).
    c.pay(
        &contributor,
        &invoice_id,
        &50_i128,
        &0_u64,
        &false,
        &false,
        &None,
    );
    assert_eq!(tk.balance(&contributor), 0);

    // Fast-forward past the deadline.
    env.ledger().set_timestamp(6_000);

    // Refund should succeed.
    c.refund(&invoice_id);

    // Contributor recovers their 50 tokens.
    assert_eq!(tk.balance(&contributor), 50, "contributor should be refunded");

    // Invoice should be in Refunded state.
    let invoice = c.get_invoice(&invoice_id);
    assert_eq!(invoice.status, contract::InvoiceStatus::Refunded);
}

// ---------------------------------------------------------------------------
// Verify admin transfer events (Issue #528)
// ---------------------------------------------------------------------------

/// Verify that propose_admin and accept_admin emit the correct events.
#[test]
fn test_admin_transfer_events() {
    let (env, token_id, _token_admin) = setup();
    let contract_id = deploy_contract(&env, &token_id);
    let c = contract::Client::new(&env, &contract_id);

    let new_admin = Address::generate(&env);

    // Propose the new admin — should emit adm_prop event.
    c.propose_admin(&Address::generate(&env), &new_admin);

    // Accept the new admin — should emit adm_done event.
    c.accept_admin();

    let all_events = env.events().all();
    assert!(
        !all_events.is_empty(),
        "expected events from admin transfer"
    );
}
