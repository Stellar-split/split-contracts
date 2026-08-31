#![cfg(test)]

extern crate std;

use crate::errors::Error;
use crate::types::EscrowStatus;
use crate::InvoiceEscrowContract;
use crate::InvoiceEscrowContractClient;
use soroban_sdk::testutils::{Address as _, Events, Ledger, LedgerInfo};
use soroban_sdk::{symbol_short, token, Address, Env, IntoVal};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Deploy the invoice-escrow contract and return (env, contract_id).
fn setup() -> (Env, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, InvoiceEscrowContract);
    (env, contract_id)
}

/// Deploy a minimal token (soroban test token) and mint `amount` to `to`.
fn create_token(env: &Env, admin: &Address) -> Address {
    let token_id = env.register_stellar_asset_contract_v2(admin.clone());
    token_id.address()
}

fn mint(env: &Env, token: &Address, admin: &Address, to: &Address, amount: i128) {
    let client = token::StellarAssetClient::new(env, token);
    client.mint(to, &amount);
    let _ = admin; // kept for clarity; mock_all_auths covers the mint auth
}

// ---------------------------------------------------------------------------
// initialize
// ---------------------------------------------------------------------------

#[test]
fn test_initialize_success() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    client.initialize(&admin);
    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_initialize_twice_fails() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    client.initialize(&admin);
    let result = client.try_initialize(&admin);
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

// ---------------------------------------------------------------------------
// transfer_admin — proposal step
// ---------------------------------------------------------------------------

#[test]
fn test_transfer_admin_sets_pending() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);

    client.initialize(&admin);
    client.transfer_admin(&new_admin);

    // Pending admin should be set; current admin unchanged.
    assert_eq!(client.get_pending_admin(), Some(new_admin.clone()));
    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_transfer_admin_not_initialized() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let new_admin = Address::generate(&env);

    let result = client.try_transfer_admin(&new_admin);
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_transfer_admin_replaces_pending() {
    // Calling transfer_admin twice replaces the first pending address.
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let first = Address::generate(&env);
    let second = Address::generate(&env);

    client.initialize(&admin);
    client.transfer_admin(&first);
    client.transfer_admin(&second);

    assert_eq!(client.get_pending_admin(), Some(second));
}

#[test]
fn test_transfer_admin_emits_event() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);

    client.initialize(&admin);
    client.transfer_admin(&new_admin);

    let events = env.events().all();
    // Find adm_prop event
    let found = events.iter().any(|(_, topics, _)| {
        topics
            == (
                symbol_short!("escrow"),
                symbol_short!("adm_prop"),
            )
                .into_val(&env)
    });
    assert!(found, "adm_prop event not emitted");
}

// ---------------------------------------------------------------------------
// accept_admin
// ---------------------------------------------------------------------------

#[test]
fn test_accept_admin_transfers_authority() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);

    client.initialize(&admin);
    client.transfer_admin(&new_admin);
    client.accept_admin();

    assert_eq!(client.get_admin(), new_admin);
    assert_eq!(client.get_pending_admin(), None);
}

#[test]
fn test_accept_admin_no_pending_fails() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    client.initialize(&admin);
    let result = client.try_accept_admin();
    assert_eq!(result, Err(Ok(Error::NoPendingAdmin)));
}

#[test]
fn test_accept_admin_emits_event() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);

    client.initialize(&admin);
    client.transfer_admin(&new_admin);
    client.accept_admin();

    let events = env.events().all();
    let found = events.iter().any(|(_, topics, _)| {
        topics
            == (
                symbol_short!("escrow"),
                symbol_short!("adm_acpt"),
            )
                .into_val(&env)
    });
    assert!(found, "adm_acpt event not emitted");
}

// ---------------------------------------------------------------------------
// cancel_transfer
// ---------------------------------------------------------------------------

