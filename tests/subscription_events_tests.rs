#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Symbol, Vec,
};

#[test]
fn test_subscriber_joined_event_on_first_payment() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let creator = Address::generate(&env);
    let subscriber1 = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&subscriber1, &10_000);

    env.ledger().set_timestamp(1_000);
    env.ledger().set_sequence_number(100);

    // Create subscription invoice
    // Make first payment from subscriber1
    
    // Verify:
    // 1. subscriber_joined event emitted
    // 2. Event contains subscription_id
    // 3. Event contains subscriber address
    // 4. Event contains joined_at_ledger = 100
}

#[test]
fn test_subscriber_cancelled_event_on_cancellation() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let creator = Address::generate(&env);
    let subscriber = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&subscriber, &10_000);

    env.ledger().set_timestamp(1_000);
    env.ledger().set_sequence_number(100);

    // Create subscription invoice
    // Subscribe (make first payment)
    // Cancel subscription
    
    // Verify:
    // 1. subscriber_cancelled event emitted
    // 2. Event contains subscription_id
    // 3. Event contains subscriber address
    // 4. Event contains cancelled_at_ledger
    // 5. Event contains cycles_completed
}

#[test]
fn test_subscriber_lapsed_event_on_missed_cycle() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let creator = Address::generate(&env);
    let subscriber = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&subscriber, &10_000);

    env.ledger().set_timestamp(1_000);
    env.ledger().set_sequence_number(100);

    // Create subscription with daily cycle
    // Make first payment
    // Wait for cycle to pass without payment
    // Trigger lapse detection
    
    // Verify:
    // 1. subscriber_lapsed event emitted
    // 2. Event contains subscription_id
    // 3. Event contains subscriber address
    // 4. Event contains last_paid_cycle
}

#[test]
fn test_full_subscriber_lifecycle_events_in_order() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let creator = Address::generate(&env);
    let subscriber = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&subscriber, &50_000);

    env.ledger().set_timestamp(1_000);
    env.ledger().set_sequence_number(100);

    // Create subscription with monthly cycle
    // Cycle 1: subscriber makes payment → joined event
    // Cycle 2: subscriber makes payment (no event)
    // Cycle 3: subscriber misses → lapsed event
    // Cycle 4: subscriber makes payment (rejoins?)
    // Call cancel_subscription → cancelled event
    
    // Verify:
    // 1. Events in correct order: joined, lapsed, cancelled
    // 2. Each event has correct ledger number
    // 3. cycles_completed increments correctly
}

#[test]
fn test_get_active_subscribers_returns_correct_count() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let creator = Address::generate(&env);
    let subscriber1 = Address::generate(&env);
    let subscriber2 = Address::generate(&env);
    let subscriber3 = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&subscriber1, &10_000);
    StellarAssetClient::new(&env, &token_id).mint(&subscriber2, &10_000);
    StellarAssetClient::new(&env, &token_id).mint(&subscriber3, &10_000);

    env.ledger().set_timestamp(1_000);
    env.ledger().set_sequence_number(100);

    // Create subscription
    // Subscribe 3 people
    // Verify get_active_subscribers returns 3
    
    // Cancel one subscription
    // Verify get_active_subscribers returns 2
    
    // One lapse
    // Verify get_active_subscribers returns 1 (lapsed not counted as active)
}

#[test]
fn test_subscriber_joined_no_duplicate_on_resubscribe() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let creator = Address::generate(&env);
    let subscriber = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&subscriber, &50_000);

    env.ledger().set_timestamp(1_000);

    // Create subscription
    // Subscriber joins (payment)
    // Subscriber lapses
    // Subscriber resubscribes (payment again)
    
    // Verify:
    // 1. Only ONE subscriber_joined event
    // 2. lapsed event is separate
    // 3. No "rejoined" event, just continues subscription
}

#[test]
fn test_subscription_events_stored_in_log() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let creator = Address::generate(&env);
    let subscriber = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&subscriber, &10_000);

    env.ledger().set_timestamp(1_000);

    // Create subscription
    // Generate joined, lapsed, cancelled events
    // Call get_event_log(subscription_id)
    
    // Verify:
    // 1. All three event types present
    // 2. Correct order maintained
    // 3. Event data includes all fields
}

#[test]
fn test_multiple_subscribers_independent_events() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let creator = Address::generate(&env);
    let sub1 = Address::generate(&env);
    let sub2 = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sub1, &10_000);
    StellarAssetClient::new(&env, &token_id).mint(&sub2, &10_000);

    env.ledger().set_timestamp(1_000);
    env.ledger().set_sequence_number(100);

    // Create subscription
    // sub1 joins at ledger 100
    // sub2 joins at ledger 110
    // sub1 cancels at ledger 120
    
    // Verify:
    // 1. Each subscriber gets own event stream
    // 2. Events don't interfere
    // 3. joined_at_ledger differs for each (100 vs 110)
}

#[test]
fn test_cycles_completed_increments() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let creator = Address::generate(&env);
    let subscriber = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&subscriber, &100_000);

    env.ledger().set_timestamp(1_000);
    env.ledger().set_sequence_number(100);

    // Create subscription with monthly cycle
    // Pay for 12 months
    // Cancel
    
    // Verify:
    // 1. cancelled event shows cycles_completed = 12
    // 2. Each payment increments cycle counter
}

#[test]
fn test_subscription_events_with_get_event_log_since() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let creator = Address::generate(&env);
    let subscriber = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&subscriber, &50_000);

    env.ledger().set_timestamp(1_000);
    env.ledger().set_sequence_number(100);

    // Create subscription
    // joined at ledger 100
    // lapsed at ledger 200
    // Call get_event_log_since(sub_id, 150)
    
    // Verify:
    // 1. Returns only lapsed event
    // 2. joined event excluded
    // 3. Filtering works correctly
}
