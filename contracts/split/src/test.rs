#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};
use types::InvoiceOptions;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SplitContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

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

fn default_options(env: &Env) -> InvoiceOptions {
    InvoiceOptions {
        co_creators: Vec::new(env),
        allow_early_withdrawal: false,
        bonus_pool: 0,
        bonus_max_payers: 0,
        prerequisite_id: None,
        tranches: Vec::new(env),
        insurance_pool: 0,
    }
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
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 2_000);
    assert_eq!(id, 1);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Pending);
    assert_eq!(invoice.funded, 0);
    assert!(!invoice.frozen);
}

#[test]
fn test_pay_and_auto_release() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);
    c.pay(&payer, &id, &200_i128, &0_u64);

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

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer1, &150);
    sa.mint(&payer2, &150);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 9_999);

    c.pay(&payer1, &id, &150_i128, &0_u64);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);

    c.pay(&payer2, &id, &150_i128, &0_u64);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
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

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 500, &token_id, 2_000);
    c.pay(&payer, &id, &100_i128, &0_u64);

    env.ledger().set_timestamp(3_000);
    c.refund(&id);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Refunded);
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

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 2_000);
    env.ledger().set_timestamp(3_000);
    c.pay(&payer, &id, &100_i128, &0_u64);
}

#[test]
#[should_panic(expected = "payment exceeds remaining balance")]
fn test_overpayment_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.pay(&payer, &id, &200_i128, &0_u64);
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
    c.pay(&payer, &id, &600_i128, &0_u64);

    assert_eq!(tk.balance(&r1), 100);
    assert_eq!(tk.balance(&r2), 200);
    assert_eq!(tk.balance(&r3), 300);
}

#[test]
fn test_audit_log() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);
    c.pay(&payer, &id, &200_i128, &0_u64);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);

    let log = c.get_audit_log(&id);
    assert_eq!(log.len(), 2);
    assert_eq!(log.get_unchecked(0).action, symbol_short!("pay"));
    assert_eq!(log.get_unchecked(1).action, symbol_short!("release"));
}

#[test]
fn test_audit_log_with_cancel() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.cancel_invoice(&creator, &id);

    let log = c.get_audit_log(&id);
    assert_eq!(log.len(), 1);
    assert_eq!(log.get_unchecked(0).action, symbol_short!("cancel"));
    assert_eq!(log.get_unchecked(0).actor, creator);
}

#[test]
fn test_template_save_and_create_two_invoices() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);
    let stellar_asset = StellarAssetClient::new(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    stellar_asset.mint(&payer, &400);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let name = soroban_sdk::symbol_short!("bill");
    c.save_template(&creator, &name, &recipients, &amounts, &token_id);

    let id1 = c.create_from_template(&creator, &name, &5_000_u64);
    let id2 = c.create_from_template(&creator, &name, &6_000_u64);

    assert_ne!(id1, id2);

    c.pay(&payer, &id1, &100_i128, &0_u64);
    c.pay(&payer, &id2, &100_i128, &0_u64);

    assert_eq!(c.get_invoice(&id1).status, InvoiceStatus::Released);
    assert_eq!(c.get_invoice(&id2).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 200);
}

#[test]
fn test_template_overwrite() {
    let (env, contract_id, token_id) = setup();
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

    let id = c.create_from_template(&creator, &name, &9_999_u64);
    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.recipients.get_unchecked(0), r2);
    assert_eq!(invoice.amounts.get_unchecked(0), 75_i128);
}

#[test]
fn test_cancel_with_refund() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);
    let stellar_asset = StellarAssetClient::new(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    stellar_asset.mint(&payer, &300);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 9_999);

    c.pay(&payer, &id, &150_i128, &0_u64);
    assert_eq!(tk.balance(&payer), 150);

    c.cancel_invoice(&creator, &id);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Refunded);
    assert_eq!(tk.balance(&payer), 300);
}

#[test]
#[should_panic(expected = "invoice is not pending")]
fn test_cancel_non_pending_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let stellar_asset = StellarAssetClient::new(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    stellar_asset.mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.pay(&payer, &id, &100_i128, &0_u64);
    c.cancel_invoice(&creator, &id);
}

