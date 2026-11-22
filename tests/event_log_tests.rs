#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger, Events, BytesN as _},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Symbol, Vec, Map, BytesN,
};
use split_contracts::types::InvoiceStatus;
use split_contracts::events::{invoice_state_changed, allowlist_updated, dispute_raised};

#[test]
fn test_event_log_stores_creation_event() {
    use split_contracts::contract::Client as SplitContractClient;

    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let contract_id = env.register_contract_wasm(None, split_contracts::WASM);
    let client = SplitContractClient::new(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let invoice_id = client.create_invoice(&creator, &recipients, &amounts, &token_id, &2_000);

    let events = client.get_event_log(&invoice_id);
    assert!(events.len() > 0);
    assert_eq!(events.len(), 1);

    // The creation event must be a detailed state-transition event.
    let created = events.get(0).unwrap();
    assert_eq!(created.event_type, Symbol::new(&env, "created"));
    assert_eq!(created.ledger, env.ledger().sequence());

    // Data map must carry the invoice id and the initial status.
    let status_val = created.data.get(Symbol::new(&env, "status"));
    assert!(status_val.is_some(), "created event must include status");
    let status: InvoiceStatus = status_val.unwrap().into_val(&env);
    assert_eq!(status, InvoiceStatus::Pending);

    let id_val = created.data.get(Symbol::new(&env, "invoice_id"));
    assert!(id_val.is_some(), "created event must include invoice_id");
    let logged_id: u64 = id_val.unwrap().into_val(&env);
    assert_eq!(logged_id, invoice_id);

    // The event must also be emitted as a contract event for indexability.
    let contract_events = env.events().all();
    assert!(
        contract_events.iter().any(|e| e.0 == contract_id),
        "created event must be emitted from the contract for indexing"
    );
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
    use split_contracts::contract::Client as SplitContractClient;

    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let contract_id = env.register_contract_wasm(None, split_contracts::WASM);
    let client = SplitContractClient::new(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer1, &100);
    StellarAssetClient::new(&env, &token_id).mint(&payer2, &100);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);

    let invoice_id = client.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999);

    client.pay(&payer1, &invoice_id, &100_i128, &0_u64, &false, &false, &None);
    client.pay(&payer2, &invoice_id, &100_i128, &0_u64, &false, &false, &None);

    let events = client.get_event_log(&invoice_id);
    assert!(events.len() >= 1);

    // Every state transition must be logged with a distinct event type.
    let mut saw_created = false;
    let mut saw_paid = false;
    let mut saw_released = false;
    let mut last_ledger = 0_u32;
    for i in 0..events.len() {
        let ev = events.get(i).unwrap();
        // Ledger numbers must be non-decreasing (indexable ordering).
        assert!(ev.ledger >= last_ledger, "event ledgers must be ordered");
        last_ledger = ev.ledger;

        // Each event must carry the invoice id in its data map.
        let id_val = ev.data.get(Symbol::new(&env, "invoice_id"));
        assert!(id_val.is_some(), "event must include invoice_id");
        let logged_id: u64 = id_val.unwrap().into_val(&env);
        assert_eq!(logged_id, invoice_id);

        if ev.event_type == Symbol::new(&env, "created") {
            saw_created = true;
        } else if ev.event_type == Symbol::new(&env, "paid") {
            saw_paid = true;
            // Payment events must include the amount paid.
            let amount_val = ev.data.get(Symbol::new(&env, "amount"));
            assert!(amount_val.is_some(), "paid event must include amount");
            let amount: i128 = amount_val.unwrap().into_val(&env);
            assert_eq!(amount, 100_i128);
        } else if ev.event_type == Symbol::new(&env, "released") {
            saw_released = true;
        }
    }
    assert!(saw_created, "lifecycle must log created");
    assert!(saw_paid, "lifecycle must log paid");
    assert!(saw_released, "lifecycle must log released");

    // Contract events must be emitted for each transition (indexable).
    let contract_events = env.events().all();
    assert!(
        contract_events.iter().filter(|e| e.0 == contract_id).count() >= 3,
        "each transition must emit a contract event"
    );
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

