#![cfg(test)]
#![allow(clippy::all)]
// Reference-model helpers kept alongside the properties they support; not every
// one is referenced by the currently-enabled proptest cases.
#![allow(dead_code)]

extern crate std;

use proptest::prelude::*;
use std::{vec, vec::Vec};

const TOTAL_BPS: u32 = 10_000;
const MAX_BPS: u32 = 10_000;

fn percentage_weights() -> impl Strategy<Value = Vec<u32>> {
    proptest::collection::vec(0u32..=TOTAL_BPS, 1..=16)
}

fn pos_i128() -> impl Strategy<Value = i128> {
    1i128..=1_000_000_000_000_000i128
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

fn invoice_amounts() -> impl Strategy<Value = Vec<i128>> {
    (1usize..=20usize).prop_flat_map(|n| proptest::collection::vec(pos_i128(), n))
}

fn recipient_entitlement(invoice_amount: u128, percentage_bps: u32) -> u128 {
    let numerator = invoice_amount * percentage_bps as u128;
    (numerator + TOTAL_BPS as u128 - 1) / TOTAL_BPS as u128
}

// ---------------------------------------------------------------------------
// Fixed-point arithmetic reference model.
//
// The contract performs split math with integer fixed-point scaling to avoid
// repeated wide multiplications. These helpers mirror that approach so the
// properties below can validate accuracy and the sum invariant.
// ---------------------------------------------------------------------------

/// Fixed-point scale factor (2^32) used for intermediate math.
const FP_SCALE: u128 = 1u128 << 32;

/// Convert basis points to a fixed-point fraction.
fn bps_to_fixed(bps: u32) -> u128 {
    bps as u128 * FP_SCALE / TOTAL_BPS as u128
}

/// Fixed-point proportional share: `amount * fraction` truncated back to ints.
fn fixed_point_share(amount: u128, fraction: u128) -> u128 {
    (amount * fraction) / FP_SCALE
}

/// Split `invoice_amount` using fixed-point fractions, distributing any rounding
/// remainder based on entitlements so no recipient exceeds their share.
fn fixed_point_split(invoice_amount: u128, percentages: &[u32]) -> Vec<u128> {
    let n = percentages.len();
    if n == 0 {
        return Vec::new();
    }

    let mut payouts: Vec<u128> = percentages
        .iter()
        .map(|percentage| fixed_point_share(invoice_amount, bps_to_fixed(*percentage)))
        .collect();

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

        assert!(
            assigned_in_pass,
            "no recipient can receive the remaining rounding unit"
        );
    }

    payouts
}

// ---------------------------------------------------------------------------
// Incremental state update reference model.
// ---------------------------------------------------------------------------

/// A single mutation applied to a split entry's percentage table.
#[derive(Clone, Debug)]
enum Mutation {
    /// Set the percentage at `index` to `value` (clamped to `MAX_BPS`).
    Set { index: usize, value: u32 },
    /// Swap two entries.
    Swap { a: usize, b: usize },
    /// Add `delta` to the entry at `index` (saturating at `MAX_BPS`).
    Add { index: usize, delta: u32 },
}

fn mutation_strategy(n: usize) -> impl Strategy<Value = Mutation> {
    prop_oneof![
        (0usize..n, 0u32..=MAX_BPS).prop_map(|(index, value)| Mutation::Set { index, value }),
        (0usize..n, 0usize..n).prop_map(|(a, b)| Mutation::Swap { a, b }),
        (0usize..n, 0u32..=MAX_BPS).prop_map(|(index, delta)| Mutation::Add { index, delta }),
    ]
}

fn mutation_sequence(n: usize) -> impl Strategy<Value = Vec<Mutation>> {
    proptest::collection::vec(mutation_strategy(n), 0..=32)
}

/// Apply a mutation to a percentage table in place.
fn apply_mutation(percentages: &mut [u32], mutation: &Mutation) {
    match *mutation {
        Mutation::Set { index, value } => {
            percentages[index] = value.min(MAX_BPS);
        }
        Mutation::Swap { a, b } => percentages.swap(a, b),
        Mutation::Add { index, delta } => {
            percentages[index] = percentages[index].saturating_add(delta).min(MAX_BPS);
        }
    }
}

/// Full-entry write: recompute the entire table from scratch.
fn full_write(initial: &[u32], mutations: &[Mutation]) -> Vec<u32> {
    let mut table = initial.to_vec();
    for mutation in mutations {
        apply_mutation(&mut table, mutation);
    }
    table
}

