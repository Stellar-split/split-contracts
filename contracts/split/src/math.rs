//! Issue #555: Token-decimal normalisation helpers.
//!
//! Stellar assets use different decimal precisions — XLM uses 7 (stroops),
//! USDC uses 6, some custom tokens use 2 or 18. When computing each
//! recipient's share from a raw token amount, integer arithmetic on differing
//! scales produces silently wrong results.
//!
//! The contract normalises every raw amount to a fixed 7-decimal canonical
//! representation before any split arithmetic, then denormalises back to the
//! token's native scale before each `token::Client::transfer()` call.
//!
//! ## Normalization flow
//!
//! Every token amount in the contract moves through the same three-stage
//! pipeline so that split math is always performed on a single, comparable
//! scale:
//!
//! 1. **Ingest (native scale).** Amounts arrive from callers / the ledger in
//!    the *token's own* smallest unit (e.g. stroops for XLM at 7 decimals,
//!    base units for USDC at 6). These are never trusted for cross-token
//!    arithmetic as-is.
//! 2. **Normalize → canonical.** [`normalize_amount`] scales each raw amount
//!    up (or down) to the fixed 7-decimal `CANONICAL_DECIMALS` representation
//!    via a `10^(delta)` multiply/divide. All recipient-share computation,
//!    ratio application, fee calculation, and overflow checks happen **here,
//!    in canonical units**, so a 6-decimal and an 18-decimal token compare
//!    correctly.
//! 3. **Denormalize → native.** Before any on-chain movement,
//!    [`denormalize_amount`] reverses the scaling, converting the canonical
//!    result back to the destination token's native smallest unit, which is
//!    what `token::Client::transfer()` expects.
//!
//! The two helpers are exact inverses (`normalize_amount(x, d)` then
//! `denormalize_amount(_, d)` returns `x`), so the round-trip is lossless
//! except for the integer truncation that naturally occurs when a higher
//! decimal token is downscaled (the only place precision is intentionally
//! dropped). Negative inputs are rejected up front because a signed amount
//! has no meaningful decimal-scale meaning here.

use crate::error::ContractError;

/// Canonical internal decimal scale — matches XLM's 7-stroop precision.
/// All split arithmetic is performed in this scale.
pub const CANONICAL_DECIMALS: u32 = 7;

/// Fixed-point scale factor: `10^CANONICAL_DECIMALS`.
///
/// One whole unit is represented as `SCALE` fixed-point base units.
pub const SCALE: i128 = 10_000_000;

/// A fixed-point number scaled by `10^CANONICAL_DECIMALS`.
///
/// The inner `i128` holds the value multiplied by [`SCALE`], so `1.5` is
/// stored as `15_000_000`. All arithmetic is checked and returns
/// `Err(ContractError::ArithmeticOverflow)` on overflow, negative results
/// where they are not meaningful, or division by zero.
///
/// Using fixed-point keeps split math on a single integer scale, avoiding
/// repeated `10^delta` scaling and general integer math in the hot path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FixedPoint(i128);

impl FixedPoint {
    /// The additive identity (`0`).
    pub const ZERO: FixedPoint = FixedPoint(0);

    /// The multiplicative identity (`1.0`).
    pub const ONE: FixedPoint = FixedPoint(SCALE);

    /// Wrap a raw scaled value (already multiplied by [`SCALE`]).
    pub const fn from_raw(raw: i128) -> Self {
        FixedPoint(raw)
    }

    /// Return the raw scaled value (value multiplied by [`SCALE`]).
    pub const fn to_raw(self) -> i128 {
        self.0
    }

    /// Build a fixed-point value from a whole integer amount.
    ///
    /// Returns `Err(ArithmeticOverflow)` if `whole * SCALE` overflows or
    /// `whole` is negative.
    pub fn from_integer(whole: i128) -> Result<Self, ContractError> {
        if whole < 0 {
            return Err(ContractError::ArithmeticOverflow);
        }
        let raw = whole
            .checked_mul(SCALE)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Ok(FixedPoint(raw))
    }

    /// Truncate to a whole integer amount (rounds toward zero).
    pub fn to_integer(self) -> i128 {
        self.0 / SCALE
    }

    /// Checked addition.
    pub fn checked_add(self, other: FixedPoint) -> Result<Self, ContractError> {
        let raw = self
            .0
            .checked_add(other.0)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Ok(FixedPoint(raw))
    }