#[test]
fn payment_event_includes_breakdown_and_metadata() {
    use split_contracts::contract::Client as SplitContractClient;

    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let contract_id = env.register_contract_wasm(None, split_contracts::WASM);
    let client = SplitContractClient::new(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient1 = Address::generate(&env);
    let recipient2 = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &300);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient1.clone());
    recipients.push_back(recipient2.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);
    amounts.push_back(200_i128);

    let invoice_id = client.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999);

    client.pay(&payer, &invoice_id, &300_i128, &0_u64, &false, &false, &None);

    let events = client.get_event_log(&invoice_id);
    assert!(events.len() >= 2, "should contain created and paid events");

    let payment_event = events.last().expect("should have payment event");
    assert_eq!(payment_event.event_type, Symbol::new(&env, "paid"));

    let breakdown = payment_event
        .data
        .get(Symbol::new(&env, "breakdown"))
        .expect("payment event should include breakdown");
    let breakdown: Map<Address, i128> = breakdown.try_into().expect("breakdown should be a map");
    assert_eq!(breakdown.len(), 2, "breakdown should cover all recipients");
    assert_eq!(breakdown.get(recipient1.clone()), Some(100_i128));
    assert_eq!(breakdown.get(recipient2.clone()), Some(200_i128));

    let total = payment_event
        .data
        .get(Symbol::new(&env, "total"))
        .expect("payment event should include total");
    let total: i128 = total.try_into().expect("total should be i128");
    assert_eq!(total, 300_i128);

    let payer_meta = payment_event
        .data
        .get(Symbol::new(&env, "payer"))
        .expect("payment event should include payer metadata");
    let payer_meta: Address = payer_meta.try_into().expect("payer should be an address");
    assert_eq!(payer_meta, payer);

    assert!(payment_event.ledger > 0, "event should record ledger number");
}

#[test]
fn refund_event_includes_reason() {
    use split_contracts::contract::Client as SplitContractClient;
    use split_contracts::types::RefundReason;

    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let contract_id = env.register_contract_wasm(None, split_contracts::WASM);
    let client = SplitContractClient::new(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);

    let invoice_id = client.create_invoice(&creator, &recipients, &amounts, &token_id, &2_000);

    client.pay(&payer, &invoice_id, &100_i128, &0_u64, &false, &false, &None);

    env.ledger().set_timestamp(3_000);
    client.refund(&invoice_id);

    let events = client.get_event_log(&invoice_id);
    let refund_event = events.last().expect("should have refund event");
    assert_eq!(refund_event.event_type, Symbol::new(&env, "refunded"));

    let reason = refund_event
        .data
        .get(Symbol::new(&env, "reason"))
        .expect("refund event should include reason");
    let reason: RefundReason = reason.try_into().expect("reason should be a RefundReason");
    assert_eq!(reason, RefundReason::DeadlineExpired);

    let amount = refund_event
        .data
        .get(Symbol::new(&env, "amount"))
        .expect("refund event should include amount");
    let amount: i128 = amount.try_into().expect("amount should be i128");
    assert_eq!(amount, 100_i128);
}

#[test]
fn payment_and_refund_events_are_indexable() {
    use split_contracts::contract::Client as SplitContractClient;

    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let contract_id = env.register_contract_wasm(None, split_contracts::WASM);
    let client = SplitContractClient::new(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);

    let invoice_id = client.create_invoice(&creator, &recipients, &amounts, &token_id, &2_000);
    client.pay(&payer, &invoice_id, &100_i128, &0_u64, &false, &false, &None);

    env.ledger().set_timestamp(3_000);
    client.refund(&invoice_id);

    let events = env.events().all();
    assert!(!events.is_empty(), "should emit events");

    let mut saw_paid = false;
    let mut saw_refunded = false;
    for event in events.iter() {
        let (topics, _data): (Vec<Symbol>, soroban_sdk::Val) = event.parsed_data();
        assert!(!topics.is_empty(), "events must have fixed topics for indexing");
        let topic0 = topics.get(0).expect("topic 0");
        if topic0 == Symbol::new(&env, "paid") {
            saw_paid = true;
        }
        if topic0 == Symbol::new(&env, "refunded") {
            saw_refunded = true;
        }
    }

    assert!(saw_paid, "paid event should be indexable via topic");
    assert!(saw_refunded, "refunded event should be indexable via topic");
}