#[test]
fn test_cancel_transfer_clears_pending() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);

    client.initialize(&admin);
    client.transfer_admin(&new_admin);
    client.cancel_transfer();

    assert_eq!(client.get_pending_admin(), None);
    // Admin unchanged after cancel.
    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_cancel_transfer_no_pending_fails() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    client.initialize(&admin);
    let result = client.try_cancel_transfer();
    assert_eq!(result, Err(Ok(Error::NoPendingAdmin)));
}

#[test]
fn test_cancel_transfer_emits_event() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);

    client.initialize(&admin);
    client.transfer_admin(&new_admin);
    client.cancel_transfer();

    let events = env.events().all();
    let found = events.iter().any(|(_, topics, _)| {
        topics
            == (
                symbol_short!("escrow"),
                symbol_short!("adm_cncl"),
            )
                .into_val(&env)
    });
    assert!(found, "adm_cncl event not emitted");
}

// ---------------------------------------------------------------------------
// Full admin transfer round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_full_admin_transfer_round_trip() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    let third_admin = Address::generate(&env);

    client.initialize(&admin);

    // First transfer: propose → accept
    client.transfer_admin(&new_admin);
    assert_eq!(client.get_pending_admin(), Some(new_admin.clone()));
    client.accept_admin();
    assert_eq!(client.get_admin(), new_admin);

    // Second transfer: propose → cancel → re-propose different address → accept
    client.transfer_admin(&admin); // propose old admin back
    client.cancel_transfer();      // cancel it
    assert_eq!(client.get_pending_admin(), None);

    client.transfer_admin(&third_admin);
    client.accept_admin();
    assert_eq!(client.get_admin(), third_admin);
}

// ---------------------------------------------------------------------------
// get_admin — before initialize
// ---------------------------------------------------------------------------

#[test]
fn test_get_admin_not_initialized() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);

    let result = client.try_get_admin();
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

// ---------------------------------------------------------------------------
// create_invoice
// ---------------------------------------------------------------------------

#[test]
fn test_create_invoice_success() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token(&env, &token_admin);

    client.initialize(&admin);
    let deadline = env.ledger().timestamp() + 10_000;
    let id = client.create_invoice(&creator, &token, &1_000_000, &deadline);
    assert_eq!(id, 0);

    let invoice = client.get_invoice(&id);
    assert_eq!(invoice.total_amount, 1_000_000);
    assert_eq!(invoice.funded_amount, 0);
    assert_eq!(invoice.status, EscrowStatus::Pending);
}

#[test]
fn test_create_invoice_invalid_amount() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token(&env, &token_admin);

    client.initialize(&admin);
    let deadline = env.ledger().timestamp() + 10_000;
    let result = client.try_create_invoice(&creator, &token, &0, &deadline);
    assert_eq!(result, Err(Ok(Error::InvalidTotalAmount)));
}

// ---------------------------------------------------------------------------
// deposit
// ---------------------------------------------------------------------------

#[test]
fn test_deposit_partial_and_full_auto_release() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token(&env, &token_admin);

    mint(&env, &token, &token_admin, &payer, 2_000_000);

    client.initialize(&admin);
    let deadline = env.ledger().timestamp() + 10_000;
    let id = client.create_invoice(&creator, &token, &1_000_000, &deadline);

    // Partial deposit
    client.deposit(&payer, &id, &600_000);
    let invoice = client.get_invoice(&id);
    assert_eq!(invoice.status, EscrowStatus::Active);
    assert_eq!(invoice.funded_amount, 600_000);

    // Complete deposit — triggers auto-release
    client.deposit(&payer, &id, &400_000);
    let invoice = client.get_invoice(&id);
    assert_eq!(invoice.status, EscrowStatus::Released);
}

#[test]
fn test_deposit_over_funded_fails() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token(&env, &token_admin);

    mint(&env, &token, &token_admin, &payer, 2_000_000);

    client.initialize(&admin);
    let deadline = env.ledger().timestamp() + 10_000;
    let id = client.create_invoice(&creator, &token, &1_000_000, &deadline);

    let result = client.try_deposit(&payer, &id, &1_500_000);
    assert_eq!(result, Err(Ok(Error::OverFunded)));
}