    /// Checked subtraction. Negative results are rejected.
    pub fn checked_sub(self, other: FixedPoint) -> Result<Self, ContractError> {
        let raw = self
            .0
            .checked_sub(other.0)
            .ok_or(ContractError::ArithmeticOverflow)?;
        if raw < 0 {
            return Err(ContractError::ArithmeticOverflow);
        }
        Ok(FixedPoint(raw))
    }

    /// Checked multiplication of two fixed-point values.
    ///
    /// Computes `(a * b) / SCALE` with a single division, so precision is
    /// preserved to the canonical scale (truncating any sub-scale remainder).
    pub fn checked_mul(self, other: FixedPoint) -> Result<Self, ContractError> {
        let product = self
            .0
            .checked_mul(other.0)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Ok(FixedPoint(product / SCALE))
    }

    /// Checked division of two fixed-point values.
    ///
    /// Computes `(a * SCALE) / b`. Returns `Err(ArithmeticOverflow)` on
    /// division by zero or overflow.
    pub fn checked_div(self, other: FixedPoint) -> Result<Self, ContractError> {
        if other.0 == 0 {
            return Err(ContractError::ArithmeticOverflow);
        }
        let scaled = self
            .0
            .checked_mul(SCALE)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Ok(FixedPoint(scaled / other.0))
    }
}

impl From<FixedPoint> for i128 {
    fn from(value: FixedPoint) -> i128 {
        value.0
    }
}

impl TryFrom<i128> for FixedPoint {
    type Error = ContractError;

    /// Interpret a raw scaled value as a fixed-point number.
    fn try_from(raw: i128) -> Result<Self, ContractError> {
        if raw < 0 {
            return Err(ContractError::ArithmeticOverflow);
        }
        Ok(FixedPoint(raw))
    }
}

/// Basis-point denominator (100% = 10 000 bps).
pub const BPS_DENOMINATOR: i128 = 10_000;

/// Overflow-safe `floor(a * b / d)` for non-negative `a`, `b` and positive `d`.
///
/// Tries the direct product first; if that overflows, decomposes
/// `a = q * d + r` so `a * b / d = q * b + (r * b) / d`, which only overflows
/// when the true result does not fit.
///
/// # Errors
/// [`ContractError::ArithmeticOverflow`] when `d <= 0`, an input is negative
/// and the direct product overflows, or the result does not fit in `i128`.
pub fn mul_div(a: i128, b: i128, d: i128) -> Result<i128, ContractError> {
    if d <= 0 {
        return Err(ContractError::ArithmeticOverflow);
    }
    if let Some(product) = a.checked_mul(b) {
        return Ok(product.div_euclid(d));
    }
    if a < 0 || b < 0 {
        return Err(ContractError::ArithmeticOverflow);
    }
    let q = a / d;
    let r = a % d;
    let whole = q.checked_mul(b).ok_or(ContractError::ArithmeticOverflow)?;
    let frac = r.checked_mul(b).ok_or(ContractError::ArithmeticOverflow)? / d;
    whole
        .checked_add(frac)
        .ok_or(ContractError::ArithmeticOverflow)
}

/// Overflow-safe `(a * b) mod d` for non-negative `a`, `b` and positive `d`.
///
/// This is the remainder left over by [`mul_div`], used by the
/// largest-remainder distribution step.
///
/// # Errors
/// [`ContractError::ArithmeticOverflow`] when `d <= 0`, an input is negative
/// and the direct product overflows, or `d` is too large for the reduced
/// product to fit.
pub fn mul_mod(a: i128, b: i128, d: i128) -> Result<i128, ContractError> {
    if d <= 0 {
        return Err(ContractError::ArithmeticOverflow);
    }
    if let Some(product) = a.checked_mul(b) {
        return Ok(product.rem_euclid(d));
    }
    if a < 0 || b < 0 {
        return Err(ContractError::ArithmeticOverflow);
    }
    let reduced = (a % d)
        .checked_mul(b % d)
        .ok_or(ContractError::ArithmeticOverflow)?;
    Ok(reduced % d)
}

