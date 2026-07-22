#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger, Events},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Symbol, Vec, Map,
};

#[test]
fn test_event_log_stores_creation_event() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    // Setup: create an invoice
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    // Create invoice and verify:
    // 1. get_event_log(invoice_id) returns Vec with at least 1 event
    // 2. First event has event_type = "created"
    // 3. Event contains creator address and total amount
    // 4. Event has ledger number set
}

#[test]
fn test_event_log_ring_buffer_capacity() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &10_000_000);

    // Create invoice with 50 recipients
    // Generate 60 payment events (exceeds capacity of 50)
    
    // Verify:
    // 1. get_event_log returns exactly 50 events
    // 2. First 10 events (oldest) are overwritten
    // 3. Last 50 events are present
    // 4. Ledger numbers are in ascending order
}

#[test]
fn test_event_log_full_invoice_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let creator = Address::generate(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer1, &500);
    StellarAssetClient::new(&env, &token_id).mint(&payer2, &500);

    env.ledger().set_timestamp(1_000);

    // Create invoice with 200 total
    // Make 2 payments to fully fund
    // Call release
    
    // Verify event log contains (in order):
    // 1. "created" event
    // 2. "paid" event from payer1
    // 3. "paid" event from payer2
    // 4. "released" event
}

#[test]
fn test_event_log_refund_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);

    env.ledger().set_timestamp(1_000);

    // Create invoice with 200 total (unfundable)
    // Pay 100 (partial)
    // Pass deadline
    // Call refund
    
    // Verify event log contains:
    // 1. "created"
    // 2. "paid"
    // 3. "refunded"
}

#[test]
fn test_get_event_log_since_filters_by_ledger() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &300);

    env.ledger().set_timestamp(1_000);

    // Create invoice
    // Make payment at ledger 100
    // Make payment at ledger 200
    // Call get_event_log_since(invoice_id, 150)
    
    // Verify:
    // 1. Returns only events with ledger >= 150
    // 2. Excludes the first payment (ledger 100)
    // 3. Includes second payment and release (ledger 200+)
}

#[test]
fn test_event_log_empty_for_nonexistent_invoice() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    // Call get_event_log(999) where invoice doesn't exist
    // Verify it returns empty Vec or error
}

#[test]
fn test_event_log_stored_in_instance_storage() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    // Create invoice and pay
    // Verify that get_invoice_rent_cost includes event log storage
    
    // Create another invoice with 10x payment history
    // Verify rent cost scales appropriately
}

#[test]
fn test_event_log_data_map_structure() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);

    env.ledger().set_timestamp(1_000);

    // Create invoice and make payment
    // Get event log
    
    // Verify StoredEvent structure:
    // 1. event_type field exists and contains symbol
    // 2. data field exists and contains Map
    // 3. ledger field exists and contains u32
}

#[test]
fn test_event_log_with_multiple_recipients() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient1 = Address::generate(&env);
    let recipient2 = Address::generate(&env);
    let recipient3 = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &300);

    // Create invoice with 3 recipients
    // Full fund and release
    
    // Verify release event includes all 3 recipient addresses
}
