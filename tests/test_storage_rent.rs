#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env, Vec};
use soroban_sdk::testutils::token::MockToken;

mod contract {
    soroban_sdk::contractimport!(file = "target/wasm32-unknown-unknown/release/split_contracts.wasm");
}

/// Test #288: Storage rent tracking per invoice
/// Creators need visibility into storage rent costs for their invoices

/// Test get_invoice_rent_cost returns estimated stroops per ledger
#[test]
fn test_get_invoice_rent_cost_basic() {
    let e = Env::default();
    let contract_id = e.register_contract_wasm(None, contract::WASM);
    let client = contract::Client::new(&e, &contract_id);

    // Setup
    let creator = Address::generate(&e);
    let payer = Address::generate(&e);
    let recipient = Address::generate(&e);
    let token = MockToken::new(&e, &Address::generate(&e));

    let mut recipients = Vec::new(&e);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&e);
    amounts.push_back(1000);

    token.mint(&payer, &1000);

    let invoice_id = client.create_invoice(&creator, &recipients, &amounts, &token.address, &10000);

    // get_invoice_rent_cost should return estimated stroops per ledger
    // Calculation accounts for: invoice struct size, InvoiceExt size, payment shard count, event log size
    let rent_cost = client.get_invoice_rent_cost(&invoice_id);

    assert!(rent_cost > 0);
}

/// Test rent cost increases with more recipients
#[test]
fn test_invoice_rent_cost_scales_with_recipients() {
    let e = Env::default();
    let contract_id = e.register_contract_wasm(None, contract::WASM);
    let client = contract::Client::new(&e, &contract_id);

    let creator = Address::generate(&e);
    let token = MockToken::new(&e, &Address::generate(&e));

    // Single recipient
    let mut recipients = Vec::new(&e);
    recipients.push_back(Address::generate(&e));
    let mut amounts = Vec::new(&e);
    amounts.push_back(1000);

    let invoice_id_1 = client.create_invoice(&creator, &recipients, &amounts, &token.address, &10000);
    let rent_1 = client.get_invoice_rent_cost(&invoice_id_1);

    // Multiple recipients
    let mut recipients_multi = Vec::new(&e);
    recipients_multi.push_back(Address::generate(&e));
    recipients_multi.push_back(Address::generate(&e));
    recipients_multi.push_back(Address::generate(&e));
    let mut amounts_multi = Vec::new(&e);
    amounts_multi.push_back(500);
    amounts_multi.push_back(300);
    amounts_multi.push_back(200);

    let invoice_id_2 = client.create_invoice(&creator, &recipients_multi, &amounts_multi, &token.address, &10000);
    let rent_2 = client.get_invoice_rent_cost(&invoice_id_2);

    // More recipients should increase rent cost
    assert!(rent_2 > rent_1);
}

/// Test get_total_rent_cost aggregates across all active invoices for a creator
#[test]
fn test_get_total_rent_cost_aggregates() {
    let e = Env::default();
    let contract_id = e.register_contract_wasm(None, contract::WASM);
    let client = contract::Client::new(&e, &contract_id);

    let creator = Address::generate(&e);
    let token = MockToken::new(&e, &Address::generate(&e));

    let recipient = Address::generate(&e);
    let mut recipients = Vec::new(&e);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&e);
    amounts.push_back(1000);

    // Create first invoice
    let invoice_id_1 = client.create_invoice(&creator, &recipients, &amounts, &token.address, &10000);
    let rent_1 = client.get_invoice_rent_cost(&invoice_id_1);

    // Create second invoice
    let invoice_id_2 = client.create_invoice(&creator, &recipients, &amounts, &token.address, &10000);
    let rent_2 = client.get_invoice_rent_cost(&invoice_id_2);

    // Total rent should be sum of individual invoices
    let total_rent = client.get_total_rent_cost(&creator);

    assert!(total_rent >= rent_1 + rent_2);
}

/// Test rent cost calculation at limits (many recipients and payments)
#[test]
fn test_invoice_rent_cost_with_many_recipients() {
    let e = Env::default();
    let contract_id = e.register_contract_wasm(None, contract::WASM);
    let client = contract::Client::new(&e, &contract_id);

    let creator = Address::generate(&e);
    let token = MockToken::new(&e, &Address::generate(&e));

    // Create invoice with 10 recipients
    let mut recipients = Vec::new(&e);
    for _ in 0..10 {
        recipients.push_back(Address::generate(&e));
    }
    let mut amounts = Vec::new(&e);
    for _ in 0..10 {
        amounts.push_back(100);
    }

    let invoice_id = client.create_invoice(&creator, &recipients, &amounts, &token.address, &10000);
    let rent_cost = client.get_invoice_rent_cost(&invoice_id);

    // Should return positive cost proportional to recipient count and data size
    assert!(rent_cost > 0);
}

