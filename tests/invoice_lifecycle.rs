#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env, Vec};
use soroban_sdk::testutils::token::MockToken;

mod contract {
    soroban_sdk::contractimport!(file = "target/wasm32-unknown-unknown/release/split_contracts.wasm");
}

#[test]
fn test_full_invoice_lifecycle() {
    let e = Env::default();
    let contract_id = e.register_contract_wasm(None, contract::WASM);
    let client = contract::Client::new(&e, &contract_id);

    // Setup
    let creator = Address::generate(&e);
    let payer = Address::generate(&e);
    let recipient = Address::generate(&e);
    let token = MockToken::new(&e, &Address::generate(&e));

    // Create
    let mut recipients = Vec::new(&e);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&e);
    amounts.push_back(1000);
    
    let invoice_id = client.create_invoice(&creator, &recipients, &amounts, &token.address, &10000);

    // Pay
    token.mint(&payer, &1000);
    client.pay(&payer, &invoice_id, &1000);

    // Release
    client.release(&invoice_id);

    // Assert
    assert_eq!(token.balance(&recipient), 1000);
}
