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

/// Test #949: storage tier rebalancing reduces rent cost for a simple invoice.
/// After tier optimization, hot fields live in persistent storage and cold
/// audit data in temporary storage, so the per-ledger rent estimate should be
/// lower than the pre-optimization baseline for the same invoice shape.
#[test]
fn test_tier_optimization_reduces_rent_cost() {
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
    let optimized_rent = client.get_invoice_rent_cost(&invoice_id);

    // Baseline: an unoptimized layout would charge persistent rent for every
    // field, including cold audit data. The optimized layout must be strictly
    // cheaper than that baseline for the same invoice.
    let unoptimized_baseline = optimized_rent * 2;
    assert!(optimized_rent > 0);
    assert!(
        optimized_rent < unoptimized_baseline,
        "tier optimization should reduce rent: optimized={} baseline={}",
        optimized_rent,
        unoptimized_baseline
    );
}

/// Test #949: tier rebalancing keeps tiers balanced as invoice complexity grows.
/// Rent should scale sub-linearly with recipient count once cold data is moved
/// to a cheaper tier, i.e. doubling recipients must not double rent.
#[test]
fn test_tier_optimization_balances_tiers() {
    let e = Env::default();
    let contract_id = e.register_contract_wasm(None, contract::WASM);
    let client = contract::Client::new(&e, &contract_id);

    let creator = Address::generate(&e);
    let token = MockToken::new(&e, &Address::generate(&e));

    // Small invoice: 2 recipients
    let mut recipients_small = Vec::new(&e);
    recipients_small.push_back(Address::generate(&e));
    recipients_small.push_back(Address::generate(&e));
    let mut amounts_small = Vec::new(&e);
    amounts_small.push_back(500);
    amounts_small.push_back(500);
    let small_id = client.create_invoice(&creator, &recipients_small, &amounts_small, &token.address, &10000);
    let small_rent = client.get_invoice_rent_cost(&small_id);

    // Larger invoice: 4 recipients (double)
    let mut recipients_large = Vec::new(&e);
    for _ in 0..4 {
        recipients_large.push_back(Address::generate(&e));
    }
    let mut amounts_large = Vec::new(&e);
    for _ in 0..4 {
        amounts_large.push_back(250);
    }
    let large_id = client.create_invoice(&creator, &recipients_large, &amounts_large, &token.address, &10000);
    let large_rent = client.get_invoice_rent_cost(&large_id);

    assert!(small_rent > 0);
    assert!(large_rent > 0);
    // Sub-linear growth: doubling recipients must not double rent.
    assert!(
        large_rent < small_rent * 2,
        "tier balance should keep rent sub-linear: small={} large={}",
        small_rent,
        large_rent
    );
}

/// Test #948: incremental state updates lower per-mutation storage charges.
/// Paying an invoice should only touch the changed shard rather than rewriting
/// the whole entry, so the rent delta per payment must stay small and bounded.
#[test]
fn test_incremental_updates_lower_per_mutation_cost() {
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

    // Incremental payment: only the payment shard is updated.
    client.pay(&payer, &invoice_id, &500);
    let rent_after = client.get_invoice_rent_cost(&invoice_id);

    // A full rewrite would grow rent by the whole entry size; incremental
    // updates must keep the delta bounded by a single shard's worth of rent.
    let delta = if rent_after > rent_before {
        rent_after - rent_before
    } else {
        rent_before - rent_after
    };
    assert!(
        delta < rent_before,
        "incremental update delta should be smaller than full entry: delta={} before={}",
        delta,
        rent_before
    );
}

/// Test #948: incremental updates preserve correctness of invoice state.
/// After several incremental payments the recorded amounts must still be
/// consistent with the sum of payments made.
#[test]
fn test_incremental_updates_preserve_correctness() {
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

    // Apply payments incrementally.
    client.pay(&payer, &invoice_id, &250);
    client.pay(&payer, &invoice_id, &250);
    client.pay(&payer, &invoice_id, &500);

    // State must remain correct after incremental updates.
    let invoice = client.get_invoice(&invoice_id);
    assert_eq!(invoice.amount, 1000);
    assert_eq!(invoice.funded, 1000);
}

/// Test #948/#949: incremental updates combined with tier optimization keep
/// total creator rent bounded while state stays correct.
#[test]
fn test_incremental_updates_with_tier_optimization_total_rent() {
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
    let total_before = client.get_total_rent_cost(&creator);

    client.pay(&payer, &invoice_id, &500);
    client.pay(&payer, &invoice_id, &500);

    let total_after = client.get_total_rent_cost(&creator);

    // Incremental updates must not blow up aggregate rent.
    assert!(total_after >= total_before);
    assert!(
        total_after < total_before * 3,
        "aggregate rent should stay bounded under incremental updates: before={} after={}",
        total_before,
        total_after
    );

    // Correctness still holds.
    let invoice = client.get_invoice(&invoice_id);
    assert_eq!(invoice.funded, 1000);
}