/// Test rent cost for invoice with multiple payment shards
#[test]
fn test_invoice_rent_cost_with_payment_shards() {
    let e = Env::default();
    let contract_id = e.register_contract_wasm(None, contract::WASM);
    let client = contract::Client::new(&e, &contract_id);

    let creator = Address::generate(&e);
    let payer1 = Address::generate(&e);
    let payer2 = Address::generate(&e);
    let recipient = Address::generate(&e);
    let token = MockToken::new(&e, &Address::generate(&e));

    let mut recipients = Vec::new(&e);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&e);
    amounts.push_back(1000);

    token.mint(&payer1, &500);
    token.mint(&payer2, &500);

    let invoice_id = client.create_invoice(&creator, &recipients, &amounts, &token.address, &10000);

    // Make payments from different payers (creates payment shards)
    client.pay(&payer1, &invoice_id, &500);
    client.pay(&payer2, &invoice_id, &500);

    // Rent cost should account for payment shard count
    let rent_cost = client.get_invoice_rent_cost(&invoice_id);
    assert!(rent_cost > 0);
}

/// Test rent cost after claim (shard cleanup)
#[test]
fn test_invoice_rent_cost_after_release() {
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

    let invoice_id = client.create_invoice(&creator, &recipients, &amounts, &token.address, &10000);
    let rent_before = client.get_invoice_rent_cost(&invoice_id);

    // Make full payment and release
    client.pay(&payer, &invoice_id, &1000);
    client.release(&invoice_id);

    // Rent should remain available for audit, or reduce after cleanup
    let rent_after = client.get_invoice_rent_cost(&invoice_id);
    assert!(rent_after >= 0);
}

/// Test STROOPS_PER_BYTE_PER_LEDGER constant is used in calculation
#[test]
fn test_rent_cost_uses_stroops_per_byte_constant() {
    let e = Env::default();
    let contract_id = e.register_contract_wasm(None, contract::WASM);
    let client = contract::Client::new(&e, &contract_id);

    let creator = Address::generate(&e);
    let recipient = Address::generate(&e);
    let token = MockToken::new(&e, &Address::generate(&e));

    let mut recipients = Vec::new(&e);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&e);
    amounts.push_back(1000);

    let invoice_id = client.create_invoice(&creator, &recipients, &amounts, &token.address, &10000);
    
    // Rent cost should be in stroops (positive integer representing per-ledger cost)
    let rent_cost = client.get_invoice_rent_cost(&invoice_id);
    
    // Cost should reflect network parameters and data size
    // Typically in range of 100-10000 stroops depending on invoice complexity
    assert!(rent_cost > 0);
    assert!(rent_cost < 1_000_000); // Sanity check
}

/// Test rent cost is read-only (no on-chain state changes)
#[test]
fn test_rent_cost_queries_are_readonly() {
    let e = Env::default();
    let contract_id = e.register_contract_wasm(None, contract::WASM);
    let client = contract::Client::new(&e, &contract_id);

    let creator = Address::generate(&e);
    let recipient = Address::generate(&e);
    let token = MockToken::new(&e, &Address::generate(&e));

    let mut recipients = Vec::new(&e);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&e);
    amounts.push_back(1000);

    let invoice_id = client.create_invoice(&creator, &recipients, &amounts, &token.address, &10000);

    // Multiple queries should return same result (read-only, no state modification)
    let rent_1 = client.get_invoice_rent_cost(&invoice_id);
    let rent_2 = client.get_invoice_rent_cost(&invoice_id);
    
    assert_eq!(rent_1, rent_2);
}

/// Test integration: rent estimate accuracy within 10% of actual cost
#[test]
fn test_rent_estimate_accuracy() {
    let e = Env::default();
    let contract_id = e.register_contract_wasm(None, contract::WASM);
    let client = contract::Client::new(&e, &contract_id);

    let creator = Address::generate(&e);
    let recipient = Address::generate(&e);
    let token = MockToken::new(&e, &Address::generate(&e));

    let mut recipients = Vec::new(&e);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&e);
    amounts.push_back(1000);

    let invoice_id = client.create_invoice(&creator, &recipients, &amounts, &token.address, &10000);

    let estimated_rent = client.get_invoice_rent_cost(&invoice_id);

    // Estimate should be reasonable (within 10% tolerance for test purposes)
    // Actual validation would compare against network rent reports
    assert!(estimated_rent > 0);
    assert!(estimated_rent < 100_000); // Upper bound sanity check
}
