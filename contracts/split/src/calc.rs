//! Arithmetic helpers for token distribution.
//!
//! Implements the **largest-remainder method** to distribute an integer `total`
//! across recipients proportionally, ensuring every stroop is accounted for
//! (i.e. `sum(result) == total` always holds).
//!
//! # Why largest-remainder?
//!
//! Splitting an integer `total` proportionally by ratios almost never divides evenly.
//! A naive implementation would compute each recipient's share with floor division
//! (`total * ratio / denom`) and stop there, but floor division systematically discards
//! the fractional part of every share. With `n` recipients that can leave up to `n - 1`
//! stroops undistributed — money that was paid in but never assigned to anyone, silently
//! stuck in the contract and breaking the `sum(result) == total` invariant the rest of the
//! contract relies on (e.g. reconciling `funded` against amounts actually paid out).
//!
//! The largest-remainder method fixes this without abandoning integer (floor) division:
//! 1. Compute each recipient's floor share (`total * ratio / denom`) and remainder
//!    (`total * ratio % denom`).
//! 2. Sum the floor shares; the difference between `total` and that sum is the number of
//!    leftover stroops still owed (always `< n`).
//! 3. Sort recipients by remainder descending and hand out one extra stroop each, in that
//!    order, until the leftover is exhausted.
//!
//! This guarantees `sum(result) == total` exactly, while keeping the discrepancy from
//! true proportionality to at most one stroop per recipient — the smallest error possible
//! for integer division — and it deterministically favors the recipients whose exact
//! (real-valued) share was closest to rounding up.
//!
//! **Example:** distributing `10` stroops among 3 recipients with equal ratios (`1:1:1`,
//! `denom = 3`) gives floor shares of `[3, 3, 3]` (sum `9`) with `1` stroop leftover, all
//! three remainders tied at `1`. The tie-break (first index wins) assigns the leftover
//! stroop to the first recipient, producing `[4, 3, 3]` — which sums to `10`.

#[allow(unused_imports)]
use crate::types::BASIS_POINTS_TOTAL;
use soroban_sdk::{Address, Env, Map, Symbol, Vec};

use crate::error::ContractError;
use crate::math::{bps_of, mul_div, mul_mod, BPS_DENOMINATOR};

// ---------------------------------------------------------------------------
// Issue #955: Fee event tracking
// ---------------------------------------------------------------------------

/// Identifies which fee bucket an event refers to.
///
/// Kept as a small, explicit enum so emitted events carry a clear, indexable
/// type discriminator rather than an opaque integer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeeType {
    /// Platform fee retained by the contract/treasury.
    Platform,
    /// Fee owed to the invoice creator.
    Creator,
    /// Fee owed to the protocol treasury.
    Protocol,
}

impl FeeType {
    /// Stable, indexable symbol for this fee type (used in event topics).
    pub fn as_symbol(&self) -> Symbol {
        match self {
            FeeType::Platform => Symbol::new(&Env::default(), "platform"),
            FeeType::Creator => Symbol::new(&Env::default(), "creator"),
            FeeType::Protocol => Symbol::new(&Env::default(), "protocol"),
        }
    }
}

/// Emit a `fee_calculated` event recording the computed fee for a given type.
///
/// Topics: `("fee_calculated", fee_type)` — the fee type is part of the topic
/// so indexers can filter by fee type. Data: the computed `amount`.
pub fn emit_fee_calculated(env: &Env, fee_type: FeeType, amount: i128) {
    let topics = (Symbol::new(env, "fee_calculated"), fee_type.as_symbol());
    env.events().publish(topics, amount);
}

/// Emit a `fee_distributed` event recording a fee transfer to a recipient.
///
/// Topics: `("fee_distributed", fee_type)` — indexable by fee type. Data: the
/// `(recipient, amount)` pair, where `amount` must equal the amount actually
/// transferred.
pub fn emit_fee_distributed(env: &Env, fee_type: FeeType, recipient: &Address, amount: i128) {
    let topics = (Symbol::new(env, "fee_distributed"), fee_type.as_symbol());
    env.events().publish(topics, (recipient.clone(), amount));
}