#[test]
fn test_deposit_after_deadline_fails() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token(&env, &token_admin);

    mint(&env, &token, &token_admin, &payer, 2_000_000);

    client.initialize(&admin);
    // Deadline 0 — create invoice, then advance ledger past deadline before depositing.
    let id = client.create_invoice(&creator, &token, &1_000_000, &0u64);

    env.ledger().set(LedgerInfo {
        timestamp: 1,
        protocol_version: 22,
        sequence_number: 1,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 10,
        min_persistent_entry_ttl: 10,
        max_entry_ttl: 3_110_400,
    });

    let result = client.try_deposit(&payer, &id, &500_000);
    assert_eq!(result, Err(Ok(Error::DeadlinePassed)));
}

// ---------------------------------------------------------------------------
// refund
// ---------------------------------------------------------------------------

#[test]
fn test_refund_after_deadline() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token(&env, &token_admin);

    mint(&env, &token, &token_admin, &payer, 2_000_000);

    client.initialize(&admin);
    // Deadline 1 second in the future
    let id = client.create_invoice(&creator, &token, &1_000_000, &1u64);

    // Deposit partial
    client.deposit(&payer, &id, &500_000);

    // Advance ledger time past deadline
    env.ledger().set(LedgerInfo {
        timestamp: 1_000_000,
        protocol_version: 22,
        sequence_number: 100,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 10,
        min_persistent_entry_ttl: 10,
        max_entry_ttl: 3_110_400,
    });

    let mut payers = soroban_sdk::Vec::new(&env);
    payers.push_back(payer.clone());
    client.refund(&id, &payers);

    let invoice = client.get_invoice(&id);
    assert_eq!(invoice.status, EscrowStatus::Refunded);

    // Payer should get funds back
    let token_client = token::Client::new(&env, &token);
    assert_eq!(token_client.balance(&payer), 2_000_000);
}

#[test]
fn test_refund_before_deadline_fails() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token(&env, &token_admin);

    mint(&env, &token, &token_admin, &payer, 2_000_000);

    client.initialize(&admin);
    let deadline = env.ledger().timestamp() + 100_000;
    let id = client.create_invoice(&creator, &token, &1_000_000, &deadline);
    client.deposit(&payer, &id, &500_000);

    let mut payers = soroban_sdk::Vec::new(&env);
    payers.push_back(payer);
    let result = client.try_refund(&id, &payers);
    assert_eq!(result, Err(Ok(Error::DeadlineNotPassed)));
}

// ---------------------------------------------------------------------------
// cancel_invoice
// ---------------------------------------------------------------------------

#[test]
fn test_cancel_invoice_by_admin() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token(&env, &token_admin);

    client.initialize(&admin);
    let deadline = env.ledger().timestamp() + 10_000;
    let id = client.create_invoice(&creator, &token, &1_000_000, &deadline);

    client.cancel_invoice(&id);
    let invoice = client.get_invoice(&id);
    assert_eq!(invoice.status, EscrowStatus::Cancelled);
}

// ---------------------------------------------------------------------------
// Payer blacklist tests
// ---------------------------------------------------------------------------

use soroban_sdk::BytesN;

fn bytesn32(env: &Env, data: &[u8; 32]) -> BytesN<32> {
    BytesN::from_array(env, data)
}

#[test]
fn test_blacklist_payer_success() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let payer = Address::generate(&env);

    client.initialize(&admin);

    let reason = bytesn32(&env, &[1u8; 32]);
    client.blacklist_payer(&admin, &payer, &reason);

    let entry = client.get_blacklist_entry(&payer).expect("blacklist entry not found");
    assert!(!entry.finalised);
    assert!(entry.appeal_hash.is_none());
}

