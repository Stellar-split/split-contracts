//! Issue #784: per-operation storage layout snapshots.
//!
//! After each major operation we probe a fixed set of storage keys and record
//! which are present. The result is deterministic JSON (sorted keys) compared
//! against `tests/snapshots/ops_<op>.json`. Set `UPDATE_SNAPSHOTS=1` to
//! (re)write the files. A missing snapshot file is created on first run.
#![cfg(test)]
#![allow(clippy::all)]

extern crate std;

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, IntoVal, Val, Vec,
};
use std::string::String as StdString;
use std::vec::Vec as StdVec;

struct Ctx {
    env: Env,
    contract: Address,
    token: Address,
    creator: Address,
    payer: Address,
    recipient: Address,
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    let contract = env.register(SplitContract, ());
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let c = SplitContractClient::new(&env, &contract);
    c.initialize(
        &Address::generate(&env),
        &0_i128,
        &Address::generate(&env),
        &token,
        &0_u32,
        &None,
        &0_u32,
        &0_u32,
        &0_u64,
    );
    let payer = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&payer, &10_000);
    env.ledger().set_timestamp(1_000);
    Ctx {
        env: env.clone(),
        contract,
        token,
        creator: Address::generate(&env),
        payer,
        recipient: Address::generate(&env),
    }
}

fn opts(env: &Env) -> types::InvoiceOptions {
    types::InvoiceOptions {
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
}

fn create(x: &Ctx, amount: i128) -> u64 {
    let c = SplitContractClient::new(&x.env, &x.contract);
    let mut r = Vec::new(&x.env);
    r.push_back(x.recipient.clone());
    let mut a = Vec::new(&x.env);
    a.push_back(amount);
    c.create_invoice(&x.creator, &r, &a, &x.token, &2_000_u64, &opts(&x.env))
}

fn pay(x: &Ctx, id: u64, amount: i128) {
    let c = SplitContractClient::new(&x.env, &x.contract);
    c.pay(&x.payer, &id, &amount, &0_u64, &false, &false, &None);
}

fn has_p<K: IntoVal<Env, Val>>(env: &Env, k: K) -> bool {
    env.storage().persistent().has(&k)
}

fn has_i<K: IntoVal<Env, Val>>(env: &Env, k: K) -> bool {
    env.storage().instance().has(&k)
}

/// Deterministic JSON (sorted keys) of which probed keys exist.
fn layout(x: &Ctx, id: u64) -> StdString {
    let env = &x.env;
    let mut rows: StdVec<(&str, bool)> = env.as_contract(&x.contract, || {
        std::vec![
            ("instance:counter_key", has_i(env, counter_key())),
            ("instance:total_invoices_key", has_i(env, total_invoices_key())),
            ("instance:total_volume_key", has_i(env, total_volume_key())),
            ("instance:total_released_key", has_i(env, total_released_key())),
            ("instance:total_refunded_key", has_i(env, total_refunded_key())),
            ("persistent:invoice_key", has_p(env, invoice_key(id))),
            ("persistent:invoice_ext_key", has_p(env, invoice_ext_key(id))),
            ("persistent:invoice_tags_key", has_p(env, invoice_tags_key(id))),
            ("persistent:audit_log_key", has_p(env, audit_log_key(id))),
            (
                "persistent:cumulative_contributed_key",
                has_p(env, cumulative_contributed_key(id))
            ),
            (
                "persistent:contribution_key",
                has_p(env, contribution_key(id, &x.payer))
            ),
            (
                "persistent:payer_history_key",
                has_p(env, payer_history_key(&x.payer))
            ),
            (
                "persistent:top_contributors_key",
                has_p(env, top_contributors_key(id))
            ),
            (
                "persistent:invoice_count_key",
                has_p(env, invoice_count_key(&x.creator))
            ),
            (
                "persistent:open_invoice_count_key",
                has_p(env, open_invoice_count_key(&x.creator))
            ),
            (
                "persistent:creator_stats_count_key",
                has_p(env, creator_stats_count_key(&x.creator))
            ),
            (
                "persistent:cancel_count_key",
                has_p(env, cancel_count_key(&x.creator))
            ),
            (
                "persistent:invoice_group_id_key",
                has_p(env, invoice_group_id_key(id))
            ),
            (
                "persistent:subscription_params_key",
                has_p(env, subscription_params_key(id))
            ),
        ]
    });
    rows.sort_by(|a, b| a.0.cmp(b.0));
    let mut out = StdString::from("{\n");
    for (i, (k, v)) in rows.iter().enumerate() {
        out.push_str(&std::format!("  \"{}\": {}", k, v));
        out.push_str(if i + 1 < rows.len() { ",\n" } else { "\n" });
    }
    out.push_str("}\n");
    out
}

fn assert_snapshot(name: &str, actual: &str) {
    let path = std::format!(
        "{}/../../tests/snapshots/ops_{}.json",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    let update = std::env::var("UPDATE_SNAPSHOTS").is_ok();
    if update || !std::path::Path::new(&path).exists() {
        std::fs::write(&path, actual).expect("write snapshot");
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
    assert_eq!(
        expected, actual,
        "storage layout changed for `{}`; if intentional rerun with UPDATE_SNAPSHOTS=1",
        name
    );
}

#[test]
fn snapshot_create_invoice() {
    let x = setup();
    let id = create(&x, 100);
    assert_snapshot("create_invoice", &layout(&x, id));
}

#[test]
fn snapshot_pay() {
    let x = setup();
    let id = create(&x, 100);
    pay(&x, id, 40);
    assert_snapshot("pay", &layout(&x, id));
}

#[test]
fn snapshot_release() {
    let x = setup();
    let id = create(&x, 100);
    pay(&x, id, 100);
    SplitContractClient::new(&x.env, &x.contract).release(&id);
    assert_snapshot("release", &layout(&x, id));
}

#[test]
fn snapshot_refund() {
    let x = setup();
    let id = create(&x, 100);
    pay(&x, id, 40);
    x.env.ledger().set_timestamp(5_000);
    SplitContractClient::new(&x.env, &x.contract).refund(&id);
    assert_snapshot("refund", &layout(&x, id));
}

#[test]
fn snapshot_cancel() {
    let x = setup();
    let id = create(&x, 100);
    SplitContractClient::new(&x.env, &x.contract).cancel_invoice(&x.creator, &id);
    assert_snapshot("cancel", &layout(&x, id));
}

#[test]
fn snapshot_add_recipient() {
    let x = setup();
    let id = create(&x, 100);
    SplitContractClient::new(&x.env, &x.contract).add_recipient(
        &x.creator,
        &id,
        &Address::generate(&x.env),
        &50,
    );
    assert_snapshot("add_recipient", &layout(&x, id));
}

#[test]
fn snapshot_clone_invoice() {
    let x = setup();
    let id = create(&x, 100);
    let c = SplitContractClient::new(&x.env, &x.contract);
    let cloned = c.clone_invoice(
        &x.creator,
        &id,
        &types::CloneOverrides {
            new_deadline: None,
            new_amounts: None,
            new_recipients: None,
            new_overflow_behavior: None,
            new_metadata_hash: None,
        },
    );
    assert_snapshot("clone_invoice", &layout(&x, cloned));
}
