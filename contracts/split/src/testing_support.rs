#![cfg(test)]

//! Shared helpers for the state machine, boundary, concurrency and
//! performance regression test suites.

use crate::test::default_options;
use crate::{SplitContract, SplitContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};

pub(crate) const START_TS: u64 = 1_000;
pub(crate) const DEADLINE: u64 = 9_999;

pub(crate) fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SplitContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

    let c = SplitContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

    env.ledger().set_timestamp(START_TS);
    (env, contract_id, token_id)
}

pub(crate) fn client<'a>(env: &'a Env, contract_id: &Address) -> SplitContractClient<'a> {
    SplitContractClient::new(env, contract_id)
}

pub(crate) fn token<'a>(env: &'a Env, token_id: &Address) -> TokenClient<'a> {
    TokenClient::new(env, token_id)
}

pub(crate) fn fund(env: &Env, token_id: &Address, who: &Address, amount: i128) {
    StellarAssetClient::new(env, token_id).mint(who, &amount);
}

/// Create a single-recipient invoice and return `(invoice_id, creator, recipient)`.
pub(crate) fn single_invoice(
    env: &Env,
    c: &SplitContractClient,
    token_id: &Address,
    amount: i128,
    deadline: u64,
) -> (u64, Address, Address) {
    let creator = Address::generate(env);
    let recipient = Address::generate(env);
    let mut recipients = Vec::new(env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(env);
    amounts.push_back(amount);
    let id = c.create_invoice(&creator, &recipients, &amounts, token_id, &deadline, &default_options(env));
    (id, creator, recipient)
}

/// Create an invoice with `n` recipients of `each` stroops apiece.
pub(crate) fn multi_invoice(
    env: &Env,
    c: &SplitContractClient,
    token_id: &Address,
    n: u32,
    each: i128,
) -> (u64, Vec<Address>) {
    let creator = Address::generate(env);
    let mut recipients = Vec::new(env);
    let mut amounts = Vec::new(env);
    for _ in 0..n {
        recipients.push_back(Address::generate(env));
        amounts.push_back(each);
    }
    let id = c.create_invoice(&creator, &recipients, &amounts, token_id, &DEADLINE, &default_options(env));
    (id, recipients)
}

/// Pay `amount` as `payer`, returning whether the call succeeded.
pub(crate) fn try_pay(c: &SplitContractClient, payer: &Address, id: u64, amount: i128) -> bool {
    c.try_pay(payer, &id, &amount, &0_u64, &false, &false, &None).is_ok()
}
