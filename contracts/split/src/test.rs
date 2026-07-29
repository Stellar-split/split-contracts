#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Deploy the split contract and a mock USDC token.
/// Returns `(env, contract_id, token_id)`.
fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SplitContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let stellar_asset = StellarAssetClient::new(&env, &token_id);
    stellar_asset.mint(&token_admin, &1_000_000_000);

    (env, contract_id, token_id)
}

fn client<'a>(env: &'a Env, contract_id: &Address) -> SplitContractClient<'a> {
    SplitContractClient::new(env, contract_id)
}

fn token_client<'a>(env: &'a Env, token_id: &Address) -> TokenClient<'a> {
    TokenClient::new(env, token_id)
}

/// Mint `amount` tokens to `addr`.
fn mint(env: &Env, token_id: &Address, addr: &Address, amount: i128) {
    StellarAssetClient::new(env, token_id).mint(addr, &amount);
}

/// Build a soroban Vec<Address> from a slice.
fn addrs(env: &Env, list: &[Address]) -> Vec<Address> {
    let mut v = Vec::new(env);
    for a in list {
        v.push_back(a.clone());
    }
    v
}

/// Build a soroban Vec<i128> from a slice.
fn amounts(env: &Env, list: &[i128]) -> Vec<i128> {
    let mut v = Vec::new(env);
    for &a in list {
        v.push_back(a);
    }
    v
}

// ---------------------------------------------------------------------------
// Original tests (updated for new `pay` signature with `treasury` param)
// ---------------------------------------------------------------------------

#[test]
fn test_create_invoice() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = c.create_invoice(
        &creator,
        &addrs(&env, &[recipient]),
        &amounts(&env, &[100]),
        &token_id,
        &2_000_u64,
        &None,
        &0_u32,
    );
    assert_eq!(id, 1);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Pending);
    assert_eq!(invoice.funded, 0);
    assert_eq!(invoice.parent_invoice_id, None);
    assert_eq!(invoice.late_penalty_bps, 0);
}

#[test]
fn test_pay_and_auto_release() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let treasury = Address::generate(&env);
    mint(&env, &token_id, &payer, 500);

    env.ledger().set_timestamp(1_000);

    let id = c.create_invoice(
        &creator,
        &addrs(&env, &[recipient.clone()]),
        &amounts(&env, &[200]),
        &token_id,
        &9_999_u64,
        &None,
        &0_u32,
    );

    c.pay(&payer, &id, &200_i128, &treasury);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 200);
}

#[test]
fn test_partial_pay_then_release() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);
    let creator = Address::generate(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);
    let recipient = Address::generate(&env);
    let treasury = Address::generate(&env);
    mint(&env, &token_id, &payer1, 150);
    mint(&env, &token_id, &payer2, 150);

    env.ledger().set_timestamp(1_000);

    let id = c.create_invoice(
        &creator,
        &addrs(&env, &[recipient.clone()]),
        &amounts(&env, &[300]),
        &token_id,
        &9_999_u64,
        &None,
        &0_u32,
    );

    c.pay(&payer1, &id, &150_i128, &treasury);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);

    c.pay(&payer2, &id, &150_i128, &treasury);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 300);
}

#[test]
fn test_refund_after_deadline() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let treasury = Address::generate(&env);
    mint(&env, &token_id, &payer, 100);

    env.ledger().set_timestamp(1_000);

    let id = c.create_invoice(
        &creator,
        &addrs(&env, &[recipient]),
        &amounts(&env, &[500]),
        &token_id,
        &2_000_u64,
        &None,
        &0_u32,
    );

    c.pay(&payer, &id, &100_i128, &treasury);

    // Advance past deadline + grace window (86400s)
    env.ledger().set_timestamp(2_000 + 86_400 + 1);

    c.refund(&id);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Refunded);
    assert_eq!(tk.balance(&payer), 100);
}

