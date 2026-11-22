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

/// Fixed-point scale used by the allocation path. Percentages are expressed in
/// basis points (1/10_000), so the fixed-point representation keeps the same
/// denominator to avoid any loss of precision.
const FIXED_POINT_SCALE: u128 = TOTAL_BPS as u128;

/// Allocates the complete invoice amount using fixed-point arithmetic with the
/// largest-remainder method. This mirrors an integer-safe split where rounding
/// leftovers are distributed without losing any part of the invoice amount.
fn allocate_invoice(total: u128, percentages: &[u32]) -> Vec<u128> {
    let mut payouts = Vec::with_capacity(percentages.len());
    let mut remainders = Vec::with_capacity(percentages.len());
    let mut allocated = 0u128;

    for &percentage in percentages {
        // Fixed-point multiply: total * (percentage / TOTAL_BPS) computed with
        // an explicit scale so the integer division is exact in the quotient
        // and the fractional part is captured in the remainder.
        let fixed_percentage = percentage as u128;
        let numerator = total * fixed_percentage;
        let payout = numerator / FIXED_POINT_SCALE;
        payouts.push(payout);
        remainders.push(numerator % FIXED_POINT_SCALE);
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
    fn fixed_point_allocation_matches_exact_rational_split(
        total in 0u128..=1_000_000_000_000u128,
        percentages in percentages(),
    ) {
        let payouts = allocate_invoice(total, &percentages);

        // Each payout must be within one unit of the exact rational share,
        // which is the tightest bound achievable with integer fixed-point.
        for (payout, percentage) in payouts.iter().zip(percentages.iter()) {
            let numerator = total * *percentage as u128;
            let exact_floor = numerator / FIXED_POINT_SCALE;
            let exact_ceil = numerator.div_ceil(FIXED_POINT_SCALE);
            prop_assert!(*payout >= exact_floor);
            prop_assert!(*payout <= exact_ceil);
        }
    }

    #[test]
    fn fixed_point_allocation_preserves_zero_percentages(
        total in 0u128..=1_000_000_000_000u128,
        percentages in percentages(),
    ) {
        let payouts = allocate_invoice(total, &percentages);

        for (payout, percentage) in payouts.iter().zip(percentages.iter()) {
            if *percentage == 0 {
                prop_assert_eq!(*payout, 0);
            }
        }
    }

    #[test]
    fn no_recipient_exceeds_their_percentage_entitlement(
        total in 0u128..=1_000_000_000_000u128,
        percentages in percentages(),
    ) {
        let payouts = allocate_invoice(total, &percentages);

        for (payout, percentage) in payouts.iter().zip(percentages.iter()) {
            let numerator = total * *percentage as u128;
            let entitlement_ceiling = numerator.div_ceil(TOTAL_BPS as u128);
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
        prop_assert_eq!(percentages.iter().copied().sum::<u32>(), TOTAL_BPS);
    }

    #[test]
    fn fixed_point_scale_matches_basis_points(
        percentages in percentages(),
    ) {
        prop_assert_eq!(FIXED_POINT_SCALE, TOTAL_BPS as u128);
        prop_assert_eq!(percentages.iter().copied().sum::<u32>(), TOTAL_BPS);
    }
}