/// Distribute `total` among recipients according to their `ratios` out of
/// `denom`, using the largest-remainder method to handle rounding.
///
/// # Arguments
/// * `env`    – Soroban environment (needed to allocate the result `Vec`)
/// * `total`  – total amount to distribute (stroops); must be ≥ 0
/// * `ratios` – relative weight of each recipient (must be non-empty, all ≥ 0)
/// * `denom`  – sum of all ratios (must be > 0); typically [`BASIS_POINTS_TOTAL`]
///
/// # Guarantees
/// * `result.iter().sum::<i128>() == total` always
/// * recipients with larger fractional remainders receive an extra stroop
/// * pure function: no side effects, no storage access
///
/// # Panics
/// * if `ratios` is empty
/// * if `denom` is zero
// NOTE: if you call this function and ignore its return value the Rust
// compiler will emit a `#[must_use]` warning:
//   warning: unused return value of `distribute_with_remainder` that must be used
// This ensures callers never silently drop the distribution result.
#[must_use = "the distribution result must be applied to recipients"]
pub fn distribute_with_remainder(
    env: &Env,
    total: i128,
    ratios: &Vec<i128>,
    denom: i128,
) -> Result<Vec<i128>, ContractError> {
    if ratios.is_empty() {
        return Err(ContractError::InvalidAmount);
    }
    if denom <= 0 {
        return Err(ContractError::InvalidAmount);
    }

    let n = ratios.len() as usize;

    // Compute floor shares and remainders using fixed-point helpers.
    // `mul_div` performs the same floor division as `total * r / denom` but
    // guards against intermediate overflow, and `mul_mod` gives the exact
    // remainder needed for the largest-remainder step.
    let mut shares = Vec::new(env);
    let mut remainders = Vec::new(env);

    for r in ratios.iter() {
        let share = mul_div(total, r, denom)?;
        let rem = mul_mod(total, r, denom)?;
        shares.push_back(share);
        remainders.push_back(rem);
    }

    // How many extra stroops to distribute?
    let distributed: i128 = shares.iter().sum();
    let leftover = (total - distributed) as usize;

    // Build an index array sorted by remainder descending.
    // We use a fixed-size array (max 64 recipients) for no_std compatibility.
    // Contracts with more than 64 recipients would need a larger cap, but
    // 64 is a reasonable upper bound for on-chain use.
    const MAX_RECIPIENTS: usize = 64;
    if n > MAX_RECIPIENTS {
        return Err(ContractError::InvalidAmount);
    }

    let mut indices = [0usize; MAX_RECIPIENTS];
    for i in 0..n {
        indices[i] = i;
    }

    // Insertion sort by remainder descending (small n, simple is best).
    for i in 1..n {
        let key = indices[i];
        let key_rem = remainders.get(key as u32).unwrap_or(0);
        let mut j = i;
        while j > 0 {
            let prev = indices[j - 1];
            let prev_rem = remainders.get(prev as u32).unwrap_or(0);
            if prev_rem >= key_rem {
                break;
            }
            indices[j] = indices[j - 1];
            j -= 1;
        }
        indices[j] = key;
    }

    // Distribute leftover stroops to top-remainder recipients.
    let mut shares_mut = Vec::new(env);
    for s in shares.iter() {
        shares_mut.push_back(s);
    }

    for i in 0..leftover {
        let idx = indices[i] as u32;
        let current = shares_mut.get(idx).unwrap_or(0);
        shares_mut.set(idx, current + 1);
    }

    Ok(shares_mut)
}

// ---------------------------------------------------------------------------
// Issue #561: Canonical payout ordering
// ---------------------------------------------------------------------------

/// Sort recipients by their Stellar address byte representation (lexicographic
/// order over `Address`'s inner `BytesN<32>`).
///
/// This guarantees deterministic, canonical payout ordering regardless of the
/// order in which recipients were supplied at invoice creation.
///
/// # Arguments
/// * `env`        – Soroban environment
/// * `recipients` – mutable list of recipient addresses to sort in-place
pub fn sort_recipients(env: &Env, recipients: &mut Vec<Address>) {
    let n = recipients.len();
    if n <= 1 {
        return;
    }

    // Soroban Map maintains keys in canonical (XDR-sorted) order.
    // Inserting all addresses as keys then iterating produces deterministic ordering.
    let mut ordered: Map<Address, u32> = Map::new(env);
    for i in 0..n {
        let addr = recipients.get(i).unwrap();
        ordered.set(addr, i);
    }

    let mut sorted = Vec::new(env);
    for (addr, _) in ordered.iter() {
        sorted.push_back(addr);
    }
    *recipients = sorted;
}

// ---------------------------------------------------------------------------
// Issue #705: Invoice funding completion helper
// ---------------------------------------------------------------------------