#[test]
#[should_panic(expected = "invoice deadline has passed")]
fn test_pay_after_deadline_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let treasury = Address::generate(&env);
    mint(&env, &token_id, &payer, 100);

    env.ledger().set_timestamp(1_000);

    let id = c.create_invoice(
        &creator,
        &addrs(&env, &[recipient]),
        &amounts(&env, &[100]),
        &token_id,
        &2_000_u64,
        &None,
        &0_u32,
    );

    // Advance past deadline + grace window
    env.ledger().set_timestamp(2_000 + 86_400 + 1);
    c.pay(&payer, &id, &100_i128, &treasury);
}

#[test]
#[should_panic(expected = "payment exceeds remaining balance")]
fn test_overpayment_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let treasury = Address::generate(&env);
    mint(&env, &token_id, &payer, 1_000);

    env.ledger().set_timestamp(1_000);

    let id = c.create_invoice(
        &creator,
        &addrs(&env, &[recipient]),
        &amounts(&env, &[100]),
        &token_id,
        &9_999_u64,
        &None,
        &0_u32,
    );
    c.pay(&payer, &id, &200_i128, &treasury);
}

#[test]
fn test_multi_recipient_release() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let r3 = Address::generate(&env);
    let treasury = Address::generate(&env);
    mint(&env, &token_id, &payer, 600);

    env.ledger().set_timestamp(1_000);

    let id = c.create_invoice(
        &creator,
        &addrs(&env, &[r1.clone(), r2.clone(), r3.clone()]),
        &amounts(&env, &[100, 200, 300]),
        &token_id,
        &9_999_u64,
        &None,
        &0_u32,
    );
    c.pay(&payer, &id, &600_i128, &treasury);

    assert_eq!(tk.balance(&r1), 100);
    assert_eq!(tk.balance(&r2), 200);
    assert_eq!(tk.balance(&r3), 300);
}

// ---------------------------------------------------------------------------
// #522 — Cross-Invoice Split Linkage
// ---------------------------------------------------------------------------

/// A child invoice without a parent releases normally.
#[test]
fn test_522_no_parent_releases_normally() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let treasury = Address::generate(&env);
    mint(&env, &token_id, &payer, 100);

    env.ledger().set_timestamp(1_000);

    let id = c.create_invoice(
        &creator,
        &addrs(&env, &[recipient.clone()]),
        &amounts(&env, &[100]),
        &token_id,
        &9_999_u64,
        &None,
        &0_u32,
    );

    c.pay(&payer, &id, &100_i128, &treasury);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 100);
}

/// A child invoice cannot be released before the parent is released.
/// Paying a child fully triggers auto-release which checks the parent gate.
#[test]
#[should_panic]
fn test_522_child_blocked_until_parent_released() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let treasury = Address::generate(&env);
    mint(&env, &token_id, &payer, 1_000);

    env.ledger().set_timestamp(1_000);

    // Create parent invoice (stays Pending — never funded).
    let parent_id = c.create_invoice(
        &creator,
        &addrs(&env, &[r1.clone()]),
        &amounts(&env, &[100]),
        &token_id,
        &9_999_u64,
        &None,
        &0_u32,
    );

    // Create child invoice linked to parent.
    let child_id = c.create_invoice(
        &creator,
        &addrs(&env, &[r2.clone()]),
        &amounts(&env, &[200]),
        &token_id,
        &9_999_u64,
        &Some(parent_id),
        &0_u32,
    );

    // Fully fund the child — auto-release checks parent and should panic
    // with ParentInvoiceNotFinalised because parent is still Pending.
    c.pay(&payer, &child_id, &200_i128, &treasury);
}

