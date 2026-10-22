#![cfg(test)]

//! Boundary value testing (#1025): identifies the numeric and temporal limits
//! of invoice creation, payment and fee math, then checks overflow, underflow
//! and edge-case behavior, tracking how many boundaries were exercised.

use crate::calc::{calc_platform_fee, funding_bps};
use crate::test::default_options;
use crate::testing_support::*;
use crate::types::InvoiceStatus;
use soroban_sdk::{testutils::{Address as _, Ledger}, Address, Vec};

/// Tracks identified boundaries against those verified.
#[derive(Default)]
struct BoundaryMetrics {
    identified: u32,
    verified: u32,
}

impl BoundaryMetrics {
    fn check(&mut self, ok: bool, what: &str) {
        self.identified += 1;
        assert!(ok, "boundary failed: {what}");
        self.verified += 1;
    }

    fn assert_complete(&self) {
        assert_eq!(self.identified, self.verified);
        assert!(self.identified > 0);
    }
}

#[test]
fn funding_bps_boundaries() {
    let mut m = BoundaryMetrics::default();
    m.check(funding_bps(0, 1_000) == 0, "nothing funded");
    m.check(funding_bps(1, 1_000_000) == 0, "smallest payment rounds down");
    m.check(funding_bps(999, 1_000) == 9_990, "one below total");
    m.check(funding_bps(1_000, 1_000) == 10_000, "exactly total");
    m.check(funding_bps(1_001, 1_000) == 10_000, "one above total clamps");
    m.check(funding_bps(-1, 1_000) == 0, "negative funded (underflow)");
    m.check(funding_bps(500, 0) == 0, "zero total");
    m.check(funding_bps(500, -1) == 0, "negative total");
    m.check(funding_bps(i128::MAX, i128::MAX) == 10_000, "max/max");
    m.assert_complete();
}

#[test]
fn platform_fee_boundaries() {
    let mut m = BoundaryMetrics::default();
    m.check(calc_platform_fee(0, 500) == Ok(0), "zero funded");
    m.check(calc_platform_fee(1_000, 0) == Ok(0), "zero fee rate");
    m.check(calc_platform_fee(1, 1) == Ok(0), "fee rounds down to zero");
    m.check(calc_platform_fee(10_000, 10_000) == Ok(10_000), "100% fee");
    m.check(calc_platform_fee(10_000, 1) == Ok(1), "smallest non-zero fee");
    m.check(calc_platform_fee(i128::MAX, 0) == Ok(0), "max funded, no fee");
    m.assert_complete();
}

#[test]
fn payment_amount_boundaries() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let payer = soroban_sdk::Address::generate(&env);
    fund(&env, &token_id, &payer, 1_000);
    let mut m = BoundaryMetrics::default();

    let (id, _, _) = single_invoice(&env, &c, &token_id, 100, DEADLINE);

    m.check(!try_pay(&c, &payer, id, 0), "zero payment rejected");
    m.check(!try_pay(&c, &payer, id, -1), "negative payment rejected (underflow)");
    m.check(!try_pay(&c, &payer, id, 101), "overpay by one rejected (overflow)");
    m.check(!try_pay(&c, &payer, id, i128::MAX), "i128::MAX payment rejected");
    m.check(c.get_invoice(&id).funded == 0, "rejected payments leave funded at zero");

    m.check(try_pay(&c, &payer, id, 1), "minimum payment of one accepted");
    m.check(c.get_invoice(&id).funded == 1, "funded is one");
    m.check(!try_pay(&c, &payer, id, 100), "remaining + 1 rejected");
    m.check(try_pay(&c, &payer, id, 99), "exact remainder accepted");
    m.check(c.get_invoice(&id).status == InvoiceStatus::Released, "fully funded releases");
    m.assert_complete();
}

#[test]
fn deadline_boundaries() {
    let mut m = BoundaryMetrics::default();

    // now == deadline is still payable.
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let payer = Address::generate(&env);
    fund(&env, &token_id, &payer, 100);
    let (id, _, _) = single_invoice(&env, &c, &token_id, 50, 2_000);
    env.ledger().set_timestamp(2_000);
    m.check(try_pay(&c, &payer, id, 50), "payment at the deadline accepted");

    // now == deadline + 1 is rejected.
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let payer = Address::generate(&env);
    fund(&env, &token_id, &payer, 100);
    let (id, _, _) = single_invoice(&env, &c, &token_id, 50, 2_000);
    env.ledger().set_timestamp(2_001);
    m.check(!try_pay(&c, &payer, id, 50), "payment one second after deadline rejected");
    m.assert_complete();
}

#[test]
fn invoice_creation_boundaries() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let creator = Address::generate(&env);
    let mut m = BoundaryMetrics::default();

    let one = |amount: i128| {
        let mut recipients = Vec::new(&env);
        recipients.push_back(Address::generate(&env));
        let mut amounts = Vec::new(&env);
        amounts.push_back(amount);
        c.try_create_invoice(&creator, &recipients, &amounts, &token_id, &DEADLINE, &default_options(&env))
            .is_ok()
    };

    m.check(one(1), "smallest amount accepted");
    m.check(!one(0), "zero amount rejected");
    m.check(!one(-1), "negative amount rejected (underflow)");

    // Two recipients whose amounts sum past i128::MAX must not wrap around.
    let mut recipients = Vec::new(&env);
    recipients.push_back(Address::generate(&env));
    recipients.push_back(Address::generate(&env));
    let mut amounts = Vec::new(&env);
    amounts.push_back(i128::MAX);
    amounts.push_back(1);
    let overflowed = c
        .try_create_invoice(&creator, &recipients, &amounts, &token_id, &DEADLINE, &default_options(&env))
        .is_ok();
    m.check(!overflowed, "amount sum overflow rejected");

    // Recipient and amount lengths must match.
    let mut recipients = Vec::new(&env);
    recipients.push_back(Address::generate(&env));
    let amounts: Vec<i128> = Vec::new(&env);
    let mismatched = c
        .try_create_invoice(&creator, &recipients, &amounts, &token_id, &DEADLINE, &default_options(&env))
        .is_ok();
    m.check(!mismatched, "recipient/amount length mismatch rejected");
    m.assert_complete();
}

#[test]
fn refund_boundary_is_strictly_after_deadline() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let payer = Address::generate(&env);
    fund(&env, &token_id, &payer, 100);
    let (id, _, _) = single_invoice(&env, &c, &token_id, 500, 2_000);
    assert!(try_pay(&c, &payer, id, 100));

    env.ledger().set_timestamp(2_000);
    assert!(c.try_refund(&id).is_err(), "refund at the deadline must be rejected");

    env.ledger().set_timestamp(2_001);
    c.refund(&id);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Refunded);
}
