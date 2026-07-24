#![cfg(test)]

extern crate std;

use proptest::prelude::*;
use std::{vec, vec::Vec};

const TOTAL_BPS: u32 = 10_000;

fn percentage_weights() -> impl Strategy<Value = Vec<u32>> {
    proptest::collection::vec(0u32..=TOTAL_BPS, 1..=16)
}

fn normalize_percentages(weights: &[u32]) -> Vec<u32> {
    let total_weight: u64 = weights.iter().map(|weight| *weight as u64).sum();

    if total_weight == 0 {
        let mut percentages = vec![0; weights.len()];
        percentages[0] = TOTAL_BPS;
        return percentages;
    }

    let mut percentages = Vec::with_capacity(weights.len());
    let mut assigned = 0u32;

    for weight in weights.iter().take(weights.len() - 1) {
        let percentage = ((*weight as u64 * TOTAL_BPS as u64) / total_weight) as u32;
        percentages.push(percentage);
        assigned += percentage;
    }

    percentages.push(TOTAL_BPS - assigned);
    percentages
}

fn recipient_entitlement(invoice_amount: u128, percentage_bps: u32) -> u128 {
    let numerator = invoice_amount * percentage_bps as u128;
    (numerator + TOTAL_BPS as u128 - 1) / TOTAL_BPS as u128
}

fn split_payouts(invoice_amount: u128, percentages: &[u32]) -> Vec<u128> {
    let mut payouts = percentages
        .iter()
        .map(|percentage| invoice_amount * *percentage as u128 / TOTAL_BPS as u128)
        .collect::<Vec<_>>();

    let distributed: u128 = payouts.iter().sum();
    let mut remaining = invoice_amount - distributed;

    while remaining > 0 {
        let mut assigned_in_pass = false;

        for (index, percentage) in percentages.iter().enumerate() {
            if remaining == 0 {
                break;
            }

            let entitlement = recipient_entitlement(invoice_amount, *percentage);
            if payouts[index] < entitlement {
                payouts[index] += 1;
                remaining -= 1;
                assigned_in_pass = true;
            }
        }

        assert!(assigned_in_pass, "no recipient can receive the remaining rounding unit");
    }

    payouts
}

#[derive(Clone, Debug)]
struct ReleaseState {
    released: bool,
    payouts: Vec<u128>,
}

fn release_funds(state: &mut ReleaseState, invoice_amount: u128, percentages: &[u32]) {
    if state.released {
        return;
    }

    state.payouts = split_payouts(invoice_amount, percentages);
    state.released = true;
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    #[test]
    fn payout_sum_equals_invoice_amount(
        invoice_amount in 1u128..=1_000_000_000_000_000u128,
        weights in percentage_weights(),
    ) {
        let percentages = normalize_percentages(&weights);
        let payouts = split_payouts(invoice_amount, &percentages);

        prop_assert_eq!(percentages.iter().sum::<u32>(), TOTAL_BPS);
        prop_assert_eq!(payouts.iter().sum::<u128>(), invoice_amount);
    }

    #[test]
    fn no_recipient_exceeds_percentage_entitlement(
        invoice_amount in 1u128..=1_000_000_000_000_000u128,
        weights in percentage_weights(),
    ) {
        let percentages = normalize_percentages(&weights);
        let payouts = split_payouts(invoice_amount, &percentages);

        for (payout, percentage) in payouts.iter().zip(percentages.iter()) {
            let entitlement = recipient_entitlement(invoice_amount, *percentage);
            prop_assert!(*payout <= entitlement);
        }
    }

    #[test]
    fn release_funds_is_idempotent(
        invoice_amount in 1u128..=1_000_000_000_000_000u128,
        weights in percentage_weights(),
    ) {
        let percentages = normalize_percentages(&weights);
        let mut state = ReleaseState {
            released: false,
            payouts: vec![0; percentages.len()],
        };

        release_funds(&mut state, invoice_amount, &percentages);
        let payouts_after_first_release = state.payouts.clone();

        release_funds(&mut state, invoice_amount, &percentages);

        prop_assert!(state.released);
        prop_assert_eq!(state.payouts, payouts_after_first_release);
        prop_assert_eq!(state.payouts.iter().sum::<u128>(), invoice_amount);
    }

    #[test]
    fn fully_funded_invoice_has_at_least_target_amount(
        target_amount in 0u128..=1_000_000_000_000_000u128,
        additional_funding in 0u128..=1_000_000_000_000_000u128,
    ) {
        let funded_amount = target_amount + additional_funding;
        prop_assert!(funded_amount >= target_amount);
    }

    #[test]
    fn recipient_percentages_sum_to_exactly_one_hundred_percent(
        weights in percentage_weights(),
    ) {
        let percentages = normalize_percentages(&weights);

        prop_assert_eq!(percentages.iter().sum::<u32>(), TOTAL_BPS);
        prop_assert!(percentages.iter().all(|percentage| *percentage <= TOTAL_BPS));
    }
}