/// A child invoice releases successfully after the parent is released.
#[test]
fn test_522_child_releases_after_parent_finalised() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let treasury = Address::generate(&env);
    mint(&env, &token_id, &payer, 1_000);

    env.ledger().set_timestamp(1_000);

    // Create parent and fully pay it (auto-releases).
    let parent_id = c.create_invoice(
        &creator,
        &addrs(&env, &[r1.clone()]),
        &amounts(&env, &[100]),
        &token_id,
        &9_999_u64,
        &None,
        &0_u32,
    );
    c.pay(&payer, &parent_id, &100_i128, &treasury);
    assert_eq!(c.get_invoice(&parent_id).status, InvoiceStatus::Released);

    // Create child linked to parent (already released).
    let child_id = c.create_invoice(
        &creator,
        &addrs(&env, &[r2.clone()]),
        &amounts(&env, &[200]),
        &token_id,
        &9_999_u64,
        &Some(parent_id),
        &0_u32,
    );

    // Fully fund child — should auto-release since parent is already Released.
    c.pay(&payer, &child_id, &200_i128, &treasury);
    assert_eq!(c.get_invoice(&child_id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&r2), 200);
}

/// Parent chain depth exceeding MAX_PARENT_DEPTH is rejected at creation.
#[test]
#[should_panic]
fn test_522_parent_chain_too_deep() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let creator = Address::generate(&env);
    let r = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    // Build a chain of 11 invoices (depth 10 = MAX_PARENT_DEPTH).
    let mut prev_id: Option<u64> = None;
    for _ in 0..11 {
        let id = c.create_invoice(
            &creator,
            &addrs(&env, &[r.clone()]),
            &amounts(&env, &[10]),
            &token_id,
            &9_999_u64,
            &prev_id,
            &0_u32,
        );
        prev_id = Some(id);
    }

    // One more link should exceed MAX_PARENT_DEPTH and panic.
    c.create_invoice(
        &creator,
        &addrs(&env, &[r.clone()]),
        &amounts(&env, &[10]),
        &token_id,
        &9_999_u64,
        &prev_id,
        &0_u32,
    );
}

/// Referencing a non-existent parent invoice is rejected.
#[test]
#[should_panic(expected = "invoice not found")]
fn test_522_invalid_parent_rejected() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let creator = Address::generate(&env);
    let r = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    c.create_invoice(
        &creator,
        &addrs(&env, &[r]),
        &amounts(&env, &[100]),
        &token_id,
        &9_999_u64,
        &Some(999_u64), // non-existent
        &0_u32,
    );
}

/// parent_invoice_id is stored on the Invoice struct.
#[test]
fn test_522_parent_id_stored() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let creator = Address::generate(&env);
    let r = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let parent_id = c.create_invoice(
        &creator,
        &addrs(&env, &[r.clone()]),
        &amounts(&env, &[50]),
        &token_id,
        &9_999_u64,
        &None,
        &0_u32,
    );

    let child_id = c.create_invoice(
        &creator,
        &addrs(&env, &[r]),
        &amounts(&env, &[50]),
        &token_id,
        &9_999_u64,
        &Some(parent_id),
        &0_u32,
    );

    let child = c.get_invoice(&child_id);
    assert_eq!(child.parent_invoice_id, Some(parent_id));
}

// ---------------------------------------------------------------------------
// #523 — Late Payment Penalty Fee
// ---------------------------------------------------------------------------

/// On-time contribution pays no penalty; treasury balance unchanged.
#[test]
fn test_523_ontime_no_penalty() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let treasury = Address::generate(&env);
    let tk = token_client(&env, &token_id);
    mint(&env, &token_id, &payer, 1_000);

    env.ledger().set_timestamp(1_000);

    let id = c.create_invoice(
        &creator,
        &addrs(&env, &[recipient.clone()]),
        &amounts(&env, &[500]),
        &token_id,
        &9_000_u64,
        &None,
        &100_u32, // 1% penalty bps
    );

    // Pay well before deadline — no penalty.
    c.pay(&payer, &id, &500_i128, &treasury);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    // Treasury receives nothing for an on-time payment.
    assert_eq!(tk.balance(&treasury), 0);
    // Recipient receives full amount.
    assert_eq!(tk.balance(&recipient), 500);
}

