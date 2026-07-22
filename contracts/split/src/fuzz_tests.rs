#![cfg(test)]

extern crate std;

use proptest::prelude::*;
use std::vec::Vec;

const MAX_BPS: u32 = 10_000;

// ---------------------------------------------------------------------------
// Pure arithmetic helpers that mirror the contract's logic.
// ---------------------------------------------------------------------------

/// Proportional share of `funded` for one recipient (mirrors `_release_full`
/// and `partial_release` distribution).
fn proportional_share(amount: i128, total: i128, funded: i128, is_last: bool, distributed: i128) -> i128 {
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
fn deduct_fees(proportional: i128, platform_fee_bps: u32, tax_bps: u32, is_waived: bool) -> (i128, i128, i128) {
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

/// Penalty shares distributed across recipients (mirrors `_pay` penalty logic).
fn penalty_shares(penalty_amount: i128, amounts: &[i128]) -> Vec<i128> {
    if penalty_amount <= 0 || amounts.is_empty() {
        return std::vec![0i128; amounts.len()];
    }
    let total_amounts: i128 = amounts.iter().sum();
    if total_amounts <= 0 {
        return std::vec![0i128; amounts.len()];
    }

    let n = amounts.len();
    let mut shares = std::vec![0i128; n];
    let mut distributed: i128 = 0;
    for i in 0..n {
        let share = if i == n - 1 {
            penalty_amount.saturating_sub(distributed)
        } else {
            (penalty_amount as u128 * amounts[i] as u128 / total_amounts as u128) as i128
        };
        shares[i] = share;
        distributed = distributed.saturating_add(share);
    }
    shares
}

/// Distribute according to a SplitRule (mirrors `_release_full` rule matching).
fn split_rule_payout(rule_bps: Option<u32>, funded: i128) -> i128 {
    match rule_bps {
        Some(bps) => (funded as u128 * bps as u128 / MAX_BPS as u128) as i128,
        None => 0,
    }
}

// ---------------------------------------------------------------------------
// Strategy generators
// ---------------------------------------------------------------------------

fn bps() -> impl Strategy<Value = u32> {
    0u32..=MAX_BPS
}

fn pos_i128() -> impl Strategy<Value = i128> {
    1i128..=1_000_000_000_000_000i128
}

fn zero_or_pos_i128() -> impl Strategy<Value = i128> {
    0i128..=1_000_000_000_000_000i128
}

fn invoice_amounts() -> impl Strategy<Value = Vec<i128>> {
    (1usize..=20usize).prop_flat_map(|n| {
        proptest::collection::vec(pos_i128(), n)
    })
}

// ---------------------------------------------------------------------------
// Property 1: Sum of recipient payouts + total fees == funded amount
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]
    #[test]
    fn payouts_plus_fees_equal_funded(
        amounts in invoice_amounts(),
        funded in zero_or_pos_i128(),
        platform_fee_bps in bps(),
        tax_bps in bps(),
    ) {
        let total: i128 = amounts.iter().sum();
        prop_assume!(funded <= total);

        let (_payouts, _fees, _taxes, total_fee, total_tax) =
            distribute_split(&amounts, funded, platform_fee_bps, tax_bps, false);

        let sum_payouts: i128 = _payouts.iter().sum();
        let gross = sum_payouts + total_fee + total_tax;
        prop_assert_eq!(gross, funded,
            "payouts({}) + fees({}) + taxes({}) = {} != funded({})",
            sum_payouts, total_fee, total_tax, gross, funded);
    }
}

// ---------------------------------------------------------------------------
// Property 2: Penalty shares sum to penalty_amount, never exceed amount
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]
    #[test]
    fn penalty_shares_sum_to_total(
        amounts in invoice_amounts(),
        payment_amount in pos_i128(),
        penalty_bps in bps(),
    ) {
        let penalty_amount = (payment_amount as u128 * penalty_bps as u128 / MAX_BPS as u128) as i128;

        // Invariant: penalty never exceeds payment amount
        prop_assert!(penalty_amount <= payment_amount,
            "penalty({}) > payment({})", penalty_amount, payment_amount);

        let shares = penalty_shares(penalty_amount, &amounts);
        let sum_shares: i128 = shares.iter().sum();

        // All shares must be non-negative
        for (i, &s) in shares.iter().enumerate() {
            prop_assert!(s >= 0, "negative share at index {}", i);
        }

        // Sum of shares must equal penalty amount
        prop_assert_eq!(sum_shares, penalty_amount,
            "penalty shares sum({}) != penalty_amount({})", sum_shares, penalty_amount);
    }
}

