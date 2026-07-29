#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger, Storage},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Deploy the split contract and a mock USDC token; return (env, contract_id, token_id).
fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SplitContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();

    // Mint tokens to test accounts via the admin interface.
    let stellar_asset = StellarAssetClient::new(&env, &token_id);
    stellar_asset.mint(&token_admin, &1_000_000_000);

    (env, contract_id, token_id)
}

fn client<'a>(env: &'a Env, contract_id: &Address) -> SplitContractClient<'a> {
    SplitContractClient::new(env, contract_id)
}

fn token_client<'a>(env: &'a Env, token_id: &Address) -> TokenClient<'a> {
    TokenClient::new(env, token_id)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_invoice() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    env.ledger().set_sequence(1_000);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &2_000_u32);
    assert_eq!(id, 1);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Pending);
    assert_eq!(invoice.funded, 0);
}

#[test]
fn test_pay_and_auto_release() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let stellar_asset = StellarAssetClient::new(&env, &token_id);
    stellar_asset.mint(&payer, &500);

    env.ledger().set_sequence(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u32);

    c.pay(&payer, &id, &200_i128);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Released);

    assert_eq!(tk.balance(&recipient), 200);
}

#[test]
fn test_partial_pay_then_release() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);
    let recipient = Address::generate(&env);

    let stellar_asset = StellarAssetClient::new(&env, &token_id);
    stellar_asset.mint(&payer1, &150);
    stellar_asset.mint(&payer2, &150);

    env.ledger().set_sequence(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(300_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u32);

    c.pay(&payer1, &id, &150_i128);
    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Pending);

    c.pay(&payer2, &id, &150_i128);
    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 300);
}

#[test]
fn test_refund_after_deadline() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let stellar_asset = StellarAssetClient::new(&env, &token_id);
    stellar_asset.mint(&payer, &100);

    env.ledger().set_sequence(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &2_000_u32);

    c.pay(&payer, &id, &100_i128);

    env.ledger().set_sequence(3_000);

    c.refund(&id);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Refunded);
    assert_eq!(tk.balance(&payer), 100);
}

#[test]
#[should_panic(expected = "invoice deadline has passed")]
fn test_pay_after_deadline_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let stellar_asset = StellarAssetClient::new(&env, &token_id);
    stellar_asset.mint(&payer, &100);

    env.ledger().set_sequence(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &2_000_u32);

    env.ledger().set_sequence(3_000);
    c.pay(&payer, &id, &100_i128);
}

#[test]
#[should_panic(expected = "payment exceeds remaining balance")]
fn test_overpayment_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let stellar_asset = StellarAssetClient::new(&env, &token_id);
    stellar_asset.mint(&payer, &1_000);

    env.ledger().set_sequence(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u32);
    c.pay(&payer, &id, &200_i128);
}

#[test]
fn test_multi_recipient_release() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let r3 = Address::generate(&env);

    let stellar_asset = StellarAssetClient::new(&env, &token_id);
    stellar_asset.mint(&payer, &600);

    env.ledger().set_sequence(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(r1.clone());
    recipients.push_back(r2.clone());
    recipients.push_back(r3.clone());

    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);
    amounts.push_back(200_i128);
    amounts.push_back(300_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u32);
    c.pay(&payer, &id, &600_i128);

    assert_eq!(tk.balance(&r1), 100);
    assert_eq!(tk.balance(&r2), 200);
    assert_eq!(tk.balance(&r3), 300);
}

// ---------------------------------------------------------------------------
// Issue #514 - Audit log tests
// ---------------------------------------------------------------------------

#[test]
fn test_audit_log_records_contribution() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let stellar_asset = StellarAssetClient::new(&env, &token_id);
    stellar_asset.mint(&payer, &100);

    env.ledger().set_sequence(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &5_000_u32);
    c.pay(&payer, &id, &100_i128);

    let log = c.get_audit_log(&id);
    assert_eq!(log.len(), 1);
    let record = log.get(0).unwrap();
    assert_eq!(record.from, payer);
    assert_eq!(record.to, env.current_contract_address());
    assert_eq!(record.amount, 100_i128);
    assert_eq!(record.kind, TransferKind::Contribution);
    assert_eq!(record.ledger, 1_000_u32);
}

#[test]
fn test_audit_log_records_payout_on_release() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let stellar_asset = StellarAssetClient::new(&env, &token_id);
    stellar_asset.mint(&payer, &500);

    env.ledger().set_sequence(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u32);
    c.pay(&payer, &id, &200_i128);

    let log = c.get_audit_log(&id);
    assert_eq!(log.len(), 2);
    assert_eq!(log.get(0).unwrap().kind, TransferKind::Contribution);
    assert_eq!(log.get(1).unwrap().kind, TransferKind::Payout);
}