/// Grace-window contribution triggers penalty; treasury receives penalty amount.
#[test]
fn test_523_late_penalty_charged() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let treasury = Address::generate(&env);
    // payer needs principal + penalty headroom
    mint(&env, &token_id, &payer, 2_000);

    env.ledger().set_timestamp(1_000);
    let deadline: u64 = 5_000;

    // 100 bps = 1% penalty
    let id = c.create_invoice(
        &creator,
        &addrs(&env, &[recipient.clone()]),
        &amounts(&env, &[1_000]),
        &token_id,
        &deadline,
        &None,
        &100_u32,
    );

    // Advance into grace window (after deadline, before deadline + 86400).
    env.ledger().set_timestamp(deadline + 1);

    c.pay(&payer, &id, &1_000_i128, &treasury);

    // Expected penalty: ceil(1000 * 100 / 10000) = ceil(10.00) = 10
    let expected_penalty: i128 = 10;

    assert_eq!(tk.balance(&treasury), expected_penalty);
    // Invoice should be released (fully funded).
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
}

/// late_penalty_bps = 0 means grace-window contribution is penalty-free.
#[test]
fn test_523_zero_penalty_bps_no_charge() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let treasury = Address::generate(&env);
    mint(&env, &token_id, &payer, 1_000);

    env.ledger().set_timestamp(1_000);
    let deadline: u64 = 5_000;

    let id = c.create_invoice(
        &creator,
        &addrs(&env, &[recipient.clone()]),
        &amounts(&env, &[500]),
        &token_id,
        &deadline,
        &None,
        &0_u32, // no penalty
    );

    // Pay in grace window.
    env.ledger().set_timestamp(deadline + 100);
    c.pay(&payer, &id, &500_i128, &treasury);

    // Treasury gets nothing.
    assert_eq!(tk.balance(&treasury), 0);
    assert_eq!(tk.balance(&recipient), 500);
}

/// Penalty is calculated correctly for various bps values.
#[test]
fn test_523_penalty_calculation() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let treasury = Address::generate(&env);
    mint(&env, &token_id, &payer, 10_000);

    env.ledger().set_timestamp(1_000);
    let deadline: u64 = 5_000;

    // 500 bps = 5% penalty on 1000 → 50
    let id = c.create_invoice(
        &creator,
        &addrs(&env, &[recipient.clone()]),
        &amounts(&env, &[1_000]),
        &token_id,
        &deadline,
        &None,
        &500_u32,
    );

    env.ledger().set_timestamp(deadline + 1);
    c.pay(&payer, &id, &1_000_i128, &treasury);

    // ceil(1000 * 500 / 10000) = ceil(50.0) = 50
    assert_eq!(tk.balance(&treasury), 50);
}

/// late_penalty_bps is stored on the Invoice struct.
#[test]
fn test_523_penalty_bps_stored() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let creator = Address::generate(&env);
    let r = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = c.create_invoice(
        &creator,
        &addrs(&env, &[r]),
        &amounts(&env, &[100]),
        &token_id,
        &9_999_u64,
        &None,
        &250_u32,
    );

    assert_eq!(c.get_invoice(&id).late_penalty_bps, 250);
}

// ---------------------------------------------------------------------------
// #524 — Invoice Batch Creation
// ---------------------------------------------------------------------------

