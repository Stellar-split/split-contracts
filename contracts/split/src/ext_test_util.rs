//! Shared test fixtures for the `*_ext` feature modules (hold, freeze,
//! treasury_multi, velocity). Kept separate from `test.rs` so those modules do
//! not depend on its private helpers.
#![cfg(test)]

use crate::types::{self, InvoiceOptions};
use crate::SplitContractClient;
use soroban_sdk::{testutils::Address as _, token::StellarAssetClient, Address, Env, Vec};

pub(crate) struct Fixture<'a> {
    pub env: Env,
    pub c: SplitContractClient<'a>,
    pub token: Address,
    pub admin: Address,
}

pub(crate) fn fixture<'a>() -> Fixture<'a> {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(crate::SplitContract, ());
    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(token_admin).address();
    let c = SplitContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    c.initialize(&admin, &0_i128, &treasury, &token, &0_u32, &None, &0_u32, &0_u32, &0_u64);
    Fixture { env, c, token, admin }
}

pub(crate) fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, token).mint(to, &amount);
}

/// Create a single-recipient invoice; returns (invoice_id, creator, recipient).
pub(crate) fn new_invoice(f: &Fixture, amount: i128) -> (u64, Address, Address) {
    new_invoice_with_token(f, &f.token, amount)
}

pub(crate) fn new_invoice_with_token(
    f: &Fixture,
    token: &Address,
    amount: i128,
) -> (u64, Address, Address) {
    let creator = Address::generate(&f.env);
    let recipient = Address::generate(&f.env);
    let mut recipients = Vec::new(&f.env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&f.env);
    amounts.push_back(amount);
    let id = f.c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        token,
        &9_999_999_u64,
        &default_options(&f.env),
    );
    (id, creator, recipient)
}

pub(crate) fn pay(f: &Fixture, payer: &Address, id: u64, amount: i128) {
    f.c.pay(payer, &id, &amount, &0_u64, &false, &false, &None);
}

pub(crate) fn default_options(env: &Env) -> InvoiceOptions {
    InvoiceOptions {
        co_creators: Vec::new(env),
        allow_early_withdrawal: false,
        bonus_pool: 0,
        bonus_max_payers: 0,
        creator_cosigner: None,
        velocity_limit: 0,
        velocity_window: 0,
        prerequisite_id: None,
        tranches: Vec::new(env),
        co_signers: Vec::new(env),
        required_signatures: 0,
        penalty_bps: None,
        penalty_deadline: None,
        min_funding_bps: None,
        release_stages: Vec::new(env),
        price_oracle: None,
        swap_tokens: Vec::new(env),
        tax_bps: None,
        tax_authority: None,
        insurance_premium_bps: None,
        smart_route: None,
        notification_contract: None,
        overflow_behavior: types::OverflowBehavior::Reject,
        convert_to_stream: false,
        accepted_tokens: Vec::new(env),
        forward_to: None,
        forward_invoice_id: None,
        split_rules: Vec::new(env),
        auto_resolve_rules: Vec::new(env),
        condition_oracle: None,
        cross_chain_ref: None,
        allowed_payers: None,
        refund_grace_secs: None,
        priorities: Vec::new(env),
        require_kyc: false,
        scheduled_release_at: None,
        ratios: Vec::new(env),
        cosigners: None,
        cosigner_threshold: None,
        ext: types::InvoiceOptions2::default(),
    }