/// `part * 10 000 / whole`, saturating instead of failing.
///
/// Returns `0` when `whole <= 0`. Callers clamp the result to the bps range.
pub fn bps_of(part: i128, whole: i128) -> i128 {
    if whole <= 0 {
        return 0;
    }
    match mul_div(part, BPS_DENOMINATOR, whole) {
        Ok(bps) => bps,
        // Only reachable for `part` near `i128::MAX`; fall back to dividing
        // the denominator first, which loses at most one bps of precision.
        Err(_) => part / (whole / BPS_DENOMINATOR).max(1),
    }
}

/// Scale a raw token amount to the canonical 7-decimal internal representation.
///
/// * `raw`     — the amount in the token's native (smallest) unit.
/// * `decimals` — the token's declared decimal count (e.g. 7 for XLM, 6 for USDC).
///
/// Returns the amount expressed in 7-decimal canonical units, or
/// `Err(ContractError::ArithmeticOverflow)` when the scaling multiplication
/// or division would overflow `i128` bounds.
pub fn normalize_amount(raw: i128, decimals: u32) -> Result<i128, ContractError> {
    if raw < 0 {
        return Err(ContractError::ArithmeticOverflow);
    }
    if decimals == CANONICAL_DECIMALS {
        return Ok(raw);
    }
    if decimals < CANONICAL_DECIMALS {
        let shift = CANONICAL_DECIMALS - decimals;
        let factor = 10i128
            .checked_pow(shift)
            .ok_or(ContractError::ArithmeticOverflow)?;
        let result = raw
            .checked_mul(factor)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Ok(FixedPoint::from_raw(result).to_raw())
    } else {
        let shift = decimals - CANONICAL_DECIMALS;
        let factor = 10i128
            .checked_pow(shift)
            .ok_or(ContractError::ArithmeticOverflow)?;
        let result = raw
            .checked_div(factor)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Ok(FixedPoint::from_raw(result).to_raw())
    }
}