/// Batch creates multiple invoices and returns IDs in order.
#[test]
fn test_524_batch_creates_invoices_in_order() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let creator = Address::generate(&env);
    let r = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let mut batch = Vec::new(&env);
    for i in 0..3u32 {
        batch.push_back(types::InvoiceParams {
            creator: creator.clone(),
            recipients: addrs(&env, &[r.clone()]),
            amounts: amounts(&env, &[100 + i as i128]),
            token: token_id.clone(),
            deadline: 9_999_u64,
            parent_invoice_id: None,
            late_penalty_bps: 0,
        });
    }

    let ids = c.batch_create_invoices(&batch);

    assert_eq!(ids.len(), 3);
    // IDs should be consecutive and in order.
    let id0 = ids.get(0).unwrap();
    let id1 = ids.get(1).unwrap();
    let id2 = ids.get(2).unwrap();
    assert_eq!(id1, id0 + 1);
    assert_eq!(id2, id0 + 2);

    // Each invoice exists and has correct amounts.
    assert_eq!(c.get_invoice(&id0).amounts.get(0).unwrap(), 100);
    assert_eq!(c.get_invoice(&id1).amounts.get(0).unwrap(), 101);
    assert_eq!(c.get_invoice(&id2).amounts.get(0).unwrap(), 102);
}

/// Batch with a single invoice works.
#[test]
fn test_524_batch_single_invoice() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let creator = Address::generate(&env);
    let r = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let mut batch = Vec::new(&env);
    batch.push_back(types::InvoiceParams {
        creator: creator.clone(),
        recipients: addrs(&env, &[r]),
        amounts: amounts(&env, &[50]),
        token: token_id.clone(),
        deadline: 9_999_u64,
        parent_invoice_id: None,
        late_penalty_bps: 0,
    });

    let ids = c.batch_create_invoices(&batch);
    assert_eq!(ids.len(), 1);
    let invoice = c.get_invoice(&ids.get(0).unwrap());
    assert_eq!(invoice.status, InvoiceStatus::Pending);
    assert_eq!(invoice.funded, 0);
}

/// Batch exceeding MAX_BATCH_SIZE panics with BatchTooLarge.
#[test]
#[should_panic]
fn test_524_batch_too_large_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let creator = Address::generate(&env);
    let r = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let mut batch = Vec::new(&env);
    // MAX_BATCH_SIZE = 50, push 51.
    for _ in 0..51u32 {
        batch.push_back(types::InvoiceParams {
            creator: creator.clone(),
            recipients: addrs(&env, &[r.clone()]),
            amounts: amounts(&env, &[10]),
            token: token_id.clone(),
            deadline: 9_999_u64,
            parent_invoice_id: None,
            late_penalty_bps: 0,
        });
    }

    c.batch_create_invoices(&batch);
}

/// Batch with an empty Vec panics.
#[test]
#[should_panic(expected = "batch must not be empty")]
fn test_524_empty_batch_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    env.ledger().set_timestamp(1_000);
    let batch: Vec<types::InvoiceParams> = Vec::new(&env);
    c.batch_create_invoices(&batch);
}

/// Each invoice in a batch is independently payable and releasable.
#[test]
fn test_524_batch_invoices_independently_payable() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let treasury = Address::generate(&env);
    mint(&env, &token_id, &payer, 1_000);

    env.ledger().set_timestamp(1_000);

    let mut batch = Vec::new(&env);
    batch.push_back(types::InvoiceParams {
        creator: creator.clone(),
        recipients: addrs(&env, &[r1.clone()]),
        amounts: amounts(&env, &[100]),
        token: token_id.clone(),
        deadline: 9_999_u64,
        parent_invoice_id: None,
        late_penalty_bps: 0,
    });
    batch.push_back(types::InvoiceParams {
        creator: creator.clone(),
        recipients: addrs(&env, &[r2.clone()]),
        amounts: amounts(&env, &[200]),
        token: token_id.clone(),
        deadline: 9_999_u64,
        parent_invoice_id: None,
        late_penalty_bps: 0,
    });

    let ids = c.batch_create_invoices(&batch);
    let id0 = ids.get(0).unwrap();
    let id1 = ids.get(1).unwrap();

    c.pay(&payer, &id0, &100_i128, &treasury);
    c.pay(&payer, &id1, &200_i128, &treasury);

    assert_eq!(tk.balance(&r1), 100);
    assert_eq!(tk.balance(&r2), 200);
}

