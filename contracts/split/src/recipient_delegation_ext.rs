//! Issue #870: Recipient delegation with performance tracking.
//!
//! A recipient can delegate their payout to another address for a specific
//! invoice. The delegation can be time-limited via `expires_at`. Performance
//! metrics are tracked across all invoices a recipient is listed on.

use super::*;
use crate::events;
use crate::storage_keys::{AddressKey, CompoundKey};
use soroban_sdk::{contractimpl, panic_with_error, Address, Env};

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Resolve the effective payout address for a recipient on an invoice.
/// Returns the delegate if a valid, non-expired delegation exists;
/// otherwise returns the original recipient address.
pub(crate) fn resolve_payout_address(env: &Env, invoice_id: u64, recipient: &Address) -> Address {
    let key = CompoundKey::RecipientDelegation(invoice_id, recipient.clone());
    let delegation: RecipientDelegation = match env.storage().persistent().get(&key) {
        Some(d) => d,
        None => return recipient.clone(),
    };
    // Check expiry.
    if let Some(expires_at) = delegation.expires_at {
        if env.ledger().timestamp() > expires_at {
            return recipient.clone();
        }
    }
    delegation.delegate
}

/// Record a payout event for performance tracking on a recipient.
pub(crate) fn record_recipient_payout(
    env: &Env,
    recipient: &Address,
    amount: i128,
    released: bool,
    refunded: bool,
) {
    let key = AddressKey::RecipientPerformance(recipient.clone());
    let mut perf: RecipientPerformance = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_default();
    perf.invoices_listed = perf.invoices_listed.saturating_add(1);
    if released {
        perf.invoices_released = perf.invoices_released.saturating_add(1);
        perf.total_received = perf.total_received.saturating_add(amount);
    }
    if refunded {
        perf.invoices_refunded = perf.invoices_refunded.saturating_add(1);
    }
    env.storage().persistent().set(&key, &perf);
    events::recipient_performance_updated(
        env,
        recipient,
        perf.invoices_released,
        perf.total_received,
    );
}

// ---------------------------------------------------------------------------
// Contract entry points
// ---------------------------------------------------------------------------

#[contractimpl]
impl SplitContract {
    /// Set a delegation for a recipient's payout on a specific invoice.
    /// Only the recipient themselves can set their delegation.
    pub fn set_recipient_delegation(
        env: Env,
        invoice_id: u64,
        recipient: Address,
        delegate: Address,
        expires_at: Option<u64>,
    ) {
        require_not_paused(&env);
        recipient.require_auth();
        // Validate the invoice exists and recipient is on it.
        let invoice = load_invoice(&env, invoice_id);
        if !invoice.recipients.contains(&recipient) {
            panic_with_error!(&env, ContractError::RecipientNotFound);
        }
        let delegation = RecipientDelegation {
            delegate: delegate.clone(),
            set_at: env.ledger().timestamp(),
            expires_at,
        };
        env.storage().persistent().set(
            &CompoundKey::RecipientDelegation(invoice_id, recipient.clone()),
            &delegation,
        );
        events::recipient_delegation_set(&env, invoice_id, &recipient, &delegate, &expires_at);
    }

    /// Revoke a delegation (recipient only).
    pub fn revoke_recipient_delegation(env: Env, invoice_id: u64, recipient: Address) {
        require_not_paused(&env);
        recipient.require_auth();
        let key = CompoundKey::RecipientDelegation(invoice_id, recipient.clone());
        if !env.storage().persistent().has(&key) {
            panic_with_error!(&env, ContractError::NoDelegation);
        }
        env.storage().persistent().remove(&key);
        events::recipient_delegation_revoked(&env, invoice_id, &recipient);
    }

    /// Get the delegation record for a recipient on an invoice.
    pub fn get_recipient_delegation(
        env: Env,
        invoice_id: u64,
        recipient: Address,
    ) -> Option<RecipientDelegation> {
        env.storage()
            .persistent()
            .get(&CompoundKey::RecipientDelegation(invoice_id, recipient))
    }

    /// Resolve the effective payout address for a recipient (considering active delegation).
    pub fn resolve_recipient_address(env: Env, invoice_id: u64, recipient: Address) -> Address {
        resolve_payout_address(&env, invoice_id, &recipient)
    }

    /// Get aggregate performance metrics for a recipient.
    pub fn get_recipient_performance(env: Env, recipient: Address) -> RecipientPerformance {
        env.storage()
            .persistent()
            .get(&AddressKey::RecipientPerformance(recipient))
            .unwrap_or_default()
    }

    /// Update performance metrics for a recipient (admin only).
    /// Typically called after release/refund events to track recipient history.
    pub fn record_recipient_performance(
        env: Env,
        caller: Address,
        recipient: Address,
        amount: i128,
        released: bool,
        refunded: bool,
    ) {
        require_not_paused(&env);
        caller.require_auth();
        let admin: Address = env
            .storage()
            .instance()
            .get(&admin_key())
            .expect("admin not initialised");
        if caller != admin {
            panic_with_error!(&env, ContractError::NotAuthorized);
        }
        record_recipient_payout(&env, &recipient, amount, released, refunded);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{client, make_invoice, setup_initialized};
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn set_and_get_delegation() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let recipient = Address::generate(&env);
        let delegate = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &recipient, 1000, &token, 9_999);
        c.set_recipient_delegation(&id, &recipient, &delegate, &None);
        let d = c.get_recipient_delegation(&id, &recipient).expect("delegation");
        assert_eq!(d.delegate, delegate);
        assert_eq!(d.expires_at, None);
    }

    #[test]
    fn revoke_delegation() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let recipient = Address::generate(&env);
        let delegate = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &recipient, 1000, &token, 9_999);
        c.set_recipient_delegation(&id, &recipient, &delegate, &None);
        c.revoke_recipient_delegation(&id, &recipient);
        assert!(c.get_recipient_delegation(&id, &recipient).is_none());
    }

    #[test]
    fn resolve_returns_delegate_when_set() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let recipient = Address::generate(&env);
        let delegate = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &recipient, 1000, &token, 9_999);
        c.set_recipient_delegation(&id, &recipient, &delegate, &None);
        let resolved = c.resolve_recipient_address(&id, &recipient);
        assert_eq!(resolved, delegate);
    }

    #[test]
    fn resolve_returns_recipient_when_no_delegation() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let recipient = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &recipient, 1000, &token, 9_999);
        let resolved = c.resolve_recipient_address(&id, &recipient);
        assert_eq!(resolved, recipient);
    }

    #[test]
    fn performance_defaults_to_zero() {
        let (env, cid, _token) = setup_initialized();
        let c = client(&env, &cid);
        let addr = Address::generate(&env);
        let perf = c.get_recipient_performance(&addr);
        assert_eq!(perf.invoices_listed, 0);
        assert_eq!(perf.total_received, 0);
        assert_eq!(perf.invoices_released, 0);
        assert_eq!(perf.invoices_refunded, 0);
    }

    #[test]
    fn revoke_no_delegation_errors() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let recipient = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &recipient, 1000, &token, 9_999);
        assert!(c.try_revoke_recipient_delegation(&id, &recipient).is_err());
    }

    #[test]
    fn delegation_with_expiry() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let recipient = Address::generate(&env);
        let delegate = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &recipient, 1000, &token, 9_999);
        let now = env.ledger().timestamp();
        c.set_recipient_delegation(&id, &recipient, &delegate, &Some(now + 1000));
        let d = c.get_recipient_delegation(&id, &recipient).expect("delegation");
        assert_eq!(d.expires_at, Some(now + 1000));
    }
}