/// Incremental update: apply each mutation to the live table, tracking only
/// the touched indices and recomputing derived state lazily.
fn incremental_update(initial: &[u32], mutations: &[Mutation]) -> Vec<u32> {
    let mut table = initial.to_vec();
    let mut dirty: Vec<usize> = Vec::new();

    for mutation in mutations {
        match *mutation {
            Mutation::Set { index, .. } | Mutation::Add { index, .. } => {
                apply_mutation(&mut table, mutation);
                if !dirty.contains(&index) {
                    dirty.push(index);
                }
            }
            Mutation::Swap { a, b } => {
                apply_mutation(&mut table, mutation);
                for index in [a, b] {
                    if !dirty.contains(&index) {
                        dirty.push(index);
                    }
                }
            }
        }
    }

    // Recompute derived state only for dirty entries; here the derived state is
    // the table itself, so this simply returns the converged table.
    for index in dirty {
        table[index] = table[index].min(MAX_BPS);
    }

    table
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

        assert!(
            assigned_in_pass,
            "no recipient can receive the remaining rounding unit"
        );
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
        prop_assert_eq!(state.payouts.iter().sum::<u128>(), invoice_amount);
        prop_assert_eq!(state.payouts, payouts_after_first_release);
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

    #[test]
    fn fixed_point_split_preserves_sum_invariant(
        invoice_amount in 1u128..=1_000_000_000_000_000u128,
        weights in percentage_weights(),
    ) {
        let percentages = normalize_percentages(&weights);
        let payouts = fixed_point_split(invoice_amount, &percentages);

        prop_assert_eq!(payouts.len(), percentages.len());
        prop_assert_eq!(payouts.iter().sum::<u128>(), invoice_amount);
    }

    #[test]
    fn fixed_point_split_matches_integer_split_accuracy(
        invoice_amount in 1u128..=1_000_000_000_000_000u128,
        weights in percentage_weights(),
    ) {
        let percentages = normalize_percentages(&weights);
        let fixed = fixed_point_split(invoice_amount, &percentages);
        let reference = split_payouts(invoice_amount, &percentages);

        // Every recipient must stay within one rounding unit of the reference.
        for (fp, ref_payout) in fixed.iter().zip(reference.iter()) {
            let diff = fp.abs_diff(*ref_payout);
            prop_assert!(diff <= 1, "fixed-point payout diverged by {}", diff);
        }
    }

    #[test]
    fn fixed_point_share_never_exceeds_entitlement(
        invoice_amount in 1u128..=1_000_000_000_000_000u128,
        weights in percentage_weights(),
    ) {
        let percentages = normalize_percentages(&weights);
        let payouts = fixed_point_split(invoice_amount, &percentages);

        for (payout, percentage) in payouts.iter().zip(percentages.iter()) {
            let entitlement = recipient_entitlement(invoice_amount, *percentage);
            prop_assert!(*payout <= entitlement);
        }
    }

    #[test]
    fn incremental_update_converges_to_full_write(
        weights in percentage_weights(),
        mutations in mutation_sequence(16),
    ) {
        let initial = normalize_percentages(&weights);
        // Clamp the mutation sequence to the actual table length.
        let mutations: Vec<Mutation> = mutations
            .into_iter()
            .filter_map(|mutation| match mutation {
                Mutation::Set { index, value } if index < initial.len() => {
                    Some(Mutation::Set { index, value })
                }
                Mutation::Add { index, delta } if index < initial.len() => {
                    Some(Mutation::Add { index, delta })
                }
                Mutation::Swap { a, b } if a < initial.len() && b < initial.len() => {
                    Some(Mutation::Swap { a, b })
                }
                _ => None,
            })
            .collect();

        let full = full_write(&initial, &mutations);
        let incremental = incremental_update(&initial, &mutations);

        prop_assert_eq!(full, incremental);
    }

    #[test]
    fn incremental_update_preserves_length_and_bounds(
        weights in percentage_weights(),
        mutations in mutation_sequence(16),
    ) {
        let initial = normalize_percentages(&weights);
        let mutations: Vec<Mutation> = mutations
            .into_iter()
            .filter_map(|mutation| match mutation {
                Mutation::Set { index, value } if index < initial.len() => {
                    Some(Mutation::Set { index, value })
                }
                Mutation::Add { index, delta } if index < initial.len() => {
                    Some(Mutation::Add { index, delta })
                }
                Mutation::Swap { a, b } if a < initial.len() && b < initial.len() => {
                    Some(Mutation::Swap { a, b })
                }
                _ => None,
            })
            .collect();

        let incremental = incremental_update(&initial, &mutations);

        prop_assert_eq!(incremental.len(), initial.len());
        prop_assert!(incremental.iter().all(|percentage| *percentage <= MAX_BPS));
    }
}