// ---------------------------------------------------------------------------
// #525 — Recipient Share Precision Rounding
// ---------------------------------------------------------------------------

/// Distribute 1000 among 3 equal recipients (no remainder).
#[test]
fn test_525_even_split_no_remainder() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let r3 = Address::generate(&env);
    let treasury = Address::generate(&env);
    mint(&env, &token_id, &payer, 300);

    env.ledger().set_timestamp(1_000);

    let id = c.create_invoice(
        &creator,
        &addrs(&env, &[r1.clone(), r2.clone(), r3.clone()]),
        &amounts(&env, &[100, 100, 100]),
        &token_id,
        &9_999_u64,
        &None,
        &0_u32,
    );

    c.pay(&payer, &id, &300_i128, &treasury);

    assert_eq!(tk.balance(&r1), 100);
    assert_eq!(tk.balance(&r2), 100);
    assert_eq!(tk.balance(&r3), 100);
}

/// Distribute 10 among 3 equal recipients — sum must equal 10 exactly.
#[test]
fn test_525_rounding_sum_exact() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let r3 = Address::generate(&env);
    let treasury = Address::generate(&env);
    mint(&env, &token_id, &payer, 10);

    env.ledger().set_timestamp(1_000);

    let id = c.create_invoice(
        &creator,
        &addrs(&env, &[r1.clone(), r2.clone(), r3.clone()]),
        &amounts(&env, &[10, 10, 10]), // equal ratios, total 30, funded 10
        &token_id,
        &9_999_u64,
        &None,
        &0_u32,
    );

    c.pay(&payer, &id, &10_i128, &treasury);

    let b1 = tk.balance(&r1);
    let b2 = tk.balance(&r2);
    let b3 = tk.balance(&r3);
    let total = b1 + b2 + b3;

    // Sum must be exactly 10 (every stroop accounted for).
    assert_eq!(total, 10);
    // No recipient gets more than 1 stroop over another.
    let max = b1.max(b2).max(b3);
    let min = b1.min(b2).min(b3);
    assert!(max - min <= 1);
}

/// Weighted split 1:2:3 over 1000 — largest remainder recipient gets extra.
#[test]
fn test_525_weighted_largest_remainder() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let r3 = Address::generate(&env);
    let treasury = Address::generate(&env);
    mint(&env, &token_id, &payer, 1_000);

    env.ledger().set_timestamp(1_000);

    // amounts 1:2:3 with total 1000 paid
    // denom = 1+2+3 = 6; floors: 166, 333, 500; sum=999; leftover=1
    // remainder(r1) = 1000*1%6 = 4 → highest → gets extra
    let id = c.create_invoice(
        &creator,
        &addrs(&env, &[r1.clone(), r2.clone(), r3.clone()]),
        &amounts(&env, &[1, 2, 3]),
        &token_id,
        &9_999_u64,
        &None,
        &0_u32,
    );

    c.pay(&payer, &id, &1_000_i128, &treasury);

    assert_eq!(tk.balance(&r1), 167);
    assert_eq!(tk.balance(&r2), 333);
    assert_eq!(tk.balance(&r3), 500);
    assert_eq!(tk.balance(&r1) + tk.balance(&r2) + tk.balance(&r3), 1000);
}

/// Single recipient always gets 100% of the funded amount.
#[test]
fn test_525_single_recipient_full_amount() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let treasury = Address::generate(&env);
    mint(&env, &token_id, &payer, 999);

    env.ledger().set_timestamp(1_000);

    let id = c.create_invoice(
        &creator,
        &addrs(&env, &[recipient.clone()]),
        &amounts(&env, &[999]),
        &token_id,
        &9_999_u64,
        &None,
        &0_u32,
    );

    c.pay(&payer, &id, &999_i128, &treasury);
    assert_eq!(tk.balance(&recipient), 999);
}
