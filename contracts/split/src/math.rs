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
        let factor = 10u128.pow(shift);
        let result = (raw as u128)
            .checked_mul(factor)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Ok(result as i128)
    } else {
        let shift = decimals - CANONICAL_DECIMALS;
        let factor = 10u128.pow(shift);
        let result = (raw as u128)
            .checked_div(factor)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Ok(result as i128)
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
        let factor = 10u128.pow(shift);
        let result = (normalized as u128)
            .checked_div(factor)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Ok(result as i128)
    } else {
        let shift = decimals - CANONICAL_DECIMALS;
        let factor = 10u128.pow(shift);
        let result = (normalized as u128)
            .checked_mul(factor)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Ok(result as i128)
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
}