#[test]
fn test_audit_log_records_refund() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let stellar_asset = StellarAssetClient::new(&env, &token_id);
    stellar_asset.mint(&payer, &100);

    env.ledger().set_sequence(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &2_000_u32);
    c.pay(&payer, &id, &100_i128);

    env.ledger().set_sequence(3_000);
    c.refund(&id);

    let log = c.get_audit_log(&id);
    assert_eq!(log.len(), 2);
    assert_eq!(log.get(0).unwrap().kind, TransferKind::Contribution);
    assert_eq!(log.get(1).unwrap().kind, TransferKind::Refund);
}

#[test]
fn test_audit_log_bounded_by_max() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);
    let recipient = Address::generate(&env);

    let stellar_asset = StellarAssetClient::new(&env, &token_id);
    stellar_asset.mint(&payer1, &100);
    stellar_asset.mint(&payer2, &100);

    env.ledger().set_sequence(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u32);

    c.set_max_audit_log_entries(&env.current_contract_address(), &1);

    c.pay(&payer1, &id, &100_i128);
    c.pay(&payer2, &id, &100_i128);

    let log = c.get_audit_log(&id);
    assert_eq!(log.len(), 1);
}

// ---------------------------------------------------------------------------
// Issue #515 - Archival tests
// ---------------------------------------------------------------------------

#[test]
fn test_completed_invoice_archived() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let stellar_asset = StellarAssetClient::new(&env, &token_id);
    stellar_asset.mint(&payer, &500);

    env.ledger().set_sequence(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u32);
    c.pay(&payer, &id, &200_i128);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Released);
}

#[test]
fn test_archived_invoice_key_present() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let stellar_asset = StellarAssetClient::new(&env, &token_id);
    stellar_asset.mint(&payer, &500);

    env.ledger().set_sequence(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u32);
    c.pay(&payer, &id, &200_i128);

    // Hot key should be absent after release.
    let hot: Option<Invoice> = env
        .storage()
        .persistent()
        .get(&(symbol_short!("inv"), id));
    assert!(hot.is_none());

    // Archived key should be present.
    let archived = c.get_archived_invoice(&id);
    assert_eq!(archived.status, InvoiceStatus::Released);
}

#[test]
fn test_refund_archives_invoice() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let stellar_asset = StellarAssetClient::new(&env, &token_id);
    stellar_asset.mint(&payer, &100);

    env.ledger().set_sequence(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &2_000_u32);
    c.pay(&payer, &id, &100_i128);

    env.ledger().set_sequence(3_000);
    c.refund(&id);

    // Hot key should be absent after refund.
    let hot: Option<Invoice> = env
        .storage()
        .persistent()
        .get(&(symbol_short!("inv"), id));
    assert!(hot.is_none());

    // Archived key should be present.
    let archived = c.get_archived_invoice(&id);
    assert_eq!(archived.status, InvoiceStatus::Refunded);
}

// ---------------------------------------------------------------------------
// Issue #516 - Leaderboard tests
// ---------------------------------------------------------------------------

#[test]
fn test_leaderboard_tracks_contributors() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);
    let recipient = Address::generate(&env);

    let stellar_asset = StellarAssetClient::new(&env, &token_id);
    stellar_asset.mint(&payer1, &100);
    stellar_asset.mint(&payer2, &200);

    env.ledger().set_sequence(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u32);
    c.pay(&payer1, &id, &100_i128);
    c.pay(&payer2, &id, &200_i128);

    let leaders = c.get_top_contributors(&id, 10);
    assert_eq!(leaders.len(), 2);
    assert_eq!(leaders.get(0).unwrap().0, payer2);
    assert_eq!(leaders.get(0).unwrap().1, 200_i128);
    assert_eq!(leaders.get(1).unwrap().0, payer1);
    assert_eq!(leaders.get(1).unwrap().1, 100_i128);
}

#[test]
fn test_leaderboard_trims_below_max() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_sequence(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_000_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u32);

    c.set_max_leaderboard_size(&env.current_contract_address(), &3);

    let ids = vec![
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    ];

    let stellar_asset = StellarAssetClient::new(&env, &token_id);
    for (i, addr) in ids.iter().enumerate() {
        stellar_asset.mint(addr, &((i as i128 + 1) * 100));
        c.pay(addr, &id, &((i as i128 + 1) * 100));
    }

    let leaders = c.get_top_contributors(&id, 10);
    assert_eq!(leaders.len(), 3);
    assert_eq!(leaders.get(0).unwrap().1, 500_i128);
    assert_eq!(leaders.get(1).unwrap().1, 400_i128);
    assert_eq!(leaders.get(2).unwrap().1, 300_i128);
}

#[test]
#[should_panic(expected = "deadline must be in the future")]
fn test_create_invoice_past_deadline_ledger_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_sequence(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    c.create_invoice(&creator, &recipients, &amounts, &token_id, &(500_u32));
}