#[test]
fn test_get_payer_total() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let stellar_asset = StellarAssetClient::new(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let other = Address::generate(&env);
    let recipient = Address::generate(&env);

    stellar_asset.mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 500, &token_id, 9_999);

    assert_eq!(c.get_payer_total(&id, &payer), 0);
    assert_eq!(c.get_payer_total(&id, &other), 0);

    c.pay(&payer, &id, &200_i128, &0_u64);
    assert_eq!(c.get_payer_total(&id, &payer), 200);

    c.pay(&payer, &id, &150_i128, &1_u64);
    assert_eq!(c.get_payer_total(&id, &payer), 350);
}

#[test]
fn test_audit_log_with_extend() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 2_000);
    c.extend_deadline(&creator, &id, &9_999_u64);

    let log = c.get_audit_log(&id);
    assert_eq!(log.len(), 1);
    assert_eq!(log.get_unchecked(0).action, symbol_short!("extend"));
    assert_eq!(log.get_unchecked(0).actor, creator);
}

#[test]
fn test_create_subscription() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);

    let id = c.create_subscription(&creator, &recipients, &amounts, &token_id, &3_u32);
    assert_eq!(id, 1);

    c.pay(&payer, &id, &200_i128, &0_u64);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);

    let second_invoice = c.get_invoice(&2);
    assert_eq!(second_invoice.status, InvoiceStatus::Pending);
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

    c.initialize(&admin);
    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);
    c.pause(&admin);

    c.pay(&payer, &id, &100_i128, &0_u64);
}

#[test]
fn test_unpause_restores_pay() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    c.initialize(&admin);
    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);

    c.pause(&admin);
    c.unpause(&admin);

    c.pay(&payer, &id, &200_i128, &0_u64);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
}

#[test]
fn test_get_invoice_works_while_paused() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    c.initialize(&admin);
    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);
    c.pause(&admin);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Pending);
}

// ---------------------------------------------------------------------------
// Transfer invoice
// ---------------------------------------------------------------------------

#[test]
fn test_transfer_invoice_new_creator_can_cancel() {
    let (env, contract_id, token_id) = setup();
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
#[should_panic(expected = "only creator can cancel")]
fn test_transfer_invoice_old_creator_cannot_cancel() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let new_creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.transfer_invoice(&id, &new_creator);

    c.cancel_invoice(&creator, &id);
}

// ---------------------------------------------------------------------------
// Bonus pool
// ---------------------------------------------------------------------------

#[test]
fn test_bonus_pool_distributed_to_first_payer() {
    let (env, contract_id, token_id) = setup();
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
            insurance_pool: 0,
        },
    );

    c.pay(&early_payer, &id, &150_i128, &0_u64);
    c.pay(&late_payer, &id, &150_i128, &0_u64);

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

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &200);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);
    c.pay(&payer, &id, &200_i128, &0_u64);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 200);
}

// ---------------------------------------------------------------------------
// Invoice groups
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "group members not fully funded")]
fn test_group_partial_fund_blocks_release() {
    let (env, contract_id, token_id) = setup();
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
    c.create_invoice_group(&ids);

    c.pay(&payer, &id1, &100_i128, &0_u64);

    c.release(&id1);
}

#[test]
fn test_group_all_funded_releases_both() {
    let (env, contract_id, token_id) = setup();
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
    c.create_invoice_group(&ids);

    c.pay(&payer, &id1, &100_i128, &0_u64);
    c.pay(&payer, &id2, &200_i128, &0_u64);

    c.release(&id1);

    assert_eq!(c.get_invoice(&id1).status, InvoiceStatus::Released);
    assert_eq!(c.get_invoice(&id2).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&r1), 100);
    assert_eq!(tk.balance(&r2), 200);
}

#[test]
fn test_non_grouped_invoice_unaffected() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let stellar_asset = StellarAssetClient::new(&env, &token_id);
    stellar_asset.mint(&payer, &300);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 9_999);
    c.pay(&payer, &id, &300_i128, &0_u64);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 300);
}