/// Compute the funding completion of an invoice in basis points.
///
/// Returns `funded * 10_000 / total`, clamped to `[0, 10_000]`.
///
/// # Edge cases
/// * Returns `0` when `total <= 0` (nothing to fund).
/// * Returns `0` when `funded <= 0`.
/// * Returns `10_000` when `funded >= total` (fully funded or overfunded).
///
/// # Examples
/// ```ignore
/// assert_eq!(funding_bps(500, 1000), 5_000);  // 50%
/// assert_eq!(funding_bps(1000, 1000), 10_000); // 100%
/// assert_eq!(funding_bps(1500, 1000), 10_000); // overfunded → clamped
/// assert_eq!(funding_bps(0, 1000), 0);          // nothing paid
/// assert_eq!(funding_bps(500, 0), 0);            // invalid total
/// ```
pub fn funding_bps(funded: i128, total: i128) -> u32 {
    if total <= 0 || funded <= 0 {
        return 0;
    }
    if funded >= total {
        return 10_000;
    }
    // funded < total, both positive — fixed-point helper computes
    // `funded * 10_000 / total` with overflow-safe intermediate math.
    let bps = bps_of(funded, total);
    // bps is in [0, 9_999] since funded < total; the clamp is a safeguard.
    bps.min(10_000) as u32
}

// ---------------------------------------------------------------------------
// Issue #705: Platform fee computation helper
// ---------------------------------------------------------------------------

/// Compute the platform fee for a given funded amount and fee rate.
///
/// Returns `funded * fee_bps / 10_000`, using checked arithmetic to prevent
/// overflow on very large amounts.
///
/// # Arguments
/// * `funded`   – gross collected amount (stroops); must be ≥ 0.
/// * `fee_bps`  – platform fee rate in basis points (0 – 10 000).
///
/// # Errors
/// Returns [`ContractError::ArithmeticOverflow`] when `funded * fee_bps`
/// overflows `i128` (i.e. `funded` is close to `i128::MAX` and `fee_bps > 0`).
pub fn calc_platform_fee(funded: i128, fee_bps: u32) -> Result<i128, ContractError> {
    if fee_bps == 0 || funded == 0 {
        return Ok(0);
    }
    // `mul_div` computes `funded * fee_bps / 10_000` with overflow-safe
    // intermediate arithmetic, preserving the previous checked-mul semantics.
    mul_div(funded, fee_bps as i128, BPS_DENOMINATOR)
}

// ---------------------------------------------------------------------------
// Issue #954 / #956: Payment event breakdown & refund reason helpers
// ---------------------------------------------------------------------------

/// Detailed, indexable breakdown of a single payment event.
///
/// The breakdown is derived from the same arithmetic used to distribute funds
/// (see [`distribute_with_remainder`] and [`calc_platform_fee`]) so that the
/// emitted event always reconciles with the amounts actually moved.
///
/// All fields are plain integers so the struct can be emitted directly as
/// event data and indexed by off-chain consumers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentBreakdown {
    /// Gross amount collected in this payment (stroops).
    pub gross: i128,
    /// Platform fee withheld from `gross` (stroops).
    pub platform_fee: i128,
    /// Net amount distributed to recipients (`gross - platform_fee`).
    pub net: i128,
    /// Number of recipients the net amount was split across.
    pub recipient_count: u32,
    /// Funding completion after this payment, in basis points (0 – 10_000).
    pub funding_bps: u32,
}

impl PaymentBreakdown {
    /// Build a breakdown from a gross payment amount, fee rate and funding state.
    ///
    /// # Errors
    /// Propagates [`ContractError::ArithmeticOverflow`] from [`calc_platform_fee`].
    pub fn new(
        gross: i128,
        fee_bps: u32,
        recipient_count: u32,
        funded_after: i128,
        total: i128,
    ) -> Result<Self, ContractError> {
        let platform_fee = calc_platform_fee(gross, fee_bps)?;
        let net = gross - platform_fee;
        Ok(Self {
            gross,
            platform_fee,
            net,
            recipient_count,
            funding_bps: funding_bps(funded_after, total),
        })
    }

    /// Returns `true` when the breakdown is internally consistent:
    /// `net == gross - platform_fee` and `funding_bps <= 10_000`.
    pub fn is_consistent(&self) -> bool {
        self.net == self.gross - self.platform_fee && self.funding_bps <= 10_000
    }
}

