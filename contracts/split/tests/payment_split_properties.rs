#![cfg(test)]

use proptest::prelude::*;
use std::vec::Vec;

const TOTAL_BPS: u32 = 10_000;

fn percentages() -> impl Strategy<Value = Vec<u32>> {
    (1usize..=16).prop_flat_map(|length| {
        prop::collection::vec(0u32..=TOTAL_BPS, length - 1).prop_map(move |mut cuts| {
            cuts.sort_unstable();

            let mut result = Vec::with_capacity(length);
            let mut previous = 0u32;
            for cut in cuts {
                result.push(cut - previous);
                previous = cut;
            }
            result.push(TOTAL_BPS - previous);
            result
        })
    })
}

/// Allocates the complete invoice amount using the largest-remainder method.
/// This mirrors an integer-safe split where rounding leftovers are distributed
/// without losing any part of the invoice amount.
fn allocate_invoice(total: u128, percentages: &[u32]) -> Vec<u128> {
    let mut payouts = Vec::with_capacity(percentages.len());
    let mut remainders = Vec::with_capacity(percentages.len());
    let mut allocated = 0u128;

    for &percentage in percentages {
        let numerator = total * percentage as u128;
        let payout = numerator / TOTAL_BPS as u128;
        payouts.push(payout);
        remainders.push(numerator % TOTAL_BPS as u128);
        allocated += payout;
    }

    let mut remaining = total - allocated;
    let mut order: Vec<usize> = (0..percentages.len())
        .filter(|index| percentages[*index] > 0)
        .collect();
    order.sort_by(|left, right| {
        remainders[*right]
            .cmp(&remainders[*left])
            .then_with(|| left.cmp(right))
    });

    let mut index = 0usize;
    while remaining > 0 {
        payouts[order[index]] += 1;
        remaining -= 1;
        index = (index + 1) % order.len();
    }

    payouts
}

fn release_funds(released: &mut bool, payouts: &[u128]) -> Vec<u128> {
    if *released {
        vec![0; payouts.len()]
    } else {
        *released = true;
        payouts.to_vec()
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 1_000,
        .. ProptestConfig::default()
    })]

    #[test]
    fn recipient_payouts_sum_to_the_invoice_amount(
        total in 0u128..=1_000_000_000_000u128,
        percentages in percentages(),
    ) {
        let payouts = allocate_invoice(total, &percentages);

        prop_assert_eq!(payouts.iter().sum::<u128>(), total);
    }

    #[test]
    fn no_recipient_exceeds_their_percentage_entitlement(
        total in 0u128..=1_000_000_000_000u128,
        percentages in percentages(),
    ) {
        let payouts = allocate_invoice(total, &percentages);

        for (payout, percentage) in payouts.iter().zip(percentages.iter()) {
            let numerator = total * *percentage as u128;
            let entitlement_ceiling =
                (numerator + TOTAL_BPS as u128 - 1) / TOTAL_BPS as u128;
            prop_assert!(*payout <= entitlement_ceiling);
        }
    }

    #[test]
    fn release_funds_is_idempotent(
        total in 0u128..=1_000_000_000_000u128,
        percentages in percentages(),
    ) {
        let payouts = allocate_invoice(total, &percentages);
        let mut released = false;

        let first_release = release_funds(&mut released, &payouts);
        let second_release = release_funds(&mut released, &payouts);

        prop_assert_eq!(first_release, payouts);
        prop_assert_eq!(second_release.iter().sum::<u128>(), 0);
        prop_assert!(released);
    }

    #[test]
    fn fully_funded_invoice_has_at_least_its_target_amount(
        target in 0u128..=1_000_000_000_000u128,
        additional_funding in 0u128..=1_000_000_000_000u128,
    ) {
        let funded_amount = target + additional_funding;

        prop_assert!(funded_amount >= target);
    }

    #[test]
    fn recipient_percentages_sum_to_exactly_one_hundred_percent(
        percentages in percentages(),
    ) {
        prop_assert_eq!(percentages.iter().map(|value| *value as u32).sum::<u32>(), TOTAL_BPS);
    }
}