// ---------------------------------------------------------------------------
// Issue #21 — pay() nonce
// ---------------------------------------------------------------------------

#[test]
fn test_nonce_increments_per_payer_per_invoice() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 600, &token_id, 9_999);

    assert_eq!(c.get_nonce(&id, &payer), 0);

    c.pay(&payer, &id, &200_i128, &0_u64);
    assert_eq!(c.get_nonce(&id, &payer), 1);

    c.pay(&payer, &id, &200_i128, &1_u64);
    assert_eq!(c.get_nonce(&id, &payer), 2);

    c.pay(&payer, &id, &200_i128, &2_u64);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
}

#[test]
#[should_panic(expected = "invalid nonce")]
fn test_wrong_nonce_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 600, &token_id, 9_999);

    c.pay(&payer, &id, &200_i128, &0_u64);
    c.pay(&payer, &id, &200_i128, &1_u64);
    // nonce should be 2 now — submitting 1 again must panic.
    c.pay(&payer, &id, &200_i128, &1_u64);
}

#[test]
fn test_nonce_is_independent_per_invoice() {
    let (env, contract_id, token_id) = setup();
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
    c.pay(&payer, &id1, &100_i128, &0_u64);
    c.pay(&payer, &id2, &100_i128, &0_u64);

    assert_eq!(c.get_nonce(&id1, &payer), 1);
    assert_eq!(c.get_nonce(&id2, &payer), 1);
}

// ---------------------------------------------------------------------------
// Issue #22 — prerequisite invoice linking
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "prerequisite not released")]
fn test_release_blocked_by_prerequisite() {
    let (env, contract_id, token_id) = setup();
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
            insurance_pool: 0,
        },
    );

    // Fund B fully but don't touch A.
    c.pay(&payer, &id_b, &200_i128, &0_u64);

    // release() on B should panic because A is still Pending.
    c.release(&id_b);
}

#[test]
fn test_release_succeeds_after_prerequisite_released() {
    let (env, contract_id, token_id) = setup();
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
            insurance_pool: 0,
        },
    );

    // Release A (auto-releases on full funding).
    c.pay(&payer, &id_a, &100_i128, &0_u64);
    assert_eq!(c.get_invoice(&id_a).status, InvoiceStatus::Released);

    // Fund B fully (stays pending because it has a prerequisite).
    c.pay(&payer, &id_b, &200_i128, &0_u64);
    assert_eq!(c.get_invoice(&id_b).status, InvoiceStatus::Pending);

    // Now release B — prerequisite is satisfied.
    c.release(&id_b);
    assert_eq!(c.get_invoice(&id_b).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&r2), 200);
}

#[test]
fn test_no_prerequisite_behaves_like_normal() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &200);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);
    c.pay(&payer, &id, &200_i128, &0_u64);

    // Auto-releases because no prerequisite.
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
}

// ---------------------------------------------------------------------------
// Issue #23 — graduated release tranches
// ---------------------------------------------------------------------------

#[test]
fn test_tranches_partial_then_full_release() {
    let (env, contract_id, token_id) = setup();
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
            insurance_pool: 0,
        },
    );

    // Fund fully — no auto-release for tranche invoices.
    c.pay(&payer, &id, &1_000_i128, &0_u64);
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
    let (env, contract_id, token_id) = setup();
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
            insurance_pool: 0,
        },
    );

    c.pay(&payer, &id, &500_i128, &0_u64);
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
    let (env, contract_id, token_id) = setup();
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

    c.pay(&payer, &id1, &100_i128, &0_u64);
    assert_eq!(c.get_reputation(&payer), 1);

    c.pay(&payer, &id2, &100_i128, &0_u64);
    assert_eq!(c.get_reputation(&payer), 2);

    c.pay(&payer, &id3, &100_i128, &0_u64);
    assert_eq!(c.get_reputation(&payer), 3);
}

