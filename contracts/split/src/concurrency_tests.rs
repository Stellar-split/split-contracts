#![cfg(test)]

//! Concurrency testing (#1026). Soroban executes transactions sequentially,
//! so concurrency is modeled as interleaved operations from many actors
//! against the same invoice within one ledger. The suite checks that every
//! interleaving yields the same outcome (race detection), that every
//! interleaving terminates in a state that can make progress or is final
//! (deadlock detection), and that funds are conserved (data integrity).

use crate::testing_support::*;
use crate::types::InvoiceStatus;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address,
};

/// Counts of operations attempted, accepted and rejected across scenarios.
#[derive(Default, Debug)]
struct ConcurrencyMetrics {
    attempted: u32,
    accepted: u32,
    rejected: u32,
}

impl ConcurrencyMetrics {
    fn record(&mut self, ok: bool) {
        self.attempted += 1;
        if ok {
            self.accepted += 1;
        } else {
            self.rejected += 1;
        }
    }
}

struct Outcome {
    funded: i128,
    status: InvoiceStatus,
    recipient_balance: i128,
    payer_balances: std::vec::Vec<i128>,
    accepted: std::vec::Vec<bool>,
}

/// Run `order` (indices into `amounts`) against a fresh 100-stroop invoice.
fn run_payers(amounts: &[i128], order: &[usize]) -> Outcome {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token(&env, &token_id);
    let (id, _, recipient) = single_invoice(&env, &c, &token_id, 100, DEADLINE);

    let payers: std::vec::Vec<Address> = amounts.iter().map(|_| Address::generate(&env)).collect();
    for p in &payers {
        fund(&env, &token_id, p, 1_000);
    }

    let mut accepted = std::vec![false; amounts.len()];
    for &i in order {
        accepted[i] = try_pay(&c, &payers[i], id, amounts[i]);
    }

    let invoice = c.get_invoice(&id);
    Outcome {
        funded: invoice.funded,
        status: invoice.status,
        recipient_balance: tk.balance(&recipient),
        payer_balances: payers.iter().map(|p| tk.balance(p)).collect(),
        accepted,
    }
}

#[test]
fn interleaved_payers_fully_fund_invoice_in_any_order() {
    let amounts = [10, 20, 30, 40];
    let orders: [[usize; 4]; 4] = [[0, 1, 2, 3], [3, 2, 1, 0], [1, 3, 0, 2], [2, 0, 3, 1]];
    let mut metrics = ConcurrencyMetrics::default();

    for order in orders {
        let out = run_payers(&amounts, &order);
        for a in &out.accepted {
            metrics.record(*a);
        }
        assert_eq!(out.status, InvoiceStatus::Released);
        assert_eq!(out.recipient_balance, 100);
        for (i, bal) in out.payer_balances.iter().enumerate() {
            assert_eq!(*bal, 1_000 - amounts[i]);
        }
    }

    assert_eq!(metrics.attempted, 16);
    assert_eq!(metrics.rejected, 0);
}

#[test]
fn competing_overpayments_resolve_without_double_spend() {
    // Two payers each try 60 against a 100 invoice: exactly one can win.
    let amounts = [60, 60];
    let mut metrics = ConcurrencyMetrics::default();

    for order in [[0usize, 1], [1, 0]] {
        let out = run_payers(&amounts, &order);
        for a in &out.accepted {
            metrics.record(*a);
        }

        let winners = out.accepted.iter().filter(|a| **a).count();
        assert_eq!(winners, 1, "exactly one competing payment may land");
        assert_eq!(out.funded, 60);
        assert_eq!(out.status, InvoiceStatus::Pending);

        let winner = out.accepted.iter().position(|a| *a);
        // The loser's payment is also 60, which cannot fit; it must keep its funds.
        for (i, bal) in out.payer_balances.iter().enumerate() {
            let expected = if Some(i) == winner { 940 } else { 1_000 };
            assert_eq!(*bal, expected);
        }
    }

    assert_eq!(metrics.accepted, 2);
    assert_eq!(metrics.rejected, 2);
}

#[test]
fn race_between_final_payment_and_cancel_is_order_independent_in_outcome() {
    // Order A: payment lands first, cancel is rejected.
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let (id, creator, recipient) = single_invoice(&env, &c, &token_id, 100, DEADLINE);
    let payer = Address::generate(&env);
    fund(&env, &token_id, &payer, 100);
    assert!(try_pay(&c, &payer, id, 100));
    assert!(c.try_cancel_invoice(&creator, &id).is_err());
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(token(&env, &token_id).balance(&recipient), 100);

    // Order B: cancel lands first, payment is rejected.
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let (id, creator, recipient) = single_invoice(&env, &c, &token_id, 100, DEADLINE);
    let payer = Address::generate(&env);
    fund(&env, &token_id, &payer, 100);
    c.cancel_invoice(&creator, &id);
    assert!(!try_pay(&c, &payer, id, 100));
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Cancelled);
    assert_eq!(token(&env, &token_id).balance(&recipient), 0);
    assert_eq!(token(&env, &token_id).balance(&payer), 100);
}

#[test]
fn double_refund_cannot_pay_out_twice() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token(&env, &token_id);
    let (id, _, _) = single_invoice(&env, &c, &token_id, 500, 2_000);
    let payer = Address::generate(&env);
    fund(&env, &token_id, &payer, 100);
    assert!(try_pay(&c, &payer, id, 100));

    env.ledger().set_timestamp(3_000);
    assert!(c.try_refund(&id).is_ok());
    assert!(c.try_refund(&id).is_err(), "second refund must be rejected");
    assert_eq!(tk.balance(&payer), 100, "payer is refunded exactly once");
}

#[test]
fn no_interleaving_leaves_invoice_stuck() {
    // Deadlock detection: after any prefix of operations the invoice is either
    // final or still accepts the exact remaining amount.
    let amounts = [25, 25, 25, 25];
    for order in [[0usize, 1, 2, 3], [3, 1, 0, 2]] {
        let (env, contract_id, token_id) = setup();
        let c = client(&env, &contract_id);
        let (id, _, _) = single_invoice(&env, &c, &token_id, 100, DEADLINE);
        let payers: std::vec::Vec<Address> = amounts.iter().map(|_| Address::generate(&env)).collect();
        for p in &payers {
            fund(&env, &token_id, p, 100);
        }

        for (step, &i) in order.iter().enumerate() {
            assert!(try_pay(&c, &payers[i], id, amounts[i]));
            let invoice = c.get_invoice(&id);
            assert_eq!(invoice.funded, 25 * (step as i128 + 1));
            if invoice.status == InvoiceStatus::Pending {
                assert!(invoice.funded < 100, "pending invoice must still be fundable");
            }
        }
        assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    }
}

#[test]
fn token_supply_is_conserved_across_interleavings() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token(&env, &token_id);
    let (id, creator, recipient) = single_invoice(&env, &c, &token_id, 100, DEADLINE);

    let payers: std::vec::Vec<Address> = (0..5).map(|_| Address::generate(&env)).collect();
    for p in &payers {
        fund(&env, &token_id, p, 50);
    }

    let total_before: i128 = payers.iter().map(|p| tk.balance(p)).sum::<i128>() + tk.balance(&recipient);

    for p in &payers {
        let _ = try_pay(&c, p, id, 30);
    }
    let _ = c.try_cancel_invoice(&creator, &id);

    let held = tk.balance(&contract_id);
    let total_after: i128 = payers.iter().map(|p| tk.balance(p)).sum::<i128>() + tk.balance(&recipient) + held;
    assert_eq!(total_before, total_after, "no tokens created or destroyed");
}