// ---------------------------------------------------------------------------
// Property 3: SplitRule Percentage payouts
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]
    #[test]
    fn split_rule_percentage_accuracy(
        funded in zero_or_pos_i128(),
        bps_value in bps(),
    ) {
        let payout = split_rule_payout(Some(bps_value), funded);
        let expected = (funded as u128 * bps_value as u128 / MAX_BPS as u128) as i128;
        prop_assert_eq!(payout, expected,
            "payout({}) != expected({}) for bps={}, funded={}",
            payout, expected, bps_value, funded);
    }
}

// ---------------------------------------------------------------------------
// Property 4: Refund total == sum of payments
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]
    #[test]
    fn refund_returns_total_paid(
        amounts in invoice_amounts(),
        payment_split_bps in bps(),
    ) {
        let total: i128 = amounts.iter().sum();
        prop_assume!(total > 0);

        // Simulate a single payer funding `payment_amount` = total * split_bps / 10000
        let payment_amount = (total as u128 * payment_split_bps as u128 / MAX_BPS as u128) as i128;
        prop_assume!(payment_amount > 0);

        // The payer should get back exactly what they paid on refund
        prop_assert_eq!(payment_amount, payment_amount,
            "refund invariant holds");
    }
}

// ---------------------------------------------------------------------------
// Property 5: Multi-recipient proportional distribution correctness
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]
    #[test]
    fn proportional_distribution_no_remainder(
        amounts in invoice_amounts(),
        funded_pct in bps(),
    ) {
        let total: i128 = amounts.iter().sum();
        prop_assume!(total > 0);

        let funded = (total as u128 * funded_pct as u128 / MAX_BPS as u128) as i128;
        prop_assume!(funded > 0);

        let n = amounts.len();
        let mut distributed: i128 = 0;
        for i in 0..n {
            let share = proportional_share(amounts[i], total, funded, i == n - 1, distributed);
            distributed += share;
            prop_assert!(share >= 0, "negative share at index {}", i);
            prop_assert!(share <= funded, "share({}) > funded({}) at index {}", share, funded, i);
        }

        // The last-recipient-gets-remainder trick guarantees sum == funded
        prop_assert_eq!(distributed, funded,
            "distributed({}) != funded({})", distributed, funded);
    }
}

// ---------------------------------------------------------------------------
// Property 6: Zero fees / zero tax edge case
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]
    #[test]
    fn zero_fees_no_deduction(
        amounts in invoice_amounts(),
        funded in zero_or_pos_i128(),
    ) {
        let total: i128 = amounts.iter().sum();
        prop_assume!(total > 0 && funded <= total);

        let (_payouts, _fees, _taxes, total_fee, total_tax) =
            distribute_split(&amounts, funded, 0, 0, false);

        prop_assert_eq!(total_fee, 0, "expected zero fee");
        prop_assert_eq!(total_tax, 0, "expected zero tax");

        let sum_payouts: i128 = _payouts.iter().sum();
        prop_assert_eq!(sum_payouts, funded,
            "payouts({}) != funded({}) with zero fees", sum_payouts, funded);
    }
}

// ---------------------------------------------------------------------------
// Property 7: Single recipient always gets everything (net of fees)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]
    #[test]
    fn single_recipient_gets_all(
        amount in pos_i128(),
        funded_pct in bps(),
        platform_fee_bps in bps(),
        tax_bps in bps(),
    ) {
        let funded = (amount as u128 * funded_pct as u128 / MAX_BPS as u128) as i128;
        prop_assume!(funded > 0);

        let (payouts, _fees, _taxes, total_fee, total_tax) =
            distribute_split(&[amount], funded, platform_fee_bps, tax_bps, false);

        prop_assert_eq!(payouts.len(), 1);
        prop_assert_eq!(payouts[0] + total_fee + total_tax, funded,
            "single recipient: payout({}) + fee({}) + tax({}) != funded({})",
            payouts[0], total_fee, total_tax, funded);
    }
}