/// Reason a refund was issued, emitted alongside every refund event.
///
/// Kept as a small, explicit enum so the reason is stable, indexable and
/// self-describing to off-chain consumers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefundReason {
    /// Invoice was cancelled before funding completed.
    Cancelled = 0,
    /// Invoice expired without reaching its funding goal.
    Expired = 1,
    /// Payer requested a refund explicitly.
    Requested = 2,
    /// Invoice was overfunded and the excess is being returned.
    Overfunded = 3,
    /// Refund issued due to a dispute resolution.
    Dispute = 4,
}

impl RefundReason {
    /// Stable numeric code suitable for event indexing.
    pub fn code(&self) -> u32 {
        *self as u32
    }
}

/// Compute the platform fee and emit a `fee_calculated` event for it.
///
/// The emitted amount is exactly the value returned, so off-chain indexers
/// observe the same fee the contract will later distribute.
pub fn calc_platform_fee_with_event(
    env: &Env,
    funded: i128,
    fee_bps: u32,
) -> Result<i128, ContractError> {
    let fee = calc_platform_fee(funded, fee_bps)?;
    emit_fee_calculated(env, FeeType::Platform, fee);
    Ok(fee)
}

/// Compute a fee for the given `fee_type` from `funded` and `fee_bps`, emitting
/// a `fee_calculated` event tagged with that type.
///
/// This is the generic entry point used by the distribution helpers so each
/// fee bucket (platform, creator, protocol) produces its own typed event.
pub fn calc_fee_with_event(
    env: &Env,
    fee_type: FeeType,
    funded: i128,
    fee_bps: u32,
) -> Result<i128, ContractError> {
    let fee = calc_platform_fee(funded, fee_bps)?;
    emit_fee_calculated(env, fee_type, fee);
    Ok(fee)
}

