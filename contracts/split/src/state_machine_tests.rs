#![cfg(test)]

//! State machine testing (#1024): maps the invoice lifecycle, exercises every
//! transition, checks that guards reject invalid ones, and reports coverage
//! and per-transition analytics.

use crate::testing_support::*;
use crate::types::InvoiceStatus;
use soroban_sdk::{testutils::{Address as _, Ledger}, Address};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Action {
    PartialPay,
    FullPay,
    Cancel,
    Refund,
}

type Transition = (&'static str, Action, &'static str);

/// The state machine under test: `(from, action, to)`.
const TRANSITIONS: [Transition; 4] = [
    ("Pending", Action::PartialPay, "Pending"),
    ("Pending", Action::FullPay, "Released"),
    ("Pending", Action::Cancel, "Cancelled"),
    ("Pending", Action::Refund, "Refunded"),
];

/// Actions that must be rejected from each terminal state.
const GUARDED: [(&str, Action); 6] = [
    ("Released", Action::Cancel),
    ("Released", Action::Refund),
    ("Cancelled", Action::FullPay),
    ("Cancelled", Action::Refund),
    ("Refunded", Action::Cancel),
    ("Refunded", Action::FullPay),
];

fn name(status: &InvoiceStatus) -> &'static str {
    match status {
        InvoiceStatus::Pending => "Pending",
        InvoiceStatus::Released => "Released",
        InvoiceStatus::Cancelled => "Cancelled",
        InvoiceStatus::Refunded => "Refunded",
        _ => "Other",
    }
}

/// Records visited transitions to compute coverage and analytics.
#[derive(Default)]
struct StateCoverage {
    hits: BTreeMap<(&'static str, Action, &'static str), u32>,
    guard_hits: BTreeMap<(&'static str, Action), u32>,
}

impl StateCoverage {
    fn record(&mut self, t: Transition) {
        *self.hits.entry(t).or_insert(0) += 1;
    }

    fn record_guard(&mut self, g: (&'static str, Action)) {
        *self.guard_hits.entry(g).or_insert(0) += 1;
    }

    fn transition_coverage_pct(&self) -> u32 {
        let covered = TRANSITIONS.iter().filter(|t| self.hits.contains_key(*t)).count();
        (covered * 100 / TRANSITIONS.len()) as u32
    }

    fn guard_coverage_pct(&self) -> u32 {
        let covered = GUARDED.iter().filter(|g| self.guard_hits.contains_key(*g)).count();
        (covered * 100 / GUARDED.len()) as u32
    }
}

/// Drives an invoice of 100 stroops to the requested state. Returns the
/// environment pieces needed to attempt further actions.
fn drive_to<'a>(
    env: &'a soroban_sdk::Env,
    c: &crate::SplitContractClient<'a>,
    token_id: &Address,
    target: &str,
) -> (u64, Address, Address) {
    let (id, creator, _recipient) = single_invoice(env, c, token_id, 100, 2_000);
    let payer = Address::generate(env);
    fund(env, token_id, &payer, 1_000);
    match target {
        "Pending" => {}
        "Released" => assert!(try_pay(c, &payer, id, 100)),
        "Cancelled" => c.cancel_invoice(&creator, &id),
        "Refunded" => {
            assert!(try_pay(c, &payer, id, 40));
            env.ledger().set_timestamp(3_000);
            c.refund(&id);
        }
        other => panic!("unknown state {other}"),
    }
    (id, creator, payer)
}

#[test]
fn every_transition_reaches_expected_state() {
    let mut cov = StateCoverage::default();

    for t in TRANSITIONS {
        let (env, contract_id, token_id) = setup();
        let c = client(&env, &contract_id);
        let (id, creator, payer) = drive_to(&env, &c, &token_id, t.0);
        assert_eq!(name(&c.get_invoice(&id).status), t.0, "precondition for {:?}", t);

        match t.1 {
            Action::PartialPay => assert!(try_pay(&c, &payer, id, 40)),
            Action::FullPay => assert!(try_pay(&c, &payer, id, 100)),
            Action::Cancel => c.cancel_invoice(&creator, &id),
            Action::Refund => {
                assert!(try_pay(&c, &payer, id, 40));
                env.ledger().set_timestamp(3_000);
                c.refund(&id);
            }
        }

        assert_eq!(name(&c.get_invoice(&id).status), t.2, "transition {:?}", t);
        cov.record(t);
    }

    assert_eq!(cov.transition_coverage_pct(), 100);
}

#[test]
fn guards_reject_invalid_transitions() {
    let mut cov = StateCoverage::default();

    for g in GUARDED {
        let (env, contract_id, token_id) = setup();
        let c = client(&env, &contract_id);
        let (id, creator, payer) = drive_to(&env, &c, &token_id, g.0);
        let before = c.get_invoice(&id);

        let rejected = match g.1 {
            Action::Cancel => c.try_cancel_invoice(&creator, &id).is_err(),
            Action::Refund => c.try_refund(&id).is_err(),
            Action::FullPay | Action::PartialPay => !try_pay(&c, &payer, id, 10),
        };
        assert!(rejected, "{:?} must be rejected", g);

        let after = c.get_invoice(&id);
        assert_eq!(name(&after.status), g.0, "guard must not change state");
        assert_eq!(after.funded, before.funded, "guard must not move funds");
        cov.record_guard(g);
    }

    assert_eq!(cov.guard_coverage_pct(), 100);
}

#[test]
fn terminal_states_are_absorbing() {
    for terminal in ["Released", "Cancelled", "Refunded"] {
        let (env, contract_id, token_id) = setup();
        let c = client(&env, &contract_id);
        let (id, creator, payer) = drive_to(&env, &c, &token_id, terminal);

        let _ = c.try_cancel_invoice(&creator, &id);
        let _ = c.try_refund(&id);
        let _ = try_pay(&c, &payer, id, 10);

        assert_eq!(name(&c.get_invoice(&id).status), terminal);
    }
}

#[test]
fn transition_analytics_are_reported() {
    let mut cov = StateCoverage::default();
    for t in TRANSITIONS {
        cov.record(t);
        cov.record(t);
    }

    let total: u32 = cov.hits.values().sum();
    assert_eq!(total, (TRANSITIONS.len() as u32) * 2);
    for t in TRANSITIONS {
        assert_eq!(cov.hits[&t], 2);
    }
    assert_eq!(cov.transition_coverage_pct(), 100);
    assert_eq!(cov.guard_coverage_pct(), 0);
}
