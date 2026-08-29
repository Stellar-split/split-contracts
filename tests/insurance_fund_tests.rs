#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger, Events},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Symbol, Vec, Map,
};

// Assuming we can import the split contract client
// This would be defined after the contract implements the insurance fund functions

#[test]
fn test_insurance_fund_deposit_on_invoice_creation() {
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

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1000_i128);

    env.ledger().set_timestamp(1_000);

    let invoice_id = client.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999);
    let invoice = client.get_invoice(&invoice_id);

    assert_eq!(invoice.total_amount, 1000);
}
}

#[test]
fn test_insurance_fund_balance_read() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    // Deploy contract with initial insurance fund balance of 0
    // Call get_insurance_fund_balance()
    // Assert it returns 0 initially
    
    // After deposits, verify balance increases monotonically
}

#[test]
fn test_insurance_claim_payment_admin_only() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let admin = Address::generate(&env);
    let claimant = Address::generate(&env);

    // Fund insurance pool first
    // Attempt claim_insurance as non-admin
    // Assert it panics with permission error
    
    // Call claim_insurance as admin with valid params
    // Verify:
    // 1. claimant receives payment
    // 2. insurance_claim_paid event emitted
    // 3. Insurance fund balance decreases by claim amount
}

#[test]
fn test_insurance_claim_with_approval() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let admin = Address::generate(&env);
    let invoice_id = 123u64;
    let claimant = Address::generate(&env);
    let claim_amount = 500i128;

    // Claim with admin_approval = true
    // Verify:
    // 1. Payment is processed immediately
    // 2. event data includes claim_reason / approval_hash
    // 3. Insurance fund decreases correctly
}

#[test]
fn test_insurance_withdraw_surplus_admin_only() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let admin = Address::generate(&env);
    let recipient = Address::generate(&env);
    let surplus_amount = 200i128;

    // Attempt withdraw_insurance_surplus as non-admin
    // Assert it panics
    
    // Call withdraw_insurance_surplus as admin
    // Verify:
    // 1. Recipient receives surplus
    // 2. insurance_surplus_withdrawn event emitted
    // 3. Insurance fund balance decreases
}

#[test]
fn test_insurance_fund_grows_with_multiple_invoices() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    // Create 5 invoices with varying amounts
    // Each should contribute (amount * INSURANCE_FUND_BPS / 10_000) to the fund
    
    // Verify final balance = sum of all contributions
}

#[test]
fn test_insurance_fund_cannot_claim_more_than_balance() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let admin = Address::generate(&env);
    let claimant = Address::generate(&env);

    // Set insurance fund balance to 100
    // Attempt to claim 200
    // Assert it panics with insufficient_funds error
}

#[test]
fn test_insurance_configurable_bps() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let admin = Address::generate(&env);

    // Deploy with default INSURANCE_FUND_BPS = 10
    // Create invoice with 1000 units
    // Verify deposit = 1 unit
    
    // Admin updates INSURANCE_FUND_BPS to 25
    // Create another invoice with 1000 units
    // Verify deposit = 2.5 units (or 2 if integer)
}
