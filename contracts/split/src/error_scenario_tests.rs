#![cfg(test)]

//! Error scenario testing (issue #1023).
//!
//! Every scenario injects one invalid operation against a known-good invoice
//! and checks that:
//! 1. the operation is rejected (**error injection**),
//! 2. the invoice is left exactly as it was (**no partial state**), and
//! 3. a valid operation still succeeds afterwards (**recovery**).
//!
//! Outcomes are collected in an [`ErrorReport`] (**reporting**) that is
//! grouped by [`ErrorCategory`] (**analytics**). The scenario catalogue is
//! documented in `docs/ISSUE_1023_ERROR_SCENARIOS.md`.

extern crate std;

use crate::test::default_options;
use crate::types::InvoiceStatus;
use crate::{SplitContract, SplitContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, Vec,
};

const DEADLINE: u64 = 2_000;
const AMOUNT: i128 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ErrorCategory {
    InvalidAmount,
    InvalidInput,
    #[allow(dead_code)]
    InvalidState,
    Timing,
    NotFound,
}

struct Scenario {
    name: &'static str,
    category: ErrorCategory,
    /// Injects the error; returns `true` when the contract rejected it.
    inject: fn(&Fixture) -> bool,
}

#[derive(Default)]
struct ErrorReport {
    rejected: std::vec::Vec<&'static str>,
    accepted: std::vec::Vec<&'static str>,
    recovered: u32,
    by_category: std::vec::Vec<(ErrorCategory, u32)>,
}

impl ErrorReport {
    fn record(&mut self, s: &Scenario, rejected: bool, recovered: bool) {
        if rejected {
            self.rejected.push(s.name);
        } else {
            self.accepted.push(s.name);
        }
        if recovered {
            self.recovered += 1;
        }
        match self.by_category.iter_mut().find(|(c, _)| *c == s.category) {
            Some((_, n)) => *n += 1,
            None => self.by_category.push((s.category, 1)),
        }
    }

    fn count(&self, category: ErrorCategory) -> u32 {
        self.by_category.iter().find(|(c, _)| *c == category).map(|(_, n)| *n).unwrap_or(0)
    }
}

struct Fixture<'a> {
    env: &'a Env,
    c: SplitContractClient<'a>,
    token: Address,
    id: u64,
    creator: Address,
    payer: Address,
}

fn fixture(env: &Env) -> Fixture<'_> {
    env.mock_all_auths();
    let contract_id = env.register(SplitContract, ());
    let token_admin = Address::generate(env);
    let token = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    StellarAssetClient::new(env, &token).mint(&token_admin, &1_000_000);

    let c = SplitContractClient::new(env, &contract_id);
    c.initialize(
        &Address::generate(env),
        &0_i128,
        &Address::generate(env),
        &token,
        &0_u32,
        &None,
        &0_u32,
        &0_u32,
        &0_u64,
    );
    env.ledger().set_timestamp(1_000);

    let creator = Address::generate(env);
    let mut recipients = Vec::new(env);
    recipients.push_back(Address::generate(env));
    let mut amounts = Vec::new(env);
    amounts.push_back(AMOUNT);
    let id = c.create_invoice(&creator, &recipients, &amounts, &token, &DEADLINE, &default_options(env));

    let payer = Address::generate(env);
    StellarAssetClient::new(env, &token).mint(&payer, &10_000);

    Fixture { env, c, token, id, creator, payer }
}

fn pay(c: &SplitContractClient, payer: &Address, id: u64, amount: i128) -> bool {
    c.try_pay(payer, &id, &amount, &0_u64, &false, &false, &None).is_ok()
}