#[test]
fn test_blacklist_blocks_finalised_payer_deposit() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token(&env, &token_admin);

    mint(&env, &token, &token_admin, &payer, 2_000_000);
    client.initialize(&admin);

    // Blacklist and finalise with uphold.
    let reason = bytesn32(&env, &[1u8; 32]);
    client.blacklist_payer(&admin, &payer, &reason);
    client.finalise_blacklist(&admin, &payer, &true);

    // Create invoice and try to deposit — must fail.
    let deadline = env.ledger().timestamp() + 10_000;
    let id = client.create_invoice(&creator, &token, &1_000_000, &deadline);
    let result = client.try_deposit(&payer, &id, &500_000);
    assert_eq!(result, Err(Ok(Error::PayerBlacklisted)));
}

#[test]
fn test_blacklist_blocks_finalised_payer_pay_invoice() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token(&env, &token_admin);

    mint(&env, &token, &token_admin, &payer, 2_000_000);
    client.initialize(&admin);

    // Blacklist and finalise with uphold.
    let reason = bytesn32(&env, &[1u8; 32]);
    client.blacklist_payer(&admin, &payer, &reason);
    client.finalise_blacklist(&admin, &payer, &true);

    let deadline = env.ledger().timestamp() + 10_000;
    let id = client.create_invoice(&creator, &token, &1_000_000, &deadline);
    let result = client.try_pay_invoice(&payer, &id, &500_000);
    assert_eq!(result, Err(Ok(Error::PayerBlacklisted)));
}

#[test]
fn test_appeal_then_reinstate_allows_deposit() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token(&env, &token_admin);

    mint(&env, &token, &token_admin, &payer, 2_000_000);
    client.initialize(&admin);

    // Blacklist the payer.
    let reason = bytesn32(&env, &[2u8; 32]);
    client.blacklist_payer(&admin, &payer, &reason);

    // Payer submits appeal.
    let appeal = bytesn32(&env, &[3u8; 32]);
    client.submit_appeal(&payer, &0u64, &appeal);

    let entry = client.get_blacklist_entry(&payer).expect("entry must exist");
    assert!(entry.appeal_hash.is_some());

    // Admin reinstates (uphold: false).
    client.finalise_blacklist(&admin, &payer, &false);

    // Payer should now be able to deposit.
    let deadline = env.ledger().timestamp() + 10_000;
    let id = client.create_invoice(&creator, &token, &1_000_000, &deadline);
    client.deposit(&payer, &id, &500_000);

    let invoice = client.get_invoice(&id);
    assert_eq!(invoice.funded_amount, 500_000);
}

#[test]
fn test_finalise_uphold_keeps_ban() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let payer = Address::generate(&env);

    client.initialize(&admin);

    let reason = bytesn32(&env, &[4u8; 32]);
    client.blacklist_payer(&admin, &payer, &reason);
    client.finalise_blacklist(&admin, &payer, &true);

    let entry = client.get_blacklist_entry(&payer).expect("entry must exist");
    assert!(entry.finalised);
    assert!(entry.upheld);
    assert!(client.is_payer_blacklisted(&payer));
}

#[test]
fn test_non_admin_cannot_blacklist() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let payer = Address::generate(&env);

    client.initialize(&admin);

    let reason = bytesn32(&env, &[5u8; 32]);
    // Attacker tries to blacklist — stored admin is the real admin,
    // but attacker.require_auth() will be called so it will work in
    // mock environment, but the address check fails. Wait — in mock,
    // mock_all_auths bypasses require_auth. But we check admin != stored_admin.
    let result = client.try_blacklist_payer(&attacker, &payer, &reason);
    assert_eq!(result, Err(Ok(Error::NotAdmin)));
}

#[test]
fn test_blacklist_entry_emits_event() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let payer = Address::generate(&env);

    client.initialize(&admin);

    let reason = bytesn32(&env, &[6u8; 32]);
    client.blacklist_payer(&admin, &payer, &reason);

    let events = env.events().all();
    let found = events.iter().any(|(_, topics, _)| {
        topics
            == (
                symbol_short!("blacklist"),
                symbol_short!("bl_add"),
            )
                .into_val(&env)
    });
    assert!(found, "bl_add event not emitted");
}

