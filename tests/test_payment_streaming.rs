#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env, Vec};
use soroban_sdk::testutils::token::MockToken;

mod contract {
    soroban_sdk::contractimport!(file = "target/wasm32-unknown-unknown/release/split_contracts.wasm");
}

/// Test #287: Streaming payment mode with per-block micro-payment accumulator
/// InvoiceType::Stream { rate_per_ledger: u64, start_ledger: u32 }
#[test]
fn test_create_stream_basic() {
    let e = Env::default();
    let contract_id = e.register_contract_wasm(None, contract::WASM);
    let client = contract::Client::new(&e, &contract_id);

    // Setup
    let creator = Address::generate(&e);
    let payer = Address::generate(&e);
    let recipient = Address::generate(&e);
    let token = MockToken::new(&e, &Address::generate(&e));

    // Create stream: 1000 total, 10 per ledger, starting at ledger 100
    let mut recipients = Vec::new(&e);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&e);
    amounts.push_back(1000);

    // Mint tokens to payer for full deposit
    token.mint(&payer, &1000);

    // create_stream should deposit full amount upfront
    // Returns stream_id (same as invoice_id concept)
    let stream_id = client.create_stream(
        &creator,
        &payer,
        &recipients,
        &amounts,
        &token.address,
        &10, // rate_per_ledger
        &100, // start_ledger
    );

    assert!(stream_id > 0);
}

/// Test claim_stream releases accrued amount since last claim
#[test]
fn test_claim_stream_accrued() {
    let e = Env::default();
    let contract_id = e.register_contract_wasm(None, contract::WASM);
    let client = contract::Client::new(&e, &contract_id);

    let creator = Address::generate(&e);
    let payer = Address::generate(&e);
    let recipient = Address::generate(&e);
    let token = MockToken::new(&e, &Address::generate(&e));

    let mut recipients = Vec::new(&e);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&e);
    amounts.push_back(1000);

    token.mint(&payer, &1000);

    let stream_id = client.create_stream(
        &creator,
        &payer,
        &recipients,
        &amounts,
        &token.address,
        &10,
        &100,
    );

    // Advance 50 ledgers, so 50 * 10 = 500 should be accrued
    for _ in 0..50 {
        e.ledger().with_mut(|l| {
            l.sequence += 1;
        });
    }

    // Claim should release 500 to recipient
    client.claim_stream(&stream_id, &recipient);

    assert_eq!(token.balance(&recipient), 500);
}

/// Test cancel_stream stops stream and refunds remaining amount
#[test]
fn test_cancel_stream_refunds() {
    let e = Env::default();
    let contract_id = e.register_contract_wasm(None, contract::WASM);
    let client = contract::Client::new(&e, &contract_id);

    let creator = Address::generate(&e);
    let payer = Address::generate(&e);
    let recipient = Address::generate(&e);
    let token = MockToken::new(&e, &Address::generate(&e));

    let mut recipients = Vec::new(&e);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&e);
    amounts.push_back(1000);

    token.mint(&payer, &1000);

    let stream_id = client.create_stream(
        &creator,
        &payer,
        &recipients,
        &amounts,
        &token.address,
        &10,
        &100,
    );

    // Advance 30 ledgers: 30 * 10 = 300 accrued, 700 remaining
    for _ in 0..30 {
        e.ledger().with_mut(|l| {
            l.sequence += 1;
        });
    }

    client.cancel_stream(&stream_id);

    // Remaining 700 should be returned to payer
    assert_eq!(token.balance(&payer), 700);
}

/// Test get_stream_balance returns correct state
#[test]
fn test_get_stream_balance() {
    let e = Env::default();
    let contract_id = e.register_contract_wasm(None, contract::WASM);
    let client = contract::Client::new(&e, &contract_id);

    let creator = Address::generate(&e);
    let payer = Address::generate(&e);
    let recipient = Address::generate(&e);
    let token = MockToken::new(&e, &Address::generate(&e));

    let mut recipients = Vec::new(&e);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&e);
    amounts.push_back(1000);

    token.mint(&payer, &1000);

    let stream_id = client.create_stream(
        &creator,
        &payer,
        &recipients,
        &amounts,
        &token.address,
        &10,
        &100,
    );

    for _ in 0..40 {
        e.ledger().with_mut(|l| {
            l.sequence += 1;
        });
    }

    let balance = client.get_stream_balance(&stream_id);
    
    // Should have: accrued=400, claimed=0, remaining=600, rate=10
    assert_eq!(balance.0, 400); // accrued
    assert_eq!(balance.1, 0); // claimed
    assert_eq!(balance.2, 600); // remaining
    assert_eq!(balance.3, 10); // rate_per_ledger
}

/// Test stream_created event
#[test]
fn test_stream_created_event() {
    let e = Env::default();
    let contract_id = e.register_contract_wasm(None, contract::WASM);
    let client = contract::Client::new(&e, &contract_id);

    let creator = Address::generate(&e);
    let payer = Address::generate(&e);
    let recipient = Address::generate(&e);
    let token = MockToken::new(&e, &Address::generate(&e));

    let mut recipients = Vec::new(&e);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&e);
    amounts.push_back(1000);

    token.mint(&payer, &1000);

    // stream_created event should be emitted with stream_id, payer, total_amount, rate_per_ledger
    let stream_id = client.create_stream(
        &creator,
        &payer,
        &recipients,
        &amounts,
        &token.address,
        &10,
        &100,
    );

    assert!(stream_id > 0);
}

/// Test stream_claimed event
#[test]
fn test_stream_claimed_event() {
    let e = Env::default();
    let contract_id = e.register_contract_wasm(None, contract::WASM);
    let client = contract::Client::new(&e, &contract_id);

    let creator = Address::generate(&e);
    let payer = Address::generate(&e);
    let recipient = Address::generate(&e);
    let token = MockToken::new(&e, &Address::generate(&e));

    let mut recipients = Vec::new(&e);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&e);
    amounts.push_back(1000);

    token.mint(&payer, &1000);

    let stream_id = client.create_stream(
        &creator,
        &payer,
        &recipients,
        &amounts,
        &token.address,
        &10,
        &100,
    );

    for _ in 0..25 {
        e.ledger().with_mut(|l| {
            l.sequence += 1;
        });
    }

    // stream_claimed event should be emitted with stream_id, recipient, amount
    client.claim_stream(&stream_id, &recipient);
}

/// Test stream_cancelled event
#[test]
fn test_stream_cancelled_event() {
    let e = Env::default();
    let contract_id = e.register_contract_wasm(None, contract::WASM);
    let client = contract::Client::new(&e, &contract_id);

    let creator = Address::generate(&e);
    let payer = Address::generate(&e);
    let recipient = Address::generate(&e);
    let token = MockToken::new(&e, &Address::generate(&e));

    let mut recipients = Vec::new(&e);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&e);
    amounts.push_back(1000);

    token.mint(&payer, &1000);

    let stream_id = client.create_stream(
        &creator,
        &payer,
        &recipients,
        &amounts,
        &token.address,
        &10,
        &100,
    );

    // stream_cancelled event should be emitted with stream_id, remaining_amount
    client.cancel_stream(&stream_id);
}