#[test]
fn test_reputation_is_per_address() {
    let (env, contract_id, token_id) = setup();
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

    c.pay(&payer_a, &id, &100_i128, &0_u64);
    c.pay(&payer_a, &id, &100_i128, &1_u64);
    c.pay(&payer_b, &id, &100_i128, &0_u64);
    c.pay(&payer_b, &id, &100_i128, &1_u64);

    // payer_a paid twice, payer_b paid twice.
    assert_eq!(c.get_reputation(&payer_a), 2);
    assert_eq!(c.get_reputation(&payer_b), 2);

    // Unrelated address has zero reputation.
    let other = Address::generate(&env);
    assert_eq!(c.get_reputation(&other), 0);
}

// ---------------------------------------------------------------------------
// Issue #40 — add_recipient
// ---------------------------------------------------------------------------

#[test]
fn test_add_recipient_before_payment() {
    let (env, contract_id, token_id) = setup();
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
}

#[test]
fn test_add_recipient_and_pay_full() {
    let (env, contract_id, token_id) = setup();
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

    c.pay(&payer, &id, &300_i128, &0_u64);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&r1), 100);
    assert_eq!(tk.balance(&r2), 200);
}

#[test]
#[should_panic(expected = "cannot add recipient after payment received")]
fn test_add_recipient_after_payment_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &r1, 200, &token_id, 9_999);
    c.pay(&payer, &id, &100_i128, &0_u64);

    c.add_recipient(&creator, &id, &r2, &50_i128);
}

#[test]
#[should_panic(expected = "only creator can add recipients")]
fn test_add_recipient_non_creator_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let impostor = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &r1, 100, &token_id, 9_999);
    c.add_recipient(&impostor, &id, &r2, &50_i128);
}

#[test]
fn test_add_recipient_and_pay_partial_then_release() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &600);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &r1, 200, &token_id, 9_999);
    c.add_recipient(&creator, &id, &r2, &300_i128);

    c.pay(&payer, &id, &500_i128, &0_u64);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&r1), 200);
    assert_eq!(tk.balance(&r2), 300);
}

#[test]
fn test_add_recipient_with_audit_log() {
    let (env, contract_id, token_id) = setup();
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

// ---------------------------------------------------------------------------
// Insurance pool
// ---------------------------------------------------------------------------

#[test]
fn test_insurance_pool_returned_on_refund() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer, &500);
    sa.mint(&creator, &200);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);

    let creator_bal_before = tk.balance(&creator);
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
            insurance_pool: 100,
        },
    );

    // Insurance pool was deposited.
    assert_eq!(tk.balance(&creator), creator_bal_before - 100);

    c.pay(&payer, &id, &300_i128, &0_u64);

    // Deadline passes, refund.
    env.ledger().set_timestamp(10_001);
    c.refund(&id);

    // Payer got their money back.
    assert_eq!(tk.balance(&payer), 500);
    // Creator got insurance pool back.
    assert_eq!(tk.balance(&creator), creator_bal_before);
}

#[test]
fn test_insurance_pool_covers_shortfall() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let drain_target = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer, &500);
    sa.mint(&creator, &200);
    sa.mint(&drain_target, &1_000);
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
            insurance_pool: 200,
        },
    );

    c.pay(&payer, &id, &300_i128, &0_u64);

    // Simulate a bug: drain 100 tokens from the contract.
    env.as_contract(&contract_id, || {
        tk.transfer(&env.current_contract_address(), &drain_target, &100_i128);
    });

    // Deadline passes, refund.
    let creator_bal_before = tk.balance(&creator);
    env.ledger().set_timestamp(10_001);
    c.refund(&id);

    // Payer got full refund despite shortfall.
    assert_eq!(tk.balance(&payer), 500);
    // Creator got back what remains after payer refund (contract had 500 - 100 drain = 400,
    // paid 300 to payer, remaining 100 returned to creator = 100 back).
    assert_eq!(tk.balance(&creator), creator_bal_before + 100);
}

#[test]
#[should_panic(expected = "insurance pool insufficient to cover shortfall")]
fn test_insurance_pool_insufficient_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let drain_target = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer, &500);
    sa.mint(&creator, &100);
    sa.mint(&drain_target, &1_000);
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
            insurance_pool: 50,
        },
    );

    c.pay(&payer, &id, &300_i128, &0_u64);

    // Drain 250 — contract has 50, needs 300 for refund.
    env.as_contract(&contract_id, || {
        tk.transfer(&env.current_contract_address(), &drain_target, &250_i128);
    });

    env.ledger().set_timestamp(10_001);
    c.refund(&id);
}