#[test]
fn test_finalise_blacklist_emits_event() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let payer = Address::generate(&env);

    client.initialize(&admin);

    let reason = bytesn32(&env, &[7u8; 32]);
    client.blacklist_payer(&admin, &payer, &reason);
    client.finalise_blacklist(&admin, &payer, &true);

    let events = env.events().all();
    let found = events.iter().any(|(_, topics, _)| {
        topics
            == (
                symbol_short!("blacklist"),
                symbol_short!("bl_fin"),
            )
                .into_val(&env)
    });
    assert!(found, "bl_fin event not emitted");
}

#[test]
fn test_is_payer_blacklisted_returns_false_for_non_blacklisted() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let payer = Address::generate(&env);

    client.initialize(&admin);
    assert!(!client.is_payer_blacklisted(&payer));
}

#[test]
fn test_cannot_finalise_twice() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let payer = Address::generate(&env);

    client.initialize(&admin);

    let reason = bytesn32(&env, &[8u8; 32]);
    client.blacklist_payer(&admin, &payer, &reason);
    client.finalise_blacklist(&admin, &payer, &true);

    let result = client.try_finalise_blacklist(&admin, &payer, &true);
    assert_eq!(result, Err(Ok(Error::AlreadyFinalised)));
}

// ---------------------------------------------------------------------------
// #735: get_escrow_balance
// ---------------------------------------------------------------------------

#[test]
fn test_get_escrow_balance_existing_invoice_returns_funded_amount() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let payer = Address::generate(&env);

    client.initialize(&admin);

    let token = create_token(&env, &token_admin);
    let total: i128 = 1_000;
    let deposit_amount: i128 = 400;
    let deadline: u64 = env.ledger().timestamp() + 10_000;

    // Mint enough tokens to the payer.
    mint(&env, &token, &token_admin, &payer, total);

    let invoice_id = client.create_invoice(
        &Address::generate(&env),
        &token,
        &total,
        &deadline,
    );

    // Before any deposit, balance should equal 0.
    assert_eq!(client.get_escrow_balance(&invoice_id), 0);

    // Deposit a partial amount.
    client.deposit(&payer, &invoice_id, &deposit_amount);

    // Balance must reflect the deposited amount.
    assert_eq!(client.get_escrow_balance(&invoice_id), deposit_amount);
}

#[test]
fn test_get_escrow_balance_unknown_invoice_returns_zero() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    client.initialize(&admin);

    // Invoice ID 9999 was never created — must return 0, not panic.
    assert_eq!(client.get_escrow_balance(&9999), 0);
}

// ---------------------------------------------------------------------------
// #736: EscrowReleased event
// ---------------------------------------------------------------------------

#[test]
fn test_release_emits_escrow_released_event() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);

    client.initialize(&admin);

    let token = create_token(&env, &token_admin);
    let total: i128 = 500;
    let deadline: u64 = env.ledger().timestamp() + 10_000;

    mint(&env, &token, &token_admin, &payer, total);

    let invoice_id = client.create_invoice(&creator, &token, &total, &deadline);

    // Deposit the full amount so the invoice becomes fully funded.
    client.deposit(&payer, &invoice_id, &total);

    // Verify EscrowReleased event was emitted.
    let all_events = env.events().all();
    let mut found = false;
    for event in all_events.iter() {
        // Topics for the structured release event are (escrow, released).
        let topics = event.1;
        if topics.len() >= 2 {
            if let Ok(t0) = <soroban_sdk::Symbol as soroban_sdk::TryFromVal<Env, soroban_sdk::Val>>::try_from_val(
                &env,
                &topics.get_unchecked(0),
            ) {
                if let Ok(t1) = <soroban_sdk::Symbol as soroban_sdk::TryFromVal<Env, soroban_sdk::Val>>::try_from_val(
                    &env,
                    &topics.get_unchecked(1),
                ) {
                    if t0 == symbol_short!("escrow") && t1 == symbol_short!("released") {
                        found = true;
                        break;
                    }
                }
            }
        }
    }
    assert!(found, "EscrowReleased event was not emitted during release");
}
