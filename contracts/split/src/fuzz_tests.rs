#![cfg(test)]
#![allow(clippy::all)]

extern crate std;

use proptest::prelude::*;
use std::{vec, vec::Vec};

const TOTAL_BPS: u32 = 10_000;

fn percentage_weights() -> impl Strategy<Value = Vec<u32>> {
    proptest::collection::vec(0u32..=TOTAL_BPS, 1..=16)
}

fn normalize_percentages(weights: &[u32]) -> Vec<u32> {
    let total_weight: u64 = weights.iter().map(|weight| *weight as u64).sum();
// ---------------------------------------------------------------------------
// Pure arithmetic helpers that mirror the contract's logic.
// ---------------------------------------------------------------------------

/// Proportional share of `funded` for one recipient (mirrors `_release_full`
/// and `partial_release` distribution).
fn proportional_share(
    amount: i128,
    total: i128,
    funded: i128,
    is_last: bool,
    distributed: i128,
) -> i128 {
    if total == 0 {
        return 0;
    }
    if is_last {
        funded.saturating_sub(distributed)
    } else {
        (amount as u128 * funded as u128 / total as u128) as i128
    }
}

/// Platform fee and tax deduction (mirrors `_release_full` fee logic).
fn deduct_fees(
    proportional: i128,
    platform_fee_bps: u32,
    tax_bps: u32,
    is_waived: bool,
) -> (i128, i128, i128) {
    let tax = (proportional as u128 * tax_bps as u128 / MAX_BPS as u128) as i128;
    let post_tax = proportional.saturating_sub(tax);
    let fee = if is_waived {
        0
    } else {
        (post_tax as u128 * platform_fee_bps as u128 / MAX_BPS as u128) as i128
    };
    let payout = post_tax.saturating_sub(fee);
    (payout, fee, tax)
}

/// Distribute `funded` across `amounts` proportionally and deduct fees.
/// Returns (payouts, fees, taxes) per recipient plus total_fee, total_tax.
fn distribute_split(
    amounts: &[i128],
    funded: i128,
    platform_fee_bps: u32,
    tax_bps: u32,
    waive_all: bool,
) -> (Vec<i128>, Vec<i128>, Vec<i128>, i128, i128) {
    let n = amounts.len();
    let total: i128 = amounts.iter().sum();
    if total == 0 || funded == 0 {
        return (std::vec![0; n], std::vec![0; n], std::vec![0; n], 0, 0);
    }

    let mut payouts = std::vec![0i128; n];
    let mut fees = std::vec![0i128; n];
    let mut taxes = std::vec![0i128; n];
    let mut distributed: i128 = 0;

    for i in 0..n {
        let prop = proportional_share(amounts[i], total, funded, i == n - 1, distributed);
        distributed += prop;
        let (p, f, t) = deduct_fees(prop, platform_fee_bps, tax_bps, waive_all);
        payouts[i] = p;
        fees[i] = f;
        taxes[i] = t;
    }

    let total_fee: i128 = fees.iter().sum();
    let total_tax: i128 = taxes.iter().sum();
    (payouts, fees, taxes, total_fee, total_tax)
}

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
fn invoice_amounts() -> impl Strategy<Value = Vec<i128>> {
    (1usize..=20usize).prop_flat_map(|n| proptest::collection::vec(pos_i128(), n))
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
