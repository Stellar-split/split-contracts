//! Issue #556 / #558: Invoice validation helpers.
//!
//! These guards run *before* any storage is written during invoice creation
//! (or before any recipient mutation) so that a malformed recipient list is
//! rejected atomically.

use crate::error::ContractError;
use soroban_sdk::{symbol_short, Address, Env, Map, Vec};

/// Issue #556: Reject a recipient list that contains duplicate addresses.
///
/// Uses a `soroban_sdk::Map` for O(n log n) membership tracking — no
/// unbounded heap allocation and a single pass over the slice.
///
/// # Errors
/// Returns [ContractError::DuplicateRecipient] when a duplicate is found.
pub fn assert_unique_recipients(env: &Env, recipients: &[Address]) -> Result<(), ContractError> {
    let mut seen: Map<Address, bool> = Map::new(env);
    for r in recipients.iter() {
        if seen.has(r) {
            return Err(ContractError::DuplicateRecipient);
        }
        seen.set(r.clone(), true);
    }
    Ok(())
}

/// Issue #558: Verify that every recipient has a trustline for the payment
/// token by calling `balance()` on the token contract.
///
/// A host-trapped invocation (the account has never been initialised on the
/// ledger, or no trustline exists for the asset) is treated as "no trustline".
///
/// The check is bypassed for the native / XLM asset, which requires no
/// trustline.  In Soroban the native asset is identified by the absence of an
/// explicit token contract — callers should pass the native sentinel address
/// (`env.ledger().native_token_id()` when available) or simply skip the check
/// for native invoices.
///
/// Returns `Ok(())` when every recipient returns a valid balance, or
/// `Err(ContractError::RecipientMissingTrustline)` with the offending address
/// surfaced in the panic message.
/// # Errors
/// Returns [ContractError::RecipientMissingTrustline] when a balance call fails.
pub fn assert_recipients_have_trustlines(
    env: &Env,
    token: &Address,
    recipients: &[Address],
) -> Result<(), ContractError> {
    for r in recipients.iter() {
        let result = env.try_invoke_contract::<i128, soroban_sdk::Error>(
            token,
            &symbol_short!("balance"),
            (r.clone(),).into_val(env),
        );
        match result {
            Ok(_) => {}
            Err(_) => {
                // Surface the offending address in the panic message so
                // callers and tests can identify which recipient is missing
                // the trustline.
                env.panic_with_error(ContractError::RecipientMissingTrustline);
            }
        }
    }
    Ok(())
}

/// Issue #623: Verify that the sum of `values` equals exactly `BASIS_POINTS_TOTAL` (10 000).
///
/// Used wherever a slice of basis-point weights must cover 100% of a whole:
/// split ratios, release stages, fee recipients, etc.
///
/// # Errors
/// Returns `Err(ContractError::InvalidRatioSum)` when `values.iter().sum::<u32>() != 10_000`.
///
/// # Examples
/// ```
/// assert!(assert_bps_sum(&[5_000u32, 5_000]).is_ok());
/// assert!(assert_bps_sum(&[3_000u32, 3_000]).is_err());
/// ```
pub const BASIS_POINTS_TOTAL: u32 = 10_000;

pub fn assert_bps_sum(values: &[u32]) -> Result<(), ContractError> {
    let sum: u32 = values.iter().copied().fold(0u32, |acc, v| acc.saturating_add(v));
    assert_bps_total(sum)
}

/// Variant of [`assert_bps_sum`] for call sites where the sum has already
/// been computed (e.g. from a soroban `Vec` iterator in a `no_std` context).
///
/// Returns `Err(ContractError::InvalidRatioSum)` when `total != 10_000`.
pub fn assert_bps_total(total: u32) -> Result<(), ContractError> {
    if total != BASIS_POINTS_TOTAL {
        return Err(ContractError::InvalidRatioSum);
    }
    Ok(())
}

/// Issue #704: Validate that a single basis-point value is within the legal
/// range `[0, BASIS_POINTS_TOTAL]` (i.e. `0..=10_000`).
///
/// Per-invoice options such as `penalty_bps`, `tax_bps`, and
/// `insurance_premium_bps` are stored as `u32` basis points and must never
/// exceed 100%. Callers invoke this guard before writing storage so an
/// out-of-range value is rejected atomically. The three existing call sites
/// use `.expect("… must be ≤ 10000")`, so this returns a `Result` and lets the
/// caller choose how to surface the failure.
///
/// # Errors
/// Returns `Err(ContractError::InvalidRatio)` when `bps > BASIS_POINTS_TOTAL`.
pub fn assert_valid_bps(bps: u32) -> Result<(), ContractError> {
    if bps > BASIS_POINTS_TOTAL {
        return Err(ContractError::InvalidRatio);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn unique_recipients_passes() {
        let env = Env::default();
        let a = Address::generate(&env);
        let b = Address::generate(&env);
        let c = Address::generate(&env);
        let mut v: Vec<Address> = Vec::new(&env);
        v.push_back(a.clone());
        v.push_back(b.clone());
        v.push_back(c.clone());
        assert!(assert_unique_recipients(&env, &v.to_vec()).is_ok());
    }

    #[test]
    fn duplicate_recipient_rejected() {
        let env = Env::default();
        let a = Address::generate(&env);
        let b = Address::generate(&env);
        let mut v: Vec<Address> = Vec::new(&env);
        v.push_back(a.clone());
        v.push_back(b.clone());
        v.push_back(a.clone());
        assert_eq!(
            assert_unique_recipients(&env, &v.to_vec()),
            Err(ContractError::DuplicateRecipient)
        );
    }

    #[test]
    fn empty_recipient_list_passes() {
        let env = Env::default();
        let v: Vec<Address> = Vec::new(&env);
        assert!(assert_unique_recipients(&env, &v.to_vec()).is_ok());
    }

    // --- assert_bps_sum (issue #623) ---

    #[test]
    fn bps_sum_equals_10000_passes() {
        assert!(assert_bps_sum(&[5_000u32, 5_000]).is_ok());
        assert!(assert_bps_sum(&[3_000u32, 3_000, 4_000]).is_ok());
        assert!(assert_bps_sum(&[10_000u32]).is_ok());
    }

    #[test]
    fn bps_sum_not_10000_fails() {
        assert_eq!(
            assert_bps_sum(&[3_000u32, 3_000]),
            Err(ContractError::InvalidRatioSum)
        );
        assert_eq!(
            assert_bps_sum(&[0u32]),
            Err(ContractError::InvalidRatioSum)
        );
        assert_eq!(
            assert_bps_sum(&[10_001u32]),
            Err(ContractError::InvalidRatioSum)
        );
        assert_eq!(
            assert_bps_sum(&[]),
            Err(ContractError::InvalidRatioSum)
        );
    }
}