/// Convert a canonical 7-decimal amount back to the token's native scale.
///
/// * `normalized` — the amount in 7-decimal canonical units.
/// * `decimals`   — the token's declared decimal count.
///
/// Returns the amount in the token's native (smallest) unit, or
/// `Err(ContractError::ArithmeticOverflow)` when the scaling multiplication
/// or division would overflow `i128` bounds.
pub fn denormalize_amount(normalized: i128, decimals: u32) -> Result<i128, ContractError> {
    if normalized < 0 {
        return Err(ContractError::ArithmeticOverflow);
    }
    if decimals == CANONICAL_DECIMALS {
        return Ok(normalized);
    }
    if decimals < CANONICAL_DECIMALS {
        let shift = CANONICAL_DECIMALS - decimals;
        let factor = 10i128
            .checked_pow(shift)
            .ok_or(ContractError::ArithmeticOverflow)?;
        let result = normalized
            .checked_div(factor)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Ok(FixedPoint::from_raw(result).to_raw())
    } else {
        let shift = decimals - CANONICAL_DECIMALS;
        let factor = 10i128
            .checked_pow(shift)
            .ok_or(ContractError::ArithmeticOverflow)?;
        let result = normalized
            .checked_mul(factor)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Ok(FixedPoint::from_raw(result).to_raw())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_7_decimal_is_identity() {
        assert_eq!(normalize_amount(1_000_000, 7).unwrap(), 1_000_000);
        assert_eq!(normalize_amount(0, 7).unwrap(), 0);
    }

    #[test]
    fn normalize_6_decimal_upscales() {
        // USDC: 1_000_000 raw (1 USDC) -> 10_000_000 canonical (1 XLM-stroop-equivalent)
        assert_eq!(normalize_amount(1_000_000, 6).unwrap(), 10_000_000);
    }

    #[test]
    fn normalize_2_decimal_upscales() {
        // 2-decimal token: 100 raw -> 1_000_000 canonical
        assert_eq!(normalize_amount(100, 2).unwrap(), 1_000_000);
    }

    #[test]
    fn normalize_18_decimal_downscales() {
        // 18-decimal token: 1_000_000_000_000_000_000 raw (1 token) -> 10_000_000 canonical
        assert_eq!(normalize_amount(1_000_000_000_000_000_000, 18).unwrap(), 10_000_000);
    }

    #[test]
    fn denormalize_roundtrip_6() {
        let raw = 1_000_000i128;
        let normalized = normalize_amount(raw, 6).unwrap();
        let back = denormalize_amount(normalized, 6).unwrap();
        assert_eq!(back, raw);
    }

    #[test]
    fn denormalize_roundtrip_2() {
        let raw = 100i128;
        let normalized = normalize_amount(raw, 2).unwrap();
        let back = denormalize_amount(normalized, 2).unwrap();
        assert_eq!(back, raw);
    }

    #[test]
    fn denormalize_roundtrip_18() {
        let raw = 1_000_000_000_000_000_000i128;
        let normalized = normalize_amount(raw, 18).unwrap();
        let back = denormalize_amount(normalized, 18).unwrap();
        assert_eq!(back, raw);
    }

    #[test]
    fn normalize_negative_returns_error() {
        assert_eq!(
            normalize_amount(-1, 7),
            Err(ContractError::ArithmeticOverflow)
        );
    }

    #[test]
    fn denormalize_negative_returns_error() {
        assert_eq!(
            denormalize_amount(-1, 7),
            Err(ContractError::ArithmeticOverflow)
        );
    }

    // -----------------------------------------------------------------------
    // denormalize_amount standalone tests
    // -----------------------------------------------------------------------

    /// d=7: identity — normalized value is returned unchanged.
    #[test]
    fn denormalize_7_decimal_is_identity() {
        assert_eq!(denormalize_amount(1_000_000, 7).unwrap(), 1_000_000);
        assert_eq!(denormalize_amount(0, 7).unwrap(), 0);
    }

    /// Round-trip for d=0: token with no decimals (whole units only).
    /// normalize: raw * 10^7  →  denormalize: / 10^7 → raw
    #[test]
    fn denormalize_roundtrip_0() {
        let raw = 42i128;
        let normalized = normalize_amount(raw, 0).unwrap();
        let back = denormalize_amount(normalized, 0).unwrap();
        assert_eq!(back, raw);
    }

    /// Round-trip for d=7: canonical scale, no conversion needed.
    #[test]
    fn denormalize_roundtrip_7() {
        let raw = 5_000_000i128;
        let normalized = normalize_amount(raw, 7).unwrap();
        let back = denormalize_amount(normalized, 7).unwrap();
        assert_eq!(back, raw);
    }

    /// Round-trip for d=8: one decimal place above canonical.
    /// normalize: raw / 10  →  denormalize: * 10 → raw
    #[test]
    fn denormalize_roundtrip_8() {
        // Use a value divisible by 10 so the divide-then-multiply round-trip is exact.
        let raw = 1_000_000_000i128; // 10 tokens at 8-decimal precision
        let normalized = normalize_amount(raw, 8).unwrap();
        let back = denormalize_amount(normalized, 8).unwrap();
        assert_eq!(back, raw);
    }

    /// Overflow: scaling a very large normalized value up (decimals > 7) wraps
    /// past i128::MAX and must return Err(ArithmeticOverflow).
    #[test]
    fn denormalize_overflow_large_value() {
        // With decimals=18 the factor is 10^11. i128::MAX / 10^11 ≈ 1.7e27,
        // so any value above that threshold will overflow on checked_mul.
        let huge: i128 = i128::MAX / 10 + 1; // definitely overflows * 10^11
        assert_eq!(
            denormalize_amount(huge, 18),
            Err(ContractError::ArithmeticOverflow)
        );
    }

    // -----------------------------------------------------------------------
    // Boundary conditions required by acceptance criteria
    // -----------------------------------------------------------------------

    /// decimals == 0: normalize must scale raw up by exactly 10^7.
    #[test]
    fn normalize_0_decimal_scales_up_by_10_pow_7() {
        assert_eq!(normalize_amount(1, 0).unwrap(), 10_000_000);
        assert_eq!(normalize_amount(0, 0).unwrap(), 0);
        assert_eq!(normalize_amount(3, 0).unwrap(), 30_000_000);
    }

    /// decimals == 0: denormalize must scale canonical amount down by exactly 10^7.
    #[test]
    fn denormalize_0_decimal_scales_down_by_10_pow_7() {
        assert_eq!(denormalize_amount(10_000_000, 0).unwrap(), 1);
        assert_eq!(denormalize_amount(0, 0).unwrap(), 0);
        assert_eq!(denormalize_amount(30_000_000, 0).unwrap(), 3);
    }

    /// decimals == CANONICAL_DECIMALS (7): normalize returns raw unchanged, including i128::MAX.
    #[test]
    fn normalize_canonical_decimals_is_identity_including_max() {
        assert_eq!(
            normalize_amount(i128::MAX, CANONICAL_DECIMALS).unwrap(),
            i128::MAX
        );
    }

    /// decimals == CANONICAL_DECIMALS (7): denormalize returns normalized unchanged, including i128::MAX.
    #[test]
    fn denormalize_canonical_decimals_is_identity_including_max() {
        assert_eq!(
            denormalize_amount(i128::MAX, CANONICAL_DECIMALS).unwrap(),
            i128::MAX
        );
    }

    /// decimals == 18: denormalize scales canonical units up by 10^11.
    #[test]
    fn denormalize_18_decimal_scales_up_by_10_pow_11() {
        // 10_000_000 canonical (1 token at 7 decimals) -> 1_000_000_000_000_000_000 (1 token at 18 decimals)
        assert_eq!(
            denormalize_amount(10_000_000, 18).unwrap(),
            1_000_000_000_000_000_000
        );
        assert_eq!(denormalize_amount(0, 18).unwrap(), 0);
    }

    /// raw < 0: normalize returns Err(ArithmeticOverflow) regardless of decimals.
    #[test]
    fn normalize_negative_raw_always_errors() {
        // identity path (decimals == 7)
        assert_eq!(normalize_amount(-1, 7), Err(ContractError::ArithmeticOverflow));
        // upscale path (decimals < 7)
        assert_eq!(normalize_amount(-1, 0), Err(ContractError::ArithmeticOverflow));
        assert_eq!(normalize_amount(-1, 6), Err(ContractError::ArithmeticOverflow));
        // downscale path (decimals > 7)
        assert_eq!(normalize_amount(-1, 18), Err(ContractError::ArithmeticOverflow));
    }

    /// Very large raw values overflow when upscaling and must return Err(ArithmeticOverflow).
    #[test]
    fn normalize_i128_max_overflows_on_upscale() {
        // decimals == 0: factor = 10^7; i128::MAX * 10^7 overflows u128 as well.
        assert_eq!(
            normalize_amount(i128::MAX, 0),
            Err(ContractError::ArithmeticOverflow)
        );
        // decimals == 6: factor = 10^1; i128::MAX * 10 overflows.
        assert_eq!(
            normalize_amount(i128::MAX, 6),
            Err(ContractError::ArithmeticOverflow)
        );
    }

    /// Very large normalized values overflow when upscaling in denormalize.
    #[test]
    fn denormalize_i128_max_overflows_on_upscale() {
        // decimals == 18: factor = 10^11; i128::MAX * 10^11 overflows.
        assert_eq!(
            denormalize_amount(i128::MAX, 18),
            Err(ContractError::ArithmeticOverflow)
        );
        // decimals == 0: factor = 10^7 in the multiply branch? No — decimals==0 < 7 so
        // denormalize _divides_ by 10^7, which never overflows. Verify it succeeds instead.
        // (The overflow case for denormalize/0 is not possible by construction.)
        assert!(denormalize_amount(i128::MAX, 0).is_ok());
    }

    // -----------------------------------------------------------------------
    // FixedPoint arithmetic tests (#947)
    // -----------------------------------------------------------------------

    #[test]
    fn fixed_point_constants() {
        assert_eq!(FixedPoint::ZERO.to_raw(), 0);
        assert_eq!(FixedPoint::ONE.to_raw(), SCALE);
        assert_eq!(SCALE, 10_000_000);
    }

    #[test]
    fn fixed_point_from_integer_and_back() {
        assert_eq!(FixedPoint::from_integer(0).unwrap().to_raw(), 0);
        assert_eq!(FixedPoint::from_integer(1).unwrap().to_raw(), SCALE);
        assert_eq!(FixedPoint::from_integer(42).unwrap().to_integer(), 42);
    }

    #[test]
    fn fixed_point_from_integer_negative_errors() {
        assert_eq!(
            FixedPoint::from_integer(-1),
            Err(ContractError::ArithmeticOverflow)
        );
    }

    #[test]
    fn fixed_point_from_integer_overflow_errors() {
        assert_eq!(
            FixedPoint::from_integer(i128::MAX),
            Err(ContractError::ArithmeticOverflow)
        );
    }

    #[test]
    fn fixed_point_try_from_raw() {
        assert_eq!(FixedPoint::try_from(SCALE).unwrap(), FixedPoint::ONE);
        assert_eq!(
            FixedPoint::try_from(-1),
            Err(ContractError::ArithmeticOverflow)
        );
        let raw: i128 = FixedPoint::ONE.into();
        assert_eq!(raw, SCALE);
    }

    #[test]
    fn fixed_point_add_sub() {
        let a = FixedPoint::from_integer(3).unwrap();
        let b = FixedPoint::from_integer(2).unwrap();
        assert_eq!(a.checked_add(b).unwrap().to_integer(), 5);
        assert_eq!(a.checked_sub(b).unwrap().to_integer(), 1);
    }

    #[test]
    fn fixed_point_sub_negative_errors() {
        let a = FixedPoint::from_integer(1).unwrap();
        let b = FixedPoint::from_integer(2).unwrap();
        assert_eq!(a.checked_sub(b), Err(ContractError::ArithmeticOverflow));
    }

    #[test]
    fn fixed_point_add_overflow_errors() {
        let a = FixedPoint::from_raw(i128::MAX);
        let b = FixedPoint::from_raw(1);
        assert_eq!(a.checked_add(b), Err(ContractError::ArithmeticOverflow));
    }

    #[test]
    fn fixed_point_mul() {
        let a = FixedPoint::from_integer(3).unwrap();
        let b = FixedPoint::from_integer(4).unwrap();
        assert_eq!(a.checked_mul(b).unwrap().to_integer(), 12);
    }

    #[test]
    fn fixed_point_mul_fractional() {
        // 1.5 * 2.0 = 3.0
        let a = FixedPoint::from_raw(15_000_000);
        let b = FixedPoint::from_integer(2).unwrap();
        assert_eq!(a.checked_mul(b).unwrap().to_raw(), 30_000_000);
    }

    #[test]
    fn fixed_point_mul_rounds_toward_zero() {
        // 0.0000001 * 0.0000001 = 1e-14, which truncates to 0 at canonical scale.
        let tiny = FixedPoint::from_raw(1);
        assert_eq!(tiny.checked_mul(tiny).unwrap().to_raw(), 0);
    }

    #[test]
    fn fixed_point_mul_overflow_errors() {
        let a = FixedPoint::from_raw(i128::MAX);
        let b = FixedPoint::from_integer(2).unwrap();
        assert_eq!(a.checked_mul(b), Err(ContractError::ArithmeticOverflow));
    }

    #[test]
    fn fixed_point_div() {
        let a = FixedPoint::from_integer(12).unwrap();
        let b = FixedPoint::from_integer(4).unwrap();
        assert_eq!(a.checked_div(b).unwrap().to_integer(), 3);
    }

    #[test]
    fn fixed_point_div_fractional() {
        // 1.0 / 2.0 = 0.5
        let a = FixedPoint::ONE;
        let b = FixedPoint::from_integer(2).unwrap();
        assert_eq!(a.checked_div(b).unwrap().to_raw(), 5_000_000);
    }

    #[test]
    fn fixed_point_div_by_zero_errors() {
        assert_eq!(
            FixedPoint::ONE.checked_div(FixedPoint::ZERO),
            Err(ContractError::ArithmeticOverflow)
        );
    }

    #[test]
    fn fixed_point_div_overflow_errors() {
        let a = FixedPoint::from_raw(i128::MAX);
        assert_eq!(
            a.checked_div(FixedPoint::ONE),
            Err(ContractError::ArithmeticOverflow)
        );
    }

    #[test]
    fn fixed_point_mul_div_roundtrip_accuracy() {
        // (a * b) / b == a for values that divide evenly at canonical scale.
        let a = FixedPoint::from_integer(7).unwrap();
        let b = FixedPoint::from_integer(3).unwrap();
        let product = a.checked_mul(b).unwrap();
        assert_eq!(product.checked_div(b).unwrap(), a);
    }

    #[test]
    fn normalize_denormalize_roundtrip_via_fixed_point() {
        // Round-trip through FixedPoint must preserve the canonical value.
        let raw = 1_234_567i128;
        let normalized = normalize_amount(raw, 6).unwrap();
        let fp = FixedPoint::try_from(normalized).unwrap();
        assert_eq!(fp.to_raw(), normalized);
        assert_eq!(denormalize_amount(fp.to_raw(), 6).unwrap(), raw);
    }
}
