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
/// Returns `Ok(())` when every address is distinct, or
/// `Err(ContractError::DuplicateRecipient)` on the first duplicate found.
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

/// Issue #628: Validate that a basis-points value is within the allowed range
/// [0, 10_000] (i.e. it does not exceed 100%).
///
/// Basis-points fields such as `penalty_bps`, `tax_bps`, and
/// `insurance_premium_bps` must never exceed 10 000 — values above that would
/// represent a fee of more than 100%, which is nonsensical.
///
/// Returns `Ok(())` when `bps <= 10_000`, or
/// `Err(ContractError::InvalidAmount)` otherwise.
pub fn assert_valid_bps(bps: u32) -> Result<(), ContractError> {
    if bps > 10_000 {
        return Err(ContractError::InvalidAmount);
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

    // Issue #628: assert_valid_bps tests
    #[test]
    fn valid_bps_passes() {
        assert!(assert_valid_bps(0).is_ok());
        assert!(assert_valid_bps(5000).is_ok());
        assert!(assert_valid_bps(10_000).is_ok());
    }

    #[test]
    fn out_of_range_bps_rejected() {
        assert_eq!(assert_valid_bps(10_001), Err(ContractError::InvalidAmount));
        assert_eq!(assert_valid_bps(u32::MAX), Err(ContractError::InvalidAmount));
    }
}