#[test]
fn test_insurance_pool_returned_on_cancel() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&creator, &100);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);

    let creator_bal_before = tk.balance(&creator);
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
            insurance_pool: 100,
        },
    );

    assert_eq!(tk.balance(&creator), creator_bal_before - 100);

    c.cancel_invoice(&creator, &id);

    // Creator got insurance pool back.
    assert_eq!(tk.balance(&creator), creator_bal_before);
}

#[test]
fn test_insurance_pool_returned_on_release() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer, &500);
    sa.mint(&creator, &100);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);

    let creator_bal_before = tk.balance(&creator);
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
            insurance_pool: 100,
        },
    );

    assert_eq!(tk.balance(&creator), creator_bal_before - 100);

    c.pay(&payer, &id, &500_i128, &0_u64);

    // Release should return insurance pool to creator.
    assert_eq!(tk.balance(&creator), creator_bal_before);
    assert_eq!(tk.balance(&recipient), 500);
}

// ---------------------------------------------------------------------------
// Scheduled payments — schedule_pay / execute_scheduled_pay
// ---------------------------------------------------------------------------

#[test]
fn test_schedule_and_execute_pay() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 9_999);

    // Schedule a payment for timestamp 5_000.
    c.schedule_pay(&payer, &id, &300_i128, &0_u64, &5_000_u64);

    // Before the scheduled time, invoice is still pending and no transfer happened.
    assert_eq!(tk.balance(&payer), 500);
    assert_eq!(c.get_invoice(&id).funded, 0);

    // Advance to execute time.
    env.ledger().set_timestamp(5_000);
    c.execute_scheduled_pay(&payer, &id);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 300);
}

#[test]
#[should_panic(expected = "scheduled payment time not yet reached")]
fn test_execute_scheduled_pay_before_time_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 9_999);

    c.schedule_pay(&payer, &id, &300_i128, &0_u64, &5_000_u64);

    // Try to execute before the scheduled time.
    env.ledger().set_timestamp(4_999);
    c.execute_scheduled_pay(&payer, &id);
}

#[test]
fn test_schedule_pay_consumes_nonce() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 9_999);

    // Nonce starts at 0.
    assert_eq!(c.get_nonce(&id, &payer), 0);

    // Schedule consumes nonce 0.
    c.schedule_pay(&payer, &id, &200_i128, &0_u64, &5_000_u64);
    assert_eq!(c.get_nonce(&id, &payer), 1);

    // A direct pay must use nonce 1 now.
    c.pay(&payer, &id, &100_i128, &1_u64);
    assert_eq!(c.get_nonce(&id, &payer), 2);
}

#[test]
fn test_schedule_pay_then_multi_step() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 500, &token_id, 9_999);

    // Pay 100 now.
    c.pay(&payer, &id, &100_i128, &0_u64);
    assert_eq!(c.get_invoice(&id).funded, 100);

    // Schedule 300 for later.
    c.schedule_pay(&payer, &id, &300_i128, &1_u64, &5_000_u64);

    // Pay remaining 100 now (nonce=2 after scheduling consumed 1).
    c.pay(&payer, &id, &100_i128, &2_u64);
    assert_eq!(c.get_invoice(&id).funded, 200);

    // Execute the scheduled payment.
    env.ledger().set_timestamp(5_000);
    c.execute_scheduled_pay(&payer, &id);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(c.get_invoice(&id).funded, 500);
    assert_eq!(tk.balance(&recipient), 500);
}

#[test]
#[should_panic(expected = "no scheduled payment found")]
fn test_execute_scheduled_pay_none_panics() {
    let (env, contract_id, _token_id) = setup();
    let c = client(&env, &contract_id);

    let payer = Address::generate(&env);
    env.ledger().set_timestamp(1_000);

    c.execute_scheduled_pay(&payer, &1_u64);
}