/// Distribute a computed fee to `recipient` and emit a `fee_distributed` event.
///
/// The emitted amount is exactly `amount`, which the caller must have already
/// transferred to `recipient`, keeping the event in sync with the actual payout.
pub fn distribute_fee_with_event(
    env: &Env,
    fee_type: FeeType,
    recipient: &Address,
    amount: i128,
) {
    emit_fee_distributed(env, fee_type, recipient, amount);
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::Env;

    fn make_ratios(env: &Env, vals: &[i128]) -> Vec<i128> {
        let mut v = Vec::new(env);
        for &x in vals {
            v.push_back(x);
        }
        v
    }

    /// Assert sum equals total and return shares.
    fn assert_exact(env: &Env, total: i128, ratios: &[i128], denom: i128) -> Vec<i128> {
        let r_vec = make_ratios(env, ratios);
        let result = distribute_with_remainder(env, total, &r_vec, denom)
            .expect("distribute_with_remainder should not fail for valid inputs");
        let sum: i128 = result.iter().sum();
        assert_eq!(
            sum, total,
            "sum {sum} != total {total} for ratios {ratios:?} / {denom}"
        );
        result
    }

    #[test]
    fn test_even_split() {
        let env = Env::default();
        let r = assert_exact(&env, 300, &[1, 1, 1], 3);
        assert_eq!(r.get(0), Some(100));
        assert_eq!(r.get(1), Some(100));
        assert_eq!(r.get(2), Some(100));
    }

    #[test]
    fn test_uneven_split_even_amount() {
        let env = Env::default();
        let r = assert_exact(&env, 100, &[1, 1], 2);
        assert_eq!(r.get(0), Some(50));
        assert_eq!(r.get(1), Some(50));
    }

    #[test]
    fn test_uneven_split_odd_amount() {
        let env = Env::default();
        // 101 split 1:1 → sum must be 101, diff ≤ 1
        let r = assert_exact(&env, 101, &[1, 1], 2);
        let a = r.get(0).unwrap();
        let b = r.get(1).unwrap();
        assert!((a - b).abs() <= 1);
    }

    #[test]
    fn test_three_way_remainder() {
        let env = Env::default();
        // 10 / 3: floors [3,3,3] leftover=1 → one recipient gets 4
        let r = assert_exact(&env, 10, &[1, 1, 1], 3);
        let mut vals = [r.get(0).unwrap(), r.get(1).unwrap(), r.get(2).unwrap()];
        vals.sort();
        assert_eq!(vals, [3, 3, 4]);
    }

    #[test]
    fn test_weighted_split() {
        let env = Env::default();
        // 1000 split 1:2:3 (denom=6)
        // floors: 166, 333, 500 → sum=999, leftover=1
        // remainders: 1000*1%6=4, 1000*2%6=2, 1000*3%6=0
        // index 0 has highest remainder → gets the extra stroop
        let r = assert_exact(&env, 1000, &[1, 2, 3], 6);
        assert_eq!(r.get(0), Some(167));
        assert_eq!(r.get(1), Some(333));
        assert_eq!(r.get(2), Some(500));
    }

    #[test]
    fn test_single_recipient() {
        let env = Env::default();
        let r = assert_exact(&env, 999, &[1], 1);
        assert_eq!(r.get(0), Some(999));
    }

    #[test]
    fn test_zero_total() {
        let env = Env::default();
        let r = assert_exact(&env, 0, &[1, 2, 3], 6);
        assert_eq!(r.get(0), Some(0));
        assert_eq!(r.get(1), Some(0));
        assert_eq!(r.get(2), Some(0));
    }

    #[test]
    fn test_large_amounts() {
        let env = Env::default();
        // 1_000_000_007 stroops across 7 equal recipients
        assert_exact(&env, 1_000_000_007, &[1, 1, 1, 1, 1, 1, 1], 7);
    }

    #[test]
    fn test_large_ratio_values() {
        let env = Env::default();
        // Real invoice: 1_000_000_000 stroops split 100_000:200_000:300_000
        assert_exact(&env, 1_000_000_000, &[100_000, 200_000, 300_000], 600_000);
    }

    #[test]
    fn single_recipient_gets_full_amount() {
        let env = Env::default();
        let r = distribute_with_remainder(&env, 12345, &make_ratios(&env, &[1]), 1).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r.get(0), Some(12345));
    }

    #[test]
    fn sum_invariant_holds_with_unequal_ratios() {
        let env = Env::default();
        // Case 1: 3 recipients with ratios [1, 1, 1] and total=10
        // Total is not evenly divisible by denom (10 % 3 != 0)
        let r1 = distribute_with_remainder(&env, 10, &make_ratios(&env, &[1, 1, 1]), 3).unwrap();
        let sum1: i128 = r1.iter().sum();
        assert_eq!(sum1, 10);

        // Case 2: 4 recipients with ratios [2, 3, 1, 4] and total=100
        let r2 = distribute_with_remainder(&env, 100, &make_ratios(&env, &[2, 3, 1, 4]), 10).unwrap();
        let sum2: i128 = r2.iter().sum();
        assert_eq!(sum2, 100);

        // Case 3: 2 recipients with ratios [1, 3] and total=999
        let r3 = distribute_with_remainder(&env, 999, &make_ratios(&env, &[1, 3]), 4).unwrap();
        let sum3: i128 = r3.iter().sum();
        assert_eq!(sum3, 999);
    }

    /// Property-based style test: exhaustively verify sum == total for many inputs.
    #[test]
    fn test_property_sum_equals_total() {
        let env = Env::default();
        let cases: &[(i128, &[i128], i128)] = &[
            (1, &[1], 1),
            (2, &[1, 1], 2),
            (3, &[1, 1, 1], 3),
            (7, &[1, 2, 4], 7),
            (99, &[3, 5, 7, 11], 26),
            (1000, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10], 55),
            (1_000_000_000, &[1, 3, 5, 7], 16),
            (13, &[2, 3], 5),
            (10_000_007, &[1, 1, 1, 1, 1, 1, 1], 7),
            (1_000_000_000_000i128, &[333_333, 333_333, 333_334], 1_000_000),
        ];

        for &(total, ratios, denom) in cases {
            assert_exact(&env, total, ratios, denom);
        }
    }

    // -----------------------------------------------------------------------
    // funding_bps tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_funding_bps_partial() {
        // 500 funded out of 1000 total → 50% → 5_000 bps
        assert_eq!(funding_bps(500, 1_000), 5_000);
    }

    #[test]
    fn test_funding_bps_full() {
        // Exactly fully funded → 100% → 10_000 bps
        assert_eq!(funding_bps(1_000, 1_000), 10_000);
    }

    #[test]
    fn test_funding_bps_overfunded() {
        // Overfunded → clamped to 10_000
        assert_eq!(funding_bps(1_500, 1_000), 10_000);
    }

    #[test]
    fn test_funding_bps_zero_funded() {
        // Nothing paid yet → 0
        assert_eq!(funding_bps(0, 1_000), 0);
    }

    #[test]
    fn test_funding_bps_zero_total() {
        // Invalid total → 0 to avoid divide-by-zero
        assert_eq!(funding_bps(500, 0), 0);
    }

    #[test]
    fn test_funding_bps_negative_total() {
        assert_eq!(funding_bps(500, -1), 0);
    }

    #[test]
    fn test_funding_bps_one_stroop_below_full() {
        // funded = total - 1 → result must be < 10_000
        let bps = funding_bps(999, 1_000);
        assert!(bps < 10_000);
        assert!(bps > 9_980); // should be ~9_990
    }

    // -----------------------------------------------------------------------
    // calc_platform_fee tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_calc_platform_fee_normal() {
        // 1_000_000 funded at 250 bps (2.5%) → fee = 25_000
        let fee = calc_platform_fee(1_000_000, 250).unwrap();
        assert_eq!(fee, 25_000);
    }

    #[test]
    fn test_calc_platform_fee_zero_bps() {
        // Zero fee rate → always zero fee regardless of funded amount
        assert_eq!(calc_platform_fee(999_999_999, 0).unwrap(), 0);
    }

    #[test]
    fn test_calc_platform_fee_max_bps() {
        // 10_000 bps = 100% → fee equals funded
        assert_eq!(calc_platform_fee(500, 10_000).unwrap(), 500);
    }

    #[test]
    fn test_calc_platform_fee_overflow() {
        // i128::MAX * 2 overflows the intermediate multiplication
        let result = calc_platform_fee(i128::MAX, 2);
        assert_eq!(result, Err(crate::error::ContractError::ArithmeticOverflow));
    }

    // -----------------------------------------------------------------------
    // PaymentBreakdown tests (Issue #954)
    // -----------------------------------------------------------------------

    #[test]
    fn test_payment_breakdown_normal() {
        // 1_000_000 gross at 250 bps, 3 recipients, fully funded afterwards.
        let b = PaymentBreakdown::new(1_000_000, 250, 3, 1_000_000, 1_000_000).unwrap();
        assert_eq!(b.gross, 1_000_000);
        assert_eq!(b.platform_fee, 25_000);
        assert_eq!(b.net, 975_000);
        assert_eq!(b.recipient_count, 3);
        assert_eq!(b.funding_bps, 10_000);
        assert!(b.is_consistent());
    }

    #[test]
    fn test_payment_breakdown_partial_funding() {
        let b = PaymentBreakdown::new(500, 0, 2, 500, 1_000).unwrap();
        assert_eq!(b.platform_fee, 0);
        assert_eq!(b.net, 500);
        assert_eq!(b.funding_bps, 5_000);
        assert!(b.is_consistent());
    }

    #[test]
    fn test_payment_breakdown_overflow() {
        let result = PaymentBreakdown::new(i128::MAX, 2, 1, 0, 0);
        assert_eq!(result, Err(crate::error::ContractError::ArithmeticOverflow));
    }

    #[test]
    fn test_payment_breakdown_reconciles_with_distribution() {
        // Net amount must be exactly distributable across recipients.
        let env = Env::default();
        let b = PaymentBreakdown::new(1_000, 100, 3, 1_000, 1_000).unwrap();
        let shares =
            distribute_with_remainder(&env, b.net, &make_ratios(&env, &[1, 1, 1]), 3).unwrap();
        let sum: i128 = shares.iter().sum();
        assert_eq!(sum, b.net);
    }

    // -----------------------------------------------------------------------
    // RefundReason tests (Issue #956)
    // -----------------------------------------------------------------------

    #[test]
    fn test_refund_reason_codes_are_stable() {
        assert_eq!(RefundReason::Cancelled.code(), 0);
        assert_eq!(RefundReason::Expired.code(), 1);
        assert_eq!(RefundReason::Requested.code(), 2);
        assert_eq!(RefundReason::Overfunded.code(), 3);
        assert_eq!(RefundReason::Dispute.code(), 4);
    }

    #[test]
    fn test_refund_reason_codes_are_unique() {
        let reasons = [
            RefundReason::Cancelled,
            RefundReason::Expired,
            RefundReason::Requested,
            RefundReason::Overfunded,
            RefundReason::Dispute,
        ];
        for (i, a) in reasons.iter().enumerate() {
            for (j, b) in reasons.iter().enumerate() {
                if i != j {
                    assert_ne!(a.code(), b.code());
                }
            }
        }
    }
}
