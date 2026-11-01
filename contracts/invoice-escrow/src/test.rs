#![cfg(test)]

extern crate std;

use crate::errors::Error;
use crate::types::{EscrowStatus, RefundReason};
use crate::InvoiceEscrowContract;
use crate::InvoiceEscrowContractClient;
use soroban_sdk::testutils::{Address as _, Events, Ledger, LedgerInfo};
use soroban_sdk::{symbol_short, token, Address, Env, IntoVal, Val};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn xdr_event_topics(env: &Env, event: &soroban_sdk::xdr::ContractEvent) -> Option<soroban_sdk::Vec<Val>> {
    let soroban_sdk::xdr::ContractEventBody::V0(body) = &event.body;
    let mut topics = soroban_sdk::Vec::new(env);
    let topics_ref: &[soroban_sdk::xdr::ScVal] = body.topics.as_ref();
    for v in topics_ref.iter() {
        use soroban_sdk::TryFromVal;
        if let Ok(val) = <Val as TryFromVal<Env, soroban_sdk::xdr::ScVal>>::try_from_val(env, v) {
            topics.push_back(val);
        }
    }
    Some(topics)
}

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
    let found = events.events().iter().any(|event| {
        if let Some(topics) = xdr_event_topics(&env, event) {
            topics == (
                symbol_short!("escrow"),
                symbol_short!("adm_prop"),
            ).into_val(&env)
        } else {
            false
        }
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
    let found = events.events().iter().any(|event| {
        if let Some(topics) = xdr_event_topics(&env, event) {
            topics == (
                symbol_short!("escrow"),
                symbol_short!("adm_acpt"),
            ).into_val(&env)
        } else {
            false
        }
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
    let found = events.events().iter().any(|event| {
        if let Some(topics) = xdr_event_topics(&env, event) {
            topics == (
                symbol_short!("escrow"),
                symbol_short!("adm_cncl"),
            ).into_val(&env)
        } else {
            false
        }
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
// #934: deadline validation boundaries
// ---------------------------------------------------------------------------

/// Deadline rules enforced by `create_invoice`:
/// - The deadline must be strictly greater than the current ledger timestamp;
///   a deadline equal to "now" (or any past timestamp) is rejected with
///   `Error::DeadlinePassed`.
/// - The deadline must not exceed `now + MAX_DEADLINE_HORIZON`; anything
///   beyond that window is rejected with `Error::DeadlineOutOfRange`.
#[test]
fn test_create_invoice_deadline_equal_now_rejected() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token(&env, &token_admin);

    client.initialize(&admin);
    env.ledger().set_timestamp(1_000);

    // Boundary: deadline == now is not in the future.
    let result = client.try_create_invoice(&creator, &token, &1_000_000, &1_000u64);
    assert_eq!(result, Err(Ok(Error::DeadlinePassed)));
}

#[test]
fn test_create_invoice_deadline_now_plus_one_accepted() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token(&env, &token_admin);

    client.initialize(&admin);
    env.ledger().set_timestamp(1_000);

    // Boundary: the smallest valid future deadline.
    let id = client.create_invoice(&creator, &token, &1_000_000, &1_001u64);
    let invoice = client.get_invoice(&id);
    assert_eq!(invoice.deadline, 1_001);
}

#[test]
fn test_create_invoice_deadline_at_max_horizon_accepted() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token(&env, &token_admin);

    client.initialize(&admin);
    let now: u64 = 1_000;
    env.ledger().set_timestamp(now);

    // Boundary: exactly at the maximum allowed horizon.
    let deadline = now + crate::MAX_DEADLINE_HORIZON;
    let id = client.create_invoice(&creator, &token, &1_000_000, &deadline);
    let invoice = client.get_invoice(&id);
    assert_eq!(invoice.deadline, deadline);
}

#[test]
fn test_create_invoice_deadline_beyond_max_horizon_rejected() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token(&env, &token_admin);

    client.initialize(&admin);
    let now: u64 = 1_000;
    env.ledger().set_timestamp(now);

    // Boundary: one second past the maximum allowed horizon.
    let deadline = now + crate::MAX_DEADLINE_HORIZON + 1;
    let result = client.try_create_invoice(&creator, &token, &1_000_000, &deadline);
    assert_eq!(result, Err(Ok(Error::DeadlineOutOfRange)));
}

#[test]
fn test_create_invoice_far_past_deadline_rejected() {
    let (env, contract_id) = setup();
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token(&env, &token_admin);

    client.initialize(&admin);
    env.ledger().set_timestamp(1_000_000);

    // A deadline far in the past must be rejected as already passed.
    let result = client.try_create_invoice(&creator, &token, &1_000_000, &1u64);
    assert_eq!(result, Err(Ok(Error::DeadlinePassed)));
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
    // Set timestamp, then create invoice with a deadline just ahead.
    env.ledger().set_timestamp(1_000);
    let id = client.create_invoice(&creator, &token, &1_000_000, &1_001u64);

    // Advance ledger past deadline before depositing.
    env.ledger().set(LedgerInfo {
        timestamp: 1_002,
        protocol_version: env.ledger().protocol_version(),
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
        protocol_version: env.ledger().protocol_version(),
        sequence_number: 100,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 10,
        min_persistent_entry_ttl: 10,
        max_entry_ttl: 3_110_400,
    });

    let mut payers = soroban_sdk::Vec::new(&env);
    payers.push_back(payer.clone());
    client.refund(&id, &payers, &RefundReason::DeadlineExpired);

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
    let result = client.try_refund(&id, &payers, &RefundReason::DeadlineExpired);
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
    let found = events.events().iter().any(|event| {
        if let Some(topics) = xdr_event_topics(&env, event) {
            topics == (
                symbol_short!("blacklist"),
                symbol_short!("bl_add"),
            ).into_val(&env)
        } else {
            false
        }
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
    let found = events.events().iter().any(|event| {
        if let Some(topics) = xdr_event_topics(&env, event) {
            topics == (
                symbol_short!("blacklist"),
                symbol_short!("bl_fin"),
            ).into_val(&env)
        } else {
            false
        }
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
    for event in all_events.events().iter() {
        // Topics for the structured release event are (escrow, released).
        if let Some(topics) = xdr_event_topics(&env, event) {
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
    }
    assert!(found, "EscrowReleased event was not emitted during release");
}

// ---------------------------------------------------------------------------
// Co-signer approval (#914)
// ---------------------------------------------------------------------------

mod cosigner_tests {
    use super::*;
    use crate::types::ApprovalStatus;
    use soroban_sdk::{vec, Symbol, TryFromVal, Vec};

    struct Ctx {
        env: Env,
        contract_id: Address,
        creator: Address,
        payer: Address,
        token: Address,
        id: u64,
    }

    impl Ctx {
        fn client(&self) -> InvoiceEscrowContractClient<'_> {
            InvoiceEscrowContractClient::new(&self.env, &self.contract_id)
        }
    }

    fn setup_invoice(total: i128) -> Ctx {
        let (env, contract_id) = setup();
        let client = InvoiceEscrowContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        let payer = Address::generate(&env);
        let token_admin = Address::generate(&env);
        let token = create_token(&env, &token_admin);
        mint(&env, &token, &token_admin, &payer, total * 2);

        client.initialize(&admin);
        let deadline = env.ledger().timestamp() + 10_000;
        let id = client.create_invoice(&creator, &token, &total, &deadline);
        Ctx { env, contract_id, creator, payer, token, id }
    }

    fn signers(env: &Env, n: u32) -> Vec<Address> {
        let mut v = Vec::new(env);
        for _ in 0..n {
            v.push_back(Address::generate(env));
        }
        v
    }

    #[test]
    fn test_set_cosigners_creates_pending_approval() {
        let ctx = setup_invoice(1_000);
        let client = ctx.client();
        let s = signers(&ctx.env, 3);

        client.set_cosigners(&ctx.id, &s, &2);

        let approval = client.get_approval(&ctx.id).unwrap();
        assert_eq!(approval.signers, s);
        assert_eq!(approval.threshold, 2);
        assert_eq!(approval.approvals.len(), 0);
        assert_eq!(approval.status, ApprovalStatus::Pending);
        assert_eq!(client.approvals_remaining(&ctx.id), 2);
    }

    #[test]
    fn test_set_cosigners_invalid_configs() {
        let ctx = setup_invoice(1_000);
        let client = ctx.client();
        let s = signers(&ctx.env, 2);

        assert_eq!(
            client.try_set_cosigners(&ctx.id, &Vec::new(&ctx.env), &1),
            Err(Ok(Error::InvalidCosignerConfig))
        );
        assert_eq!(
            client.try_set_cosigners(&ctx.id, &s, &0),
            Err(Ok(Error::InvalidCosignerConfig))
        );
        assert_eq!(
            client.try_set_cosigners(&ctx.id, &s, &3),
            Err(Ok(Error::InvalidCosignerConfig))
        );
        let a = s.get(0).unwrap();
        let dup = vec![&ctx.env, a.clone(), a];
        assert_eq!(
            client.try_set_cosigners(&ctx.id, &dup, &1),
            Err(Ok(Error::InvalidCosignerConfig))
        );
        assert_eq!(
            client.try_set_cosigners(&ctx.id, &signers(&ctx.env, 21), &1),
            Err(Ok(Error::InvalidCosignerConfig))
        );
    }

    #[test]
    fn test_set_cosigners_unknown_invoice() {
        let ctx = setup_invoice(1_000);
        let s = signers(&ctx.env, 1);
        assert_eq!(
            ctx.client().try_set_cosigners(&99, &s, &1),
            Err(Ok(Error::InvoiceNotFound))
        );
    }

    #[test]
    fn test_set_cosigners_locked_after_vote() {
        let ctx = setup_invoice(1_000);
        let client = ctx.client();
        let s = signers(&ctx.env, 2);
        client.set_cosigners(&ctx.id, &s, &2);

        // Replacing before any vote is allowed.
        client.set_cosigners(&ctx.id, &s, &1);

        client.approve_release(&s.get(0).unwrap(), &ctx.id);
        assert_eq!(
            client.try_set_cosigners(&ctx.id, &s, &2),
            Err(Ok(Error::CosignersLocked))
        );
    }

    #[test]
    fn test_full_funding_does_not_auto_release_without_approval() {
        let ctx = setup_invoice(1_000);
        let client = ctx.client();
        let s = signers(&ctx.env, 2);
        client.set_cosigners(&ctx.id, &s, &2);

        client.deposit(&ctx.payer, &ctx.id, &1_000);
        let invoice = client.get_invoice(&ctx.id);
        assert_eq!(invoice.status, EscrowStatus::Active);
        assert_eq!(invoice.funded_amount, 1_000);

        assert_eq!(client.try_release(&ctx.id), Err(Ok(Error::ApprovalRequired)));
    }

    #[test]
    fn test_threshold_approval_unblocks_release() {
        let ctx = setup_invoice(1_000);
        let client = ctx.client();
        let s = signers(&ctx.env, 3);
        client.set_cosigners(&ctx.id, &s, &2);
        client.deposit(&ctx.payer, &ctx.id, &1_000);

        assert_eq!(client.approve_release(&s.get(0).unwrap(), &ctx.id), ApprovalStatus::Pending);
        assert_eq!(client.approvals_remaining(&ctx.id), 1);
        assert!(client.has_approved(&ctx.id, &s.get(0).unwrap()));
        assert!(!client.has_approved(&ctx.id, &s.get(1).unwrap()));

        assert_eq!(client.approve_release(&s.get(1).unwrap(), &ctx.id), ApprovalStatus::Approved);
        assert_eq!(client.get_approval_status(&ctx.id), Some(ApprovalStatus::Approved));
        assert_eq!(client.approvals_remaining(&ctx.id), 0);

        client.release(&ctx.id);
        assert_eq!(client.get_invoice(&ctx.id).status, EscrowStatus::Released);
        let tok = token::Client::new(&ctx.env, &ctx.token);
        assert_eq!(tok.balance(&ctx.creator), 1_000);
    }

    #[test]
    fn test_approval_before_full_funding_allows_auto_release() {
        let ctx = setup_invoice(1_000);
        let client = ctx.client();
        let s = signers(&ctx.env, 1);
        client.set_cosigners(&ctx.id, &s, &1);
        client.approve_release(&s.get(0).unwrap(), &ctx.id);

        client.deposit(&ctx.payer, &ctx.id, &1_000);
        assert_eq!(client.get_invoice(&ctx.id).status, EscrowStatus::Released);
    }

    #[test]
    fn test_non_cosigner_cannot_vote() {
        let ctx = setup_invoice(1_000);
        let client = ctx.client();
        client.set_cosigners(&ctx.id, &signers(&ctx.env, 2), &1);
        let outsider = Address::generate(&ctx.env);

        assert_eq!(
            client.try_approve_release(&outsider, &ctx.id),
            Err(Ok(Error::NotCosigner))
        );
        assert_eq!(
            client.try_reject_release(&outsider, &ctx.id),
            Err(Ok(Error::NotCosigner))
        );
    }

    #[test]
    fn test_vote_without_cosigners_fails() {
        let ctx = setup_invoice(1_000);
        let signer = Address::generate(&ctx.env);
        assert_eq!(
            ctx.client().try_approve_release(&signer, &ctx.id),
            Err(Ok(Error::NoCosigners))
        );
    }

    #[test]
    fn test_duplicate_votes_rejected() {
        let ctx = setup_invoice(1_000);
        let client = ctx.client();
        let s = signers(&ctx.env, 3);
        client.set_cosigners(&ctx.id, &s, &2);
        let a = s.get(0).unwrap();

        client.approve_release(&a, &ctx.id);
        assert_eq!(client.try_approve_release(&a, &ctx.id), Err(Ok(Error::AlreadyVoted)));
        assert_eq!(client.try_reject_release(&a, &ctx.id), Err(Ok(Error::AlreadyVoted)));

        // Tally unaffected by the rejected duplicates.
        let approval = client.get_approval(&ctx.id).unwrap();
        assert_eq!(approval.approvals.len(), 1);
        assert_eq!(approval.rejections.len(), 0);
    }

    #[test]
    fn test_interleaved_votes_tally_correctly() {
        let ctx = setup_invoice(1_000);
        let client = ctx.client();
        let s = signers(&ctx.env, 5);
        client.set_cosigners(&ctx.id, &s, &3);

        client.approve_release(&s.get(0).unwrap(), &ctx.id);
        client.reject_release(&s.get(1).unwrap(), &ctx.id);
        client.approve_release(&s.get(2).unwrap(), &ctx.id);
        client.reject_release(&s.get(3).unwrap(), &ctx.id);
        assert_eq!(client.get_approval_status(&ctx.id), Some(ApprovalStatus::Pending));

        assert_eq!(client.approve_release(&s.get(4).unwrap(), &ctx.id), ApprovalStatus::Approved);
        let approval = client.get_approval(&ctx.id).unwrap();
        assert_eq!(approval.approvals.len(), 3);
        assert_eq!(approval.rejections.len(), 2);
    }

    #[test]
    fn test_rejection_blocks_release_when_threshold_unreachable() {
        let ctx = setup_invoice(1_000);
        let client = ctx.client();
        let s = signers(&ctx.env, 3);
        client.set_cosigners(&ctx.id, &s, &2);
        client.deposit(&ctx.payer, &ctx.id, &1_000);

        assert_eq!(client.reject_release(&s.get(0).unwrap(), &ctx.id), ApprovalStatus::Pending);
        assert_eq!(client.reject_release(&s.get(1).unwrap(), &ctx.id), ApprovalStatus::Rejected);
        assert_eq!(client.try_release(&ctx.id), Err(Ok(Error::ApprovalRejected)));
    }

    #[test]
    fn test_revoke_transitions_status_back() {
        let ctx = setup_invoice(1_000);
        let client = ctx.client();
        let s = signers(&ctx.env, 2);
        client.set_cosigners(&ctx.id, &s, &1);
        let a = s.get(0).unwrap();
        let b = s.get(1).unwrap();

        assert_eq!(client.approve_release(&a, &ctx.id), ApprovalStatus::Approved);
        assert_eq!(client.revoke_vote(&a, &ctx.id), ApprovalStatus::Pending);
        assert!(!client.has_approved(&ctx.id, &a));

        client.reject_release(&a, &ctx.id);
        assert_eq!(client.reject_release(&b, &ctx.id), ApprovalStatus::Rejected);
        assert_eq!(client.revoke_vote(&b, &ctx.id), ApprovalStatus::Pending);

        assert_eq!(client.try_revoke_vote(&b, &ctx.id), Err(Ok(Error::NotVoted)));
    }

    #[test]
    fn test_votes_blocked_after_release() {
        let ctx = setup_invoice(1_000);
        let client = ctx.client();
        let s = signers(&ctx.env, 2);
        client.set_cosigners(&ctx.id, &s, &1);
        client.approve_release(&s.get(0).unwrap(), &ctx.id);
        client.deposit(&ctx.payer, &ctx.id, &1_000);
        assert_eq!(client.get_invoice(&ctx.id).status, EscrowStatus::Released);

        assert_eq!(
            client.try_approve_release(&s.get(1).unwrap(), &ctx.id),
            Err(Ok(Error::InvalidStatus))
        );
        assert_eq!(
            client.try_revoke_vote(&s.get(0).unwrap(), &ctx.id),
            Err(Ok(Error::InvalidStatus))
        );
        assert_eq!(
            client.try_set_cosigners(&ctx.id, &s, &1),
            Err(Ok(Error::InvalidStatus))
        );
    }

    #[test]
    fn test_invoice_without_cosigners_unaffected() {
        let ctx = setup_invoice(1_000);
        let client = ctx.client();
        assert_eq!(client.get_approval_status(&ctx.id), None);
        assert_eq!(client.approvals_remaining(&ctx.id), 0);
        client.deposit(&ctx.payer, &ctx.id, &1_000);
        assert_eq!(client.get_invoice(&ctx.id).status, EscrowStatus::Released);
    }

    #[test]
    fn test_approve_emits_events() {
        let ctx = setup_invoice(1_000);
        let client = ctx.client();
        let s = signers(&ctx.env, 1);
        client.set_cosigners(&ctx.id, &s, &1);
        client.approve_release(&s.get(0).unwrap(), &ctx.id);

        let events = ctx.env.events().all();
        let has_topic = |name: Symbol| {
            events.events().iter().any(|e| {
                xdr_event_topics(&ctx.env, e)
                    .map(|t| {
                        t.len() >= 2
                            && Symbol::try_from_val(&ctx.env, &t.get(1).unwrap())
                                .map_or(false, |sym| sym == name)
                    })
                    .unwrap_or(false)
            })
        };
        assert!(has_topic(symbol_short!("approved")));
        assert!(has_topic(symbol_short!("appr_stat")));
    }
}

// ---------------------------------------------------------------------------
// Oracle condition guards (#915)
// ---------------------------------------------------------------------------

mod oracle_tests {
    use super::*;
    use crate::types::{OracleCondition, OracleSource, PriceComparator, PriceData};
    use soroban_sdk::{contract, contractimpl, Symbol, TryFromVal};

    /// Configurable oracle: returns whatever `set_price` last stored.
    #[contract]
    pub struct MockOracle;

    #[contractimpl]
    impl MockOracle {
        pub fn set_price(env: Env, price: Option<PriceData>) {
            env.storage().instance().set(&symbol_short!("price"), &price);
        }

        pub fn lastprice(env: Env, _asset: Address) -> Option<PriceData> {
            env.storage()
                .instance()
                .get::<_, Option<PriceData>>(&symbol_short!("price"))
                .flatten()
        }
    }

    /// Oracle that always traps, simulating an unreachable feed.
    #[contract]
    pub struct BrokenOracle;

    #[contractimpl]
    impl BrokenOracle {
        pub fn lastprice(_env: Env, _asset: Address) -> Option<PriceData> {
            panic!("oracle offline")
        }
    }

    struct Ctx {
        env: Env,
        contract_id: Address,
        payer: Address,
        asset: Address,
        id: u64,
    }

    impl Ctx {
        fn client(&self) -> InvoiceEscrowContractClient<'_> {
            InvoiceEscrowContractClient::new(&self.env, &self.contract_id)
        }
    }

    const NOW: u64 = 1_000_000;

    fn setup_invoice() -> Ctx {
        let (env, contract_id) = setup();
        env.ledger().with_mut(|li| li.timestamp = NOW);
        let client = InvoiceEscrowContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        let payer = Address::generate(&env);
        let token_admin = Address::generate(&env);
        let token = create_token(&env, &token_admin);
        mint(&env, &token, &token_admin, &payer, 10_000);

        client.initialize(&admin);
        let id = client.create_invoice(&creator, &token, &1_000, &(NOW + 10_000));
        let asset = Address::generate(&env);
        Ctx { env, contract_id, payer, asset, id }
    }

    fn mock_oracle(env: &Env, price: Option<PriceData>) -> Address {
        let id = env.register_contract(None, MockOracle);
        MockOracleClient::new(env, &id).set_price(&price);
        id
    }

    fn price(p: i128, ts: u64) -> Option<PriceData> {
        Some(PriceData { price: p, timestamp: ts })
    }

    fn condition(ctx: &Ctx, oracle: &Address, fallback: Option<Address>) -> OracleCondition {
        OracleCondition {
            oracle: oracle.clone(),
            fallback_oracle: fallback,
            asset: ctx.asset.clone(),
            comparator: PriceComparator::GreaterOrEqual,
            threshold_price: 100,
            max_staleness_secs: 300,
        }
    }

    #[test]
    fn test_set_oracle_condition_stores_condition() {
        let ctx = setup_invoice();
        let oracle = mock_oracle(&ctx.env, price(150, NOW));
        let client = ctx.client();
        client.set_oracle_condition(&ctx.id, &condition(&ctx, &oracle, None));

        let stored = client.get_oracle_condition(&ctx.id).unwrap();
        assert_eq!(stored.oracle, oracle);
        assert_eq!(stored.threshold_price, 100);
    }

    #[test]
    fn test_set_oracle_condition_invalid_config() {
        let ctx = setup_invoice();
        let oracle = mock_oracle(&ctx.env, None);
        let client = ctx.client();

        let mut c = condition(&ctx, &oracle, None);
        c.threshold_price = 0;
        assert_eq!(client.try_set_oracle_condition(&ctx.id, &c), Err(Ok(Error::InvalidOracleConfig)));

        let mut c = condition(&ctx, &oracle, None);
        c.max_staleness_secs = 0;
        assert_eq!(client.try_set_oracle_condition(&ctx.id, &c), Err(Ok(Error::InvalidOracleConfig)));

        let c = condition(&ctx, &oracle, Some(oracle.clone()));
        assert_eq!(client.try_set_oracle_condition(&ctx.id, &c), Err(Ok(Error::InvalidOracleConfig)));
    }

    #[test]
    fn test_set_oracle_condition_locked_after_deposit() {
        let ctx = setup_invoice();
        let oracle = mock_oracle(&ctx.env, price(150, NOW));
        let client = ctx.client();
        client.deposit(&ctx.payer, &ctx.id, &100);
        assert_eq!(
            client.try_set_oracle_condition(&ctx.id, &condition(&ctx, &oracle, None)),
            Err(Ok(Error::InvalidStatus))
        );
    }

    #[test]
    fn test_condition_met_allows_auto_release() {
        let ctx = setup_invoice();
        let oracle = mock_oracle(&ctx.env, price(150, NOW - 10));
        let client = ctx.client();
        client.set_oracle_condition(&ctx.id, &condition(&ctx, &oracle, None));

        client.deposit(&ctx.payer, &ctx.id, &1_000);
        assert_eq!(client.get_invoice(&ctx.id).status, EscrowStatus::Released);

        let reading = client.get_last_oracle_reading(&ctx.id).unwrap();
        assert_eq!(reading.price, 150);
        assert_eq!(reading.source, OracleSource::Primary);
        assert!(reading.condition_met);
    }

    #[test]
    fn test_condition_not_met_blocks_release_until_price_moves() {
        let ctx = setup_invoice();
        let oracle = mock_oracle(&ctx.env, price(50, NOW));
        let client = ctx.client();
        client.set_oracle_condition(&ctx.id, &condition(&ctx, &oracle, None));

        client.deposit(&ctx.payer, &ctx.id, &1_000);
        assert_eq!(client.get_invoice(&ctx.id).status, EscrowStatus::Active);
        assert!(!client.get_last_oracle_reading(&ctx.id).unwrap().condition_met);
        assert_eq!(client.try_release(&ctx.id), Err(Ok(Error::OracleConditionNotMet)));

        MockOracleClient::new(&ctx.env, &oracle).set_price(&price(100, NOW));
        client.release(&ctx.id);
        assert_eq!(client.get_invoice(&ctx.id).status, EscrowStatus::Released);
    }

    #[test]
    fn test_less_or_equal_comparator() {
        let ctx = setup_invoice();
        let oracle = mock_oracle(&ctx.env, price(150, NOW));
        let client = ctx.client();
        let mut c = condition(&ctx, &oracle, None);
        c.comparator = PriceComparator::LessOrEqual;
        client.set_oracle_condition(&ctx.id, &c);

        assert!(!client.check_oracle_condition(&ctx.id).condition_met);
        MockOracleClient::new(&ctx.env, &oracle).set_price(&price(100, NOW));
        assert!(client.check_oracle_condition(&ctx.id).condition_met);
    }

    #[test]
    fn test_stale_price_rejected() {
        let ctx = setup_invoice();
        let oracle = mock_oracle(&ctx.env, price(150, NOW - 301));
        let client = ctx.client();
        client.set_oracle_condition(&ctx.id, &condition(&ctx, &oracle, None));
        client.deposit(&ctx.payer, &ctx.id, &1_000);

        assert_eq!(client.try_release(&ctx.id), Err(Ok(Error::OracleStalePrice)));
        assert_eq!(client.try_check_oracle_condition(&ctx.id), Err(Ok(Error::OracleStalePrice)));
    }

    #[test]
    fn test_invalid_prices_rejected() {
        let ctx = setup_invoice();
        let oracle = mock_oracle(&ctx.env, price(0, NOW));
        let client = ctx.client();
        client.set_oracle_condition(&ctx.id, &condition(&ctx, &oracle, None));
        assert_eq!(client.try_check_oracle_condition(&ctx.id), Err(Ok(Error::OracleInvalidPrice)));

        // Future-dated price.
        MockOracleClient::new(&ctx.env, &oracle).set_price(&price(150, NOW + 1));
        assert_eq!(client.try_check_oracle_condition(&ctx.id), Err(Ok(Error::OracleInvalidPrice)));
    }

    #[test]
    fn test_missing_price_is_unavailable() {
        let ctx = setup_invoice();
        let oracle = mock_oracle(&ctx.env, None);
        let client = ctx.client();
        client.set_oracle_condition(&ctx.id, &condition(&ctx, &oracle, None));
        assert_eq!(client.try_check_oracle_condition(&ctx.id), Err(Ok(Error::OracleUnavailable)));
    }

    #[test]
    fn test_broken_oracle_does_not_abort_deposit() {
        let ctx = setup_invoice();
        let broken = ctx.env.register_contract(None, BrokenOracle);
        let client = ctx.client();
        client.set_oracle_condition(&ctx.id, &condition(&ctx, &broken, None));

        // Deposit still succeeds; release is simply deferred.
        client.deposit(&ctx.payer, &ctx.id, &1_000);
        let invoice = client.get_invoice(&ctx.id);
        assert_eq!(invoice.status, EscrowStatus::Active);
        assert_eq!(invoice.funded_amount, 1_000);
        assert_eq!(client.try_release(&ctx.id), Err(Ok(Error::OracleUnavailable)));
    }

    #[test]
    fn test_fallback_used_when_primary_broken() {
        let ctx = setup_invoice();
        let broken = ctx.env.register_contract(None, BrokenOracle);
        let fallback = mock_oracle(&ctx.env, price(200, NOW));
        let client = ctx.client();
        client.set_oracle_condition(&ctx.id, &condition(&ctx, &broken, Some(fallback)));

        client.deposit(&ctx.payer, &ctx.id, &1_000);
        assert_eq!(client.get_invoice(&ctx.id).status, EscrowStatus::Released);
        let reading = client.get_last_oracle_reading(&ctx.id).unwrap();
        assert_eq!(reading.source, OracleSource::Fallback);
        assert_eq!(reading.price, 200);
    }

    #[test]
    fn test_fallback_used_when_primary_stale() {
        let ctx = setup_invoice();
        let primary = mock_oracle(&ctx.env, price(150, NOW - 1_000));
        let fallback = mock_oracle(&ctx.env, price(120, NOW));
        let client = ctx.client();
        client.set_oracle_condition(&ctx.id, &condition(&ctx, &primary, Some(fallback)));

        let reading = client.check_oracle_condition(&ctx.id);
        assert_eq!(reading.source, OracleSource::Fallback);
        assert_eq!(reading.price, 120);

        let events = ctx.env.events().all();
        let fallback_emitted = events.events().iter().any(|e| {
            xdr_event_topics(&ctx.env, e)
                .map(|t| {
                    t.len() >= 2
                        && Symbol::try_from_val(&ctx.env, &t.get(1).unwrap())
                            .map_or(false, |sym| sym == symbol_short!("orc_fback"))
                })
                .unwrap_or(false)
        });
        assert!(fallback_emitted, "orc_fback event not emitted");
    }

    #[test]
    fn test_both_oracles_failing_blocks_release() {
        let ctx = setup_invoice();
        let broken = ctx.env.register_contract(None, BrokenOracle);
        let stale = mock_oracle(&ctx.env, price(150, NOW - 1_000));
        let client = ctx.client();
        client.set_oracle_condition(&ctx.id, &condition(&ctx, &broken, Some(stale)));
        client.deposit(&ctx.payer, &ctx.id, &1_000);

        // Error reported is the fallback's (last oracle tried).
        assert_eq!(client.try_release(&ctx.id), Err(Ok(Error::OracleStalePrice)));
    }

    #[test]
    fn test_primary_preferred_over_fallback() {
        let ctx = setup_invoice();
        let primary = mock_oracle(&ctx.env, price(150, NOW));
        let fallback = mock_oracle(&ctx.env, price(50, NOW));
        let client = ctx.client();
        client.set_oracle_condition(&ctx.id, &condition(&ctx, &primary, Some(fallback)));

        let reading = client.check_oracle_condition(&ctx.id);
        assert_eq!(reading.source, OracleSource::Primary);
        assert!(reading.condition_met);
    }

    #[test]
    fn test_admin_clear_condition_unblocks_release() {
        let ctx = setup_invoice();
        let broken = ctx.env.register_contract(None, BrokenOracle);
        let client = ctx.client();
        client.set_oracle_condition(&ctx.id, &condition(&ctx, &broken, None));
        client.deposit(&ctx.payer, &ctx.id, &1_000);
        assert_eq!(client.try_release(&ctx.id), Err(Ok(Error::OracleUnavailable)));

        client.clear_oracle_condition(&ctx.id);
        assert!(client.get_oracle_condition(&ctx.id).is_none());
        client.release(&ctx.id);
        assert_eq!(client.get_invoice(&ctx.id).status, EscrowStatus::Released);

        assert_eq!(client.try_clear_oracle_condition(&ctx.id), Err(Ok(Error::InvalidStatus)));
    }

    #[test]
    fn test_clear_without_condition_fails() {
        let ctx = setup_invoice();
        assert_eq!(
            ctx.client().try_clear_oracle_condition(&ctx.id),
            Err(Ok(Error::NoOracleCondition))
        );
        assert_eq!(
            ctx.client().try_check_oracle_condition(&ctx.id),
            Err(Ok(Error::NoOracleCondition))
        );
    }
}

// ---------------------------------------------------------------------------
// Scheduled release (#916)
// ---------------------------------------------------------------------------

mod schedule_tests {
    use super::*;
    use crate::types::ScheduleStatus;
    use soroban_sdk::testutils::storage::Persistent as _;

    struct Ctx {
        env: Env,
        contract_id: Address,
        payer: Address,
        id: u64,
    }

    impl Ctx {
        fn client(&self) -> InvoiceEscrowContractClient<'_> {
            InvoiceEscrowContractClient::new(&self.env, &self.contract_id)
        }

        fn advance(&self, secs: u64, ledgers: u32) {
            self.env.ledger().with_mut(|li| {
                li.timestamp += secs;
                li.sequence_number += ledgers;
            });
        }
    }

    const NOW: u64 = 1_000_000;

    fn setup_invoice() -> Ctx {
        let (env, contract_id) = setup();
        env.ledger().with_mut(|li| {
            li.timestamp = NOW;
            li.sequence_number = 100;
        });
        let client = InvoiceEscrowContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        let payer = Address::generate(&env);
        let token_admin = Address::generate(&env);
        let token = create_token(&env, &token_admin);
        mint(&env, &token, &token_admin, &payer, 10_000);

        client.initialize(&admin);
        let id = client.create_invoice(&creator, &token, &1_000, &(NOW + 100_000));
        Ctx { env, contract_id, payer, id }
    }

    #[test]
    fn test_schedule_release_records_schedule() {
        let ctx = setup_invoice();
        let client = ctx.client();
        let schedule = client.schedule_release(&ctx.id, &(NOW + 3_600), &720);

        assert_eq!(schedule.release_at, NOW + 3_600);
        assert_eq!(schedule.unlock_ledger, 820);
        assert_eq!(schedule.scheduled_at, NOW);
        assert_eq!(schedule.scheduled_ledger, 100);
        assert_eq!(schedule.status, ScheduleStatus::Scheduled);
        assert_eq!(client.get_release_schedule(&ctx.id), Some(schedule));
    }

    #[test]
    fn test_schedule_release_invalid_inputs() {
        let ctx = setup_invoice();
        let client = ctx.client();
        assert_eq!(
            client.try_schedule_release(&ctx.id, &NOW, &0),
            Err(Ok(Error::InvalidSchedule))
        );
        assert_eq!(
            client.try_schedule_release(&ctx.id, &(NOW - 1), &0),
            Err(Ok(Error::InvalidSchedule))
        );
        assert_eq!(
            client.try_schedule_release(&ctx.id, &(NOW + 1), &6_312_001),
            Err(Ok(Error::InvalidSchedule))
        );
        assert_eq!(
            client.try_schedule_release(&99, &(NOW + 1), &0),
            Err(Ok(Error::InvoiceNotFound))
        );
    }

    #[test]
    fn test_schedule_blocks_auto_release_until_due() {
        let ctx = setup_invoice();
        let client = ctx.client();
        client.schedule_release(&ctx.id, &(NOW + 3_600), &10);

        client.deposit(&ctx.payer, &ctx.id, &1_000);
        assert_eq!(client.get_invoice(&ctx.id).status, EscrowStatus::Active);
        assert_eq!(client.try_release(&ctx.id), Err(Ok(Error::ReleaseNotYetDue)));
        assert!(!client.is_release_due(&ctx.id));

        ctx.advance(3_600, 10);
        assert!(client.is_release_due(&ctx.id));
        client.release(&ctx.id);
        assert_eq!(client.get_invoice(&ctx.id).status, EscrowStatus::Released);
        assert_eq!(
            client.get_release_schedule(&ctx.id).unwrap().status,
            ScheduleStatus::Executed
        );
    }

    #[test]
    fn test_time_reached_but_ledgers_not_elapsed() {
        let ctx = setup_invoice();
        let client = ctx.client();
        client.schedule_release(&ctx.id, &(NOW + 60), &100);
        client.deposit(&ctx.payer, &ctx.id, &1_000);

        // Timestamp jumps ahead faster than ledgers close.
        ctx.advance(10_000, 99);
        assert_eq!(client.try_release(&ctx.id), Err(Ok(Error::ScheduleLedgerNotReached)));
        assert_eq!(client.ledgers_until_release(&ctx.id), 1);
        assert_eq!(client.seconds_until_release(&ctx.id), 0);

        ctx.advance(5, 1);
        client.release(&ctx.id);
        assert_eq!(client.get_invoice(&ctx.id).status, EscrowStatus::Released);
    }

    #[test]
    fn test_ledgers_elapsed_but_time_not_reached() {
        let ctx = setup_invoice();
        let client = ctx.client();
        client.schedule_release(&ctx.id, &(NOW + 3_600), &10);
        client.deposit(&ctx.payer, &ctx.id, &1_000);

        ctx.advance(3_599, 1_000);
        assert_eq!(client.try_release(&ctx.id), Err(Ok(Error::ReleaseNotYetDue)));
        assert_eq!(client.seconds_until_release(&ctx.id), 1);
        assert_eq!(client.ledgers_until_release(&ctx.id), 0);
    }

    #[test]
    fn test_release_exactly_at_boundaries() {
        let ctx = setup_invoice();
        let client = ctx.client();
        client.schedule_release(&ctx.id, &(NOW + 500), &50);
        client.deposit(&ctx.payer, &ctx.id, &1_000);

        ctx.advance(500, 50);
        assert_eq!(client.seconds_until_release(&ctx.id), 0);
        assert_eq!(client.ledgers_until_release(&ctx.id), 0);
        client.release(&ctx.id);
        assert_eq!(client.get_invoice(&ctx.id).status, EscrowStatus::Released);
    }

    #[test]
    fn test_countdown_views() {
        let ctx = setup_invoice();
        let client = ctx.client();
        client.schedule_release(&ctx.id, &(NOW + 1_000), &200);

        assert_eq!(client.seconds_until_release(&ctx.id), 1_000);
        assert_eq!(client.ledgers_until_release(&ctx.id), 200);
        ctx.advance(250, 50);
        assert_eq!(client.seconds_until_release(&ctx.id), 750);
        assert_eq!(client.ledgers_until_release(&ctx.id), 150);
    }

    #[test]
    fn test_due_schedule_allows_auto_release_on_deposit() {
        let ctx = setup_invoice();
        let client = ctx.client();
        client.schedule_release(&ctx.id, &(NOW + 10), &1);
        ctx.advance(10, 1);

        client.deposit(&ctx.payer, &ctx.id, &1_000);
        assert_eq!(client.get_invoice(&ctx.id).status, EscrowStatus::Released);
        assert_eq!(
            client.get_release_schedule(&ctx.id).unwrap().status,
            ScheduleStatus::Executed
        );
    }

    #[test]
    fn test_reschedule_replaces_schedule() {
        let ctx = setup_invoice();
        let client = ctx.client();
        client.schedule_release(&ctx.id, &(NOW + 10_000), &1_000);
        client.schedule_release(&ctx.id, &(NOW + 10), &0);
        client.deposit(&ctx.payer, &ctx.id, &1_000);

        ctx.advance(10, 0);
        client.release(&ctx.id);
        assert_eq!(client.get_invoice(&ctx.id).status, EscrowStatus::Released);
    }

    #[test]
    fn test_cancel_schedule_unblocks_release() {
        let ctx = setup_invoice();
        let client = ctx.client();
        client.schedule_release(&ctx.id, &(NOW + 10_000), &1_000);
        client.deposit(&ctx.payer, &ctx.id, &1_000);
        assert_eq!(client.try_release(&ctx.id), Err(Ok(Error::ReleaseNotYetDue)));

        client.cancel_release_schedule(&ctx.id);
        assert_eq!(
            client.get_release_schedule(&ctx.id).unwrap().status,
            ScheduleStatus::Cancelled
        );
        assert!(client.is_release_due(&ctx.id));
        assert_eq!(client.seconds_until_release(&ctx.id), 0);

        client.release(&ctx.id);
        assert_eq!(client.get_invoice(&ctx.id).status, EscrowStatus::Released);
        // Cancelled schedule stays cancelled rather than becoming executed.
        assert_eq!(
            client.get_release_schedule(&ctx.id).unwrap().status,
            ScheduleStatus::Cancelled
        );
    }

    #[test]
    fn test_cancel_without_schedule_fails() {
        let ctx = setup_invoice();
        let client = ctx.client();
        assert_eq!(
            client.try_cancel_release_schedule(&ctx.id),
            Err(Ok(Error::ScheduleNotFound))
        );
        client.schedule_release(&ctx.id, &(NOW + 10), &0);
        client.cancel_release_schedule(&ctx.id);
        assert_eq!(
            client.try_cancel_release_schedule(&ctx.id),
            Err(Ok(Error::ScheduleNotFound))
        );
    }

    #[test]
    fn test_schedule_blocked_after_release() {
        let ctx = setup_invoice();
        let client = ctx.client();
        client.deposit(&ctx.payer, &ctx.id, &1_000);
        assert_eq!(
            client.try_schedule_release(&ctx.id, &(NOW + 10), &0),
            Err(Ok(Error::InvalidStatus))
        );
    }

    #[test]
    fn test_schedule_persists_until_due() {
        let ctx = setup_invoice();
        let client = ctx.client();
        client.schedule_release(&ctx.id, &(NOW + 10), &100_000);

        let ttl = ctx.env.as_contract(&ctx.contract_id, || {
            ctx.env
                .storage()
                .persistent()
                .get_ttl(&(symbol_short!("schedule"), ctx.id))
        });
        assert!(ttl >= 100_000, "schedule TTL {} shorter than delay", ttl);
    }

    #[test]
    fn test_no_schedule_views() {
        let ctx = setup_invoice();
        let client = ctx.client();
        assert_eq!(client.get_release_schedule(&ctx.id), None);
        assert!(client.is_release_due(&ctx.id));
        assert_eq!(client.seconds_until_release(&ctx.id), 0);
        assert_eq!(client.ledgers_until_release(&ctx.id), 0);
    }
}

// ---------------------------------------------------------------------------
// Refund grace period (#917)
// ---------------------------------------------------------------------------

mod refund_grace_tests {
    use super::*;
    use crate::types::RefundGraceStatus;
    use soroban_sdk::vec;

    struct Ctx {
        env: Env,
        contract_id: Address,
        payer: Address,
        token: Address,
        creator: Address,
    }

    impl Ctx {
        fn client(&self) -> InvoiceEscrowContractClient<'_> {
            InvoiceEscrowContractClient::new(&self.env, &self.contract_id)
        }

        fn set_time(&self, ts: u64) {
            self.env.ledger().with_mut(|li| li.timestamp = ts);
        }

        fn create(&self) -> u64 {
            self.client()
                .create_invoice(&self.creator, &self.token, &1_000, &DEADLINE)
        }
    }

    const NOW: u64 = 1_000_000;
    const DEADLINE: u64 = NOW + 1_000;
    const GRACE: u64 = 3_600;

    fn setup_ctx() -> Ctx {
        let (env, contract_id) = setup();
        env.ledger().with_mut(|li| li.timestamp = NOW);
        let client = InvoiceEscrowContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        let payer = Address::generate(&env);
        let token_admin = Address::generate(&env);
        let token = create_token(&env, &token_admin);
        mint(&env, &token, &token_admin, &payer, 10_000);
        client.initialize(&admin);
        Ctx { env, contract_id, payer, token, creator }
    }

    #[test]
    fn test_default_grace_config() {
        let ctx = setup_ctx();
        let client = ctx.client();
        assert_eq!(client.get_default_refund_grace(), 0);
        client.set_default_refund_grace(&GRACE);
        assert_eq!(client.get_default_refund_grace(), GRACE);

        assert_eq!(
            client.try_set_default_refund_grace(&(30 * 24 * 60 * 60 + 1)),
            Err(Ok(Error::InvalidGracePeriod))
        );
    }

    #[test]
    fn test_default_grace_snapshotted_at_creation() {
        let ctx = setup_ctx();
        let client = ctx.client();
        let without = ctx.create();
        client.set_default_refund_grace(&GRACE);
        let with = ctx.create();
        client.set_default_refund_grace(&10);

        assert_eq!(client.get_refund_grace(&without), None);
        assert_eq!(client.get_refund_grace(&with).unwrap().grace_secs, GRACE);
        assert_eq!(client.refund_available_after(&without), DEADLINE);
        assert_eq!(client.refund_available_after(&with), DEADLINE + GRACE);
    }

    #[test]
    fn test_no_grace_refund_at_deadline_unchanged() {
        let ctx = setup_ctx();
        let client = ctx.client();
        let id = ctx.create();
        client.deposit(&ctx.payer, &id, &400);
        assert_eq!(client.get_refund_grace_status(&id), RefundGraceStatus::NoGrace);

        ctx.set_time(DEADLINE + 1);
        client.refund(&id, &vec![&ctx.env, ctx.payer.clone()], &RefundReason::DeadlineExpired);
        assert_eq!(client.get_invoice(&id).status, EscrowStatus::Refunded);
    }

    #[test]
    fn test_grace_blocks_refund_then_allows_after_elapsed() {
        let ctx = setup_ctx();
        let client = ctx.client();
        client.set_default_refund_grace(&GRACE);
        let id = ctx.create();
        client.deposit(&ctx.payer, &id, &400);
        let payers = vec![&ctx.env, ctx.payer.clone()];

        assert_eq!(client.get_refund_grace_status(&id), RefundGraceStatus::NotStarted);
        assert_eq!(client.try_refund(&id, &payers, &RefundReason::DeadlineExpired), Err(Ok(Error::DeadlineNotPassed)));

        ctx.set_time(DEADLINE + 1);
        assert_eq!(client.get_refund_grace_status(&id), RefundGraceStatus::Active);
        assert_eq!(client.try_refund(&id, &payers, &RefundReason::DeadlineExpired), Err(Ok(Error::RefundGraceActive)));

        // Boundary: refunds open strictly after deadline + grace.
        ctx.set_time(DEADLINE + GRACE);
        assert_eq!(client.try_refund(&id, &payers, &RefundReason::DeadlineExpired), Err(Ok(Error::RefundGraceActive)));

        ctx.set_time(DEADLINE + GRACE + 1);
        assert_eq!(client.get_refund_grace_status(&id), RefundGraceStatus::Elapsed);
        client.refund(&id, &payers, &RefundReason::DeadlineExpired);
        assert_eq!(client.get_invoice(&id).status, EscrowStatus::Refunded);
        let tok = token::Client::new(&ctx.env, &ctx.token);
        assert_eq!(tok.balance(&ctx.payer), 10_000);
    }

    #[test]
    fn test_timer_counts_down() {
        let ctx = setup_ctx();
        let client = ctx.client();
        client.set_default_refund_grace(&GRACE);
        let id = ctx.create();

        // Before the deadline the full window is reported.
        assert_eq!(client.refund_grace_remaining(&id), GRACE);
        ctx.set_time(DEADLINE);
        assert_eq!(client.refund_grace_remaining(&id), GRACE);
        ctx.set_time(DEADLINE + 600);
        assert_eq!(client.refund_grace_remaining(&id), GRACE - 600);
        ctx.set_time(DEADLINE + GRACE);
        assert_eq!(client.refund_grace_remaining(&id), 0);
        ctx.set_time(DEADLINE + GRACE + 100);
        assert_eq!(client.refund_grace_remaining(&id), 0);
    }

    #[test]
    fn test_creator_sets_invoice_grace_before_deposits() {
        let ctx = setup_ctx();
        let client = ctx.client();
        client.set_default_refund_grace(&GRACE);
        let id = ctx.create();

        client.set_refund_grace(&id, &60);
        assert_eq!(client.refund_available_after(&id), DEADLINE + 60);

        client.set_refund_grace(&id, &0);
        assert_eq!(client.get_refund_grace_status(&id), RefundGraceStatus::NoGrace);

        assert_eq!(
            client.try_set_refund_grace(&id, &(30 * 24 * 60 * 60 + 1)),
            Err(Ok(Error::InvalidGracePeriod))
        );

        client.deposit(&ctx.payer, &id, &100);
        assert_eq!(client.try_set_refund_grace(&id, &60), Err(Ok(Error::InvalidStatus)));
    }

    #[test]
    fn test_admin_extend_grace() {
        let ctx = setup_ctx();
        let client = ctx.client();
        client.set_default_refund_grace(&GRACE);
        let id = ctx.create();
        client.deposit(&ctx.payer, &id, &400);
        let payers = vec![&ctx.env, ctx.payer.clone()];

        ctx.set_time(DEADLINE + 10);
        client.extend_refund_grace(&id, &1_000);
        assert_eq!(client.get_refund_grace(&id).unwrap().extension_secs, 1_000);
        assert_eq!(client.refund_available_after(&id), DEADLINE + GRACE + 1_000);

        ctx.set_time(DEADLINE + GRACE + 1);
        assert_eq!(client.try_refund(&id, &payers, &RefundReason::DeadlineExpired), Err(Ok(Error::RefundGraceActive)));
        ctx.set_time(DEADLINE + GRACE + 1_001);
        client.refund(&id, &payers, &RefundReason::DeadlineExpired);
        assert_eq!(client.get_invoice(&id).status, EscrowStatus::Refunded);
    }

    #[test]
    fn test_extend_grace_limits() {
        let ctx = setup_ctx();
        let client = ctx.client();
        client.set_default_refund_grace(&GRACE);
        let id = ctx.create();

        assert_eq!(client.try_extend_refund_grace(&id, &0), Err(Ok(Error::InvalidGracePeriod)));
        assert_eq!(
            client.try_extend_refund_grace(&id, &(30 * 24 * 60 * 60)),
            Err(Ok(Error::InvalidGracePeriod))
        );
        assert_eq!(
            client.try_extend_refund_grace(&id, &u64::MAX),
            Err(Ok(Error::InvalidGracePeriod))
        );

        ctx.set_time(DEADLINE + GRACE + 1);
        assert_eq!(
            client.try_extend_refund_grace(&id, &10),
            Err(Ok(Error::RefundGraceElapsed))
        );
    }

    #[test]
    fn test_extend_creates_grace_when_none_before_deadline() {
        let ctx = setup_ctx();
        let client = ctx.client();
        let id = ctx.create();

        client.extend_refund_grace(&id, &500);
        assert_eq!(client.refund_available_after(&id), DEADLINE + 500);

        let other = ctx.create();
        ctx.set_time(DEADLINE + 1);
        assert_eq!(
            client.try_extend_refund_grace(&other, &500),
            Err(Ok(Error::RefundGraceElapsed))
        );
    }

    #[test]
    fn test_admin_waive_allows_immediate_refund() {
        let ctx = setup_ctx();
        let client = ctx.client();
        client.set_default_refund_grace(&GRACE);
        let id = ctx.create();
        client.deposit(&ctx.payer, &id, &400);
        let payers = vec![&ctx.env, ctx.payer.clone()];

        ctx.set_time(DEADLINE + 1);
        assert_eq!(client.try_refund(&id, &payers, &RefundReason::DeadlineExpired), Err(Ok(Error::RefundGraceActive)));

        client.waive_refund_grace(&id);
        assert_eq!(client.get_refund_grace_status(&id), RefundGraceStatus::Waived);
        assert_eq!(client.refund_grace_remaining(&id), 0);
        assert_eq!(client.refund_available_after(&id), DEADLINE);

        client.refund(&id, &payers, &RefundReason::DeadlineExpired);
        assert_eq!(client.get_invoice(&id).status, EscrowStatus::Refunded);
    }

    #[test]
    fn test_waive_before_deadline_still_requires_deadline() {
        let ctx = setup_ctx();
        let client = ctx.client();
        client.set_default_refund_grace(&GRACE);
        let id = ctx.create();
        client.deposit(&ctx.payer, &id, &400);

        client.waive_refund_grace(&id);
        assert_eq!(
            client.try_refund(&id, &vec![&ctx.env, ctx.payer.clone()], &RefundReason::DeadlineExpired),
            Err(Ok(Error::DeadlineNotPassed))
        );
    }

    #[test]
    fn test_waive_rejected_when_nothing_to_waive() {
        let ctx = setup_ctx();
        let client = ctx.client();
        let no_grace = ctx.create();
        assert_eq!(
            client.try_waive_refund_grace(&no_grace),
            Err(Ok(Error::RefundGraceElapsed))
        );

        client.set_default_refund_grace(&GRACE);
        let id = ctx.create();
        client.waive_refund_grace(&id);
        assert_eq!(client.try_waive_refund_grace(&id), Err(Ok(Error::RefundGraceElapsed)));
        assert_eq!(
            client.try_extend_refund_grace(&id, &10),
            Err(Ok(Error::RefundGraceElapsed))
        );
    }

    #[test]
    fn test_overrides_blocked_on_closed_invoice() {
        let ctx = setup_ctx();
        let client = ctx.client();
        client.set_default_refund_grace(&GRACE);
        let id = ctx.create();
        client.deposit(&ctx.payer, &id, &1_000);
        assert_eq!(client.get_invoice(&id).status, EscrowStatus::Released);

        assert_eq!(client.try_extend_refund_grace(&id, &10), Err(Ok(Error::InvalidStatus)));
        assert_eq!(client.try_waive_refund_grace(&id), Err(Ok(Error::InvalidStatus)));
    }

    #[test]
    fn test_release_still_possible_during_grace() {
        let ctx = setup_ctx();
        let client = ctx.client();
        client.set_default_refund_grace(&GRACE);
        let id = ctx.create();

        // Funding completes but a schedule delays release past the deadline.
        client.schedule_release(&id, &(DEADLINE + 100), &0);
        client.deposit(&ctx.payer, &id, &1_000);
        assert_eq!(client.get_invoice(&id).status, EscrowStatus::Active);

        ctx.set_time(DEADLINE + 100);
        assert_eq!(client.get_refund_grace_status(&id), RefundGraceStatus::Active);
        client.release(&id);
        assert_eq!(client.get_invoice(&id).status, EscrowStatus::Released);
    }

    #[test]
    fn test_grace_views_unknown_invoice() {
        let ctx = setup_ctx();
        let client = ctx.client();
        assert_eq!(client.try_get_refund_grace_status(&99), Err(Ok(Error::InvoiceNotFound)));
        assert_eq!(client.try_refund_grace_remaining(&99), Err(Ok(Error::InvoiceNotFound)));
        assert_eq!(client.try_refund_available_after(&99), Err(Ok(Error::InvoiceNotFound)));
    }
}
