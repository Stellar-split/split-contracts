//! Shared test harness for StellarSplit integration tests.
//!
//! Provides a standard environment, token client, and invoice factory that all
//! integration tests reuse, eliminating boilerplate duplication across test files.

#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};

mod contract {
    soroban_sdk::contractimport!(file = "target/wasm32-unknown-unknown/release/split_contracts.wasm");
}

/// Create a default test environment with mock auth enabled.
pub fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

/// Create a Stellar asset (token) contract and return both the token admin and
/// the deployed token address.
///
/// The admin receives a large initial mint (1 billion units).
pub fn create_token(env: &Env) -> (Address, Address) {
    let admin = Address::generate(env);
    let token_id = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    StellarAssetClient::new(env, &token_id).mint(&admin, &1_000_000_000);
    (admin, token_id)
}

/// Create a simple invoice with defaults.
///
/// Returns the invoice ID.
pub fn create_invoice_defaults(
    env: &Env,
    client: &contract::Client,
    creator: &Address,
    token: &Address,
) -> u64 {
    let recipient = Address::generate(env);
    let mut recipients = Vec::new(env);
    recipients.push_back(recipient);
    let mut amounts = Vec::new(env);
    amounts.push_back(1000);

    client.create_invoice(creator, &recipients, &amounts, token, &10000)
}

/// Create an invoice with custom recipients, amounts, and deadline.
///
/// Returns the invoice ID.
pub fn create_invoice_custom(
    env: &Env,
    client: &contract::Client,
    creator: &Address,
    recipients: &Vec<Address>,
    amounts: &Vec<i128>,
    token: &Address,
    deadline: u64,
) -> u64 {
    client.create_invoice(creator, recipients, amounts, token, &deadline)
}

/// Mint tokens to a payer and make a payment toward an invoice.
pub fn fund_invoice(
    env: &Env,
    client: &contract::Client,
    token_id: &Address,
    payer: &Address,
    amount: i128,
    invoice_id: u64,
) {
    StellarAssetClient::new(env, token_id).mint(payer, &amount);
    client.pay(payer, &invoice_id, &amount);
}

/// Helper: register the contract WASM and return a client.
pub fn deploy_contract(env: &Env) -> contract::Client {
    let contract_id = env.register_contract_wasm(None, contract::WASM);
    contract::Client::new(env, &contract_id)
}