#[test]
fn fee_paid_event_carries_amount_and_treasury() {
    let env = Env::default();
    env.mock_all_auths();

    let treasury = Address::generate(&env);
    let expected_amount: i128 = 500;

    env.ledger().set_sequence(100);

    split_contracts::events::fee_paid(&env, 1, expected_amount, &treasury);

    let events = env.events().all();
    assert!(!events.is_empty(), "should have at least one event");

    let fee_paid_event = events.last().expect("should have fee_paid event");
    let (topics, data): (Vec<Symbol>, (i128, Address, u32)) = fee_paid_event.parsed_data();

    assert_eq!(data.0, expected_amount, "amount should match");
    assert_eq!(data.1, treasury, "treasury should match");

    // Topics must be indexable: event name plus fee type.
    assert!(!topics.is_empty(), "fee event must have topics");
    assert_eq!(topics.get(0).unwrap(), Symbol::new(&env, "fee_paid"));
    assert!(
        topics.iter().any(|t| t == Symbol::new(&env, "distribution")),
        "fee_paid must be tagged with the distribution fee type"
    );
}

#[test]
fn fee_calculated_event_carries_amount_and_type() {
    let env = Env::default();
    env.mock_all_auths();

    let expected_amount: i128 = 250;
    env.ledger().set_sequence(50);

    split_contracts::events::fee_calculated(&env, 7, expected_amount, Symbol::new(&env, "platform"));

    let events = env.events().all();
    assert!(!events.is_empty(), "should have at least one event");

    let fee_event = events.last().expect("should have fee_calculated event");
    let (topics, data): (Vec<Symbol>, (i128, Symbol, u32)) = fee_event.parsed_data();

    assert_eq!(data.0, expected_amount, "calculated amount should match");
    assert_eq!(data.1, Symbol::new(&env, "platform"), "fee type should match");

    // Topics must be indexable: event name plus fee type.
    assert!(!topics.is_empty(), "fee event must have topics");
    assert_eq!(topics.get(0).unwrap(), Symbol::new(&env, "fee_calculated"));
    assert!(
        topics.iter().any(|t| t == Symbol::new(&env, "platform")),
        "fee_calculated must be tagged with the platform fee type"
    );
}

#[test]
fn fee_events_emitted_per_fee_type_with_matching_amounts() {
    let env = Env::default();
    env.mock_all_auths();

    let treasury = Address::generate(&env);
    let platform_amount: i128 = 300;
    let distribution_amount: i128 = 200;

    env.ledger().set_sequence(10);
    split_contracts::events::fee_calculated(&env, 1, platform_amount, Symbol::new(&env, "platform"));
    split_contracts::events::fee_calculated(&env, 1, distribution_amount, Symbol::new(&env, "distribution"));
    split_contracts::events::fee_paid(&env, 1, distribution_amount, &treasury);

    let events = env.events().all();
    assert!(events.len() >= 3, "each fee type must emit its own event");

    // Verify the calculated amounts match the computed values per fee type.
    let mut platform_seen = false;
    let mut distribution_seen = false;
    for i in 0..events.len() {
        let ev = events.get(i).unwrap();
        let (topics, data): (Vec<Symbol>, (i128, Symbol, u32)) = ev.parsed_data();
        if topics.get(0).unwrap() == Symbol::new(&env, "fee_calculated") {
            if data.1 == Symbol::new(&env, "platform") {
                assert_eq!(data.0, platform_amount);
                platform_seen = true;
            } else if data.1 == Symbol::new(&env, "distribution") {
                assert_eq!(data.0, distribution_amount);
                distribution_seen = true;
            }
        }
    }
    assert!(platform_seen, "platform fee calculation must be emitted");
    assert!(distribution_seen, "distribution fee calculation must be emitted");
}