fn scenarios() -> [Scenario; 8] {
    [
        Scenario {
            name: "pay zero",
            category: ErrorCategory::InvalidAmount,
            inject: |f| !pay(&f.c, &f.payer, f.id, 0),
        },
        Scenario {
            name: "pay negative",
            category: ErrorCategory::InvalidAmount,
            inject: |f| !pay(&f.c, &f.payer, f.id, -1),
        },
        Scenario {
            name: "pay more than remaining",
            category: ErrorCategory::InvalidAmount,
            inject: |f| !pay(&f.c, &f.payer, f.id, AMOUNT + 1),
        },
        Scenario {
            name: "pay after deadline",
            category: ErrorCategory::Timing,
            inject: |f| {
                f.env.ledger().set_timestamp(DEADLINE + 1);
                let rejected = !pay(&f.c, &f.payer, f.id, 10);
                f.env.ledger().set_timestamp(1_000);
                rejected
            },
        },
        Scenario {
            name: "refund before deadline",
            category: ErrorCategory::Timing,
            inject: |f| f.c.try_refund(&f.id).is_err(),
        },
        Scenario {
            name: "create invoice with zero amount",
            category: ErrorCategory::InvalidInput,
            inject: |f| {
                let mut recipients = Vec::new(f.env);
                recipients.push_back(Address::generate(f.env));
                let mut amounts = Vec::new(f.env);
                amounts.push_back(0_i128);
                f.c.try_create_invoice(&f.creator, &recipients, &amounts, &f.token, &DEADLINE, &default_options(f.env))
                    .is_err()
            },
        },
        Scenario {
            name: "create invoice with mismatched recipients and amounts",
            category: ErrorCategory::InvalidInput,
            inject: |f| {
                let mut recipients = Vec::new(f.env);
                recipients.push_back(Address::generate(f.env));
                let amounts: Vec<i128> = Vec::new(f.env);
                f.c.try_create_invoice(&f.creator, &recipients, &amounts, &f.token, &DEADLINE, &default_options(f.env))
                    .is_err()
            },
        },
        Scenario {
            name: "pay an unknown invoice",
            category: ErrorCategory::NotFound,
            inject: |f| !pay(&f.c, &f.payer, f.id + 1_000, 10),
        },
    ]
}

#[test]
fn injected_errors_are_rejected_leave_state_intact_and_recover() {
    let mut report = ErrorReport::default();

    for scenario in scenarios().iter() {
        let env = Env::default();
        let f = fixture(&env);
        let before = f.c.get_invoice(&f.id);

        let rejected = (scenario.inject)(&f);

        let after = f.c.get_invoice(&f.id);
        assert_eq!(after.funded, before.funded, "{}: funded changed", scenario.name);
        assert_eq!(after.status, before.status, "{}: status changed", scenario.name);

        // Recovery: a valid payment still completes the invoice.
        let recovered = pay(&f.c, &f.payer, f.id, AMOUNT)
            && f.c.get_invoice(&f.id).status == InvoiceStatus::Released;

        report.record(scenario, rejected, recovered);
    }

    assert!(report.accepted.is_empty(), "errors that were not rejected: {:?}", report.accepted);
    assert_eq!(report.rejected.len(), scenarios().len());
    assert_eq!(report.recovered as usize, scenarios().len());
}

#[test]
fn error_analytics_group_scenarios_by_category() {
    let mut report = ErrorReport::default();
    for scenario in scenarios().iter() {
        report.record(scenario, true, true);
    }

    assert_eq!(report.count(ErrorCategory::InvalidAmount), 3);
    assert_eq!(report.count(ErrorCategory::Timing), 2);
    assert_eq!(report.count(ErrorCategory::InvalidState), 0);
    assert_eq!(report.count(ErrorCategory::InvalidInput), 2);
    assert_eq!(report.count(ErrorCategory::NotFound), 1);
    let total: u32 = report.by_category.iter().map(|(_, n)| *n).sum();
    assert_eq!(total as usize, scenarios().len());
}

#[test]
fn cancel_after_full_payment_is_rejected_and_state_is_kept() {
    let env = Env::default();
    let f = fixture(&env);
    assert!(pay(&f.c, &f.payer, f.id, AMOUNT));
    assert_eq!(f.c.get_invoice(&f.id).status, InvoiceStatus::Released);

    assert!(f.c.try_cancel_invoice(&f.creator, &f.id).is_err());
    assert_eq!(f.c.get_invoice(&f.id).status, InvoiceStatus::Released);
}

#[test]
fn double_refund_is_rejected() {
    let env = Env::default();
    let f = fixture(&env);
    assert!(pay(&f.c, &f.payer, f.id, 40));

    env.ledger().set_timestamp(DEADLINE + 1);
    assert!(f.c.try_refund(&f.id).is_ok());
    assert!(f.c.try_refund(&f.id).is_err());
    assert_eq!(f.c.get_invoice(&f.id).status, InvoiceStatus::Refunded);
}
