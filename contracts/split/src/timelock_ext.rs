//! Issue #871: Invoice time-lock mechanisms.
//!
//! A creator can place a time-lock on an invoice that blocks certain operations
//! (payments, release, refund) until a specified `unlock_at` Unix timestamp.
//! The lock can be removed early by the creator before it expires.

use super::*;
use crate::events;
use crate::storage_keys::InvoiceKey;
use soroban_sdk::{contractimpl, panic_with_error, symbol_short, Address, Env, Symbol};

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn timelock_key(invoice_id: u64) -> InvoiceKey {
    InvoiceKey::TimeLock(invoice_id)
}

/// Which action to check against the time-lock.
pub(crate) enum TimeLockCheck {
    Pay,
    Release,
    Refund,
}

/// Assert that a time-lock does not currently block the given action.
/// No-ops when no lock is set or when the lock has already expired.
/// Panics with `TimeLockActive` when the lock is active and the action
/// is in the locked set.
pub(crate) fn assert_timelock_permits(env: &Env, invoice_id: u64, action: TimeLockCheck) {
    let key = timelock_key(invoice_id);
    let lock: InvoiceTimeLock = match env.storage().persistent().get(&key) {
        Some(l) => l,
        None => return, // no lock configured
    };
    let now = env.ledger().timestamp();
    if now >= lock.unlock_at {
        return; // lock has expired
    }
    let blocked = match action {
        TimeLockCheck::Pay => lock.lock_payments,
        TimeLockCheck::Release => lock.lock_release,
        TimeLockCheck::Refund => lock.lock_refund,
    };
    if blocked {
        panic_with_error!(env, ContractError::TimeLockActive);
    }
}

// ---------------------------------------------------------------------------
// Contract entry points
// ---------------------------------------------------------------------------

#[contractimpl]
impl SplitContract {
    /// Apply a time-lock to an invoice (creator only).
    ///
    /// `unlock_at` must be strictly in the future. Each of `lock_payments`,
    /// `lock_release`, and `lock_refund` may be set independently.
    pub fn set_invoice_timelock(
        env: Env,
        creator: Address,
        invoice_id: u64,
        unlock_at: u64,
        lock_payments: bool,
        lock_release: bool,
        lock_refund: bool,
    ) {
        require_not_paused(&env);
        creator.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        if invoice.creator != creator {
            panic_with_error!(&env, ContractError::NotAuthorized);
        }
        if unlock_at <= env.ledger().timestamp() {
            panic_with_error!(&env, ContractError::InvalidAmount);
        }
        let lock = InvoiceTimeLock {
            unlock_at,
            lock_payments,
            lock_release,
            lock_refund,
            set_by: creator,
        };
        env.storage()
            .persistent()
            .set(&timelock_key(invoice_id), &lock);
        events::timelock_set(
            &env,
            invoice_id,
            unlock_at,
            lock_payments,
            lock_release,
            lock_refund,
        );
    }

    /// Remove a time-lock before it expires (creator only).
    pub fn remove_invoice_timelock(env: Env, creator: Address, invoice_id: u64) {
        require_not_paused(&env);
        creator.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        if invoice.creator != creator {
            panic_with_error!(&env, ContractError::NotAuthorized);
        }
        let key = timelock_key(invoice_id);
        if !env.storage().persistent().has(&key) {
            panic_with_error!(&env, ContractError::NoTimeLock);
        }
        env.storage().persistent().remove(&key);
        events::timelock_removed(&env, invoice_id);
    }

    /// Get the time-lock configuration for an invoice.
    /// Returns `None` if no lock is configured.
    pub fn get_invoice_timelock(env: Env, invoice_id: u64) -> Option<InvoiceTimeLock> {
        env.storage()
            .persistent()
            .get(&timelock_key(invoice_id))
    }

    /// Check whether a specific action (`"pay"`, `"release"`, or `"refund"`)
    /// is currently blocked by an active time-lock.
    pub fn is_action_timelocked(env: Env, invoice_id: u64, action_name: Symbol) -> bool {
        let key = timelock_key(invoice_id);
        let lock: InvoiceTimeLock = match env.storage().persistent().get(&key) {
            Some(l) => l,
            None => return false,
        };
        let now = env.ledger().timestamp();
        if now >= lock.unlock_at {
            return false; // expired
        }
        let pay_sym = symbol_short!("pay");
        let release_sym = symbol_short!("release");
        let refund_sym = symbol_short!("refund");
        if action_name == pay_sym {
            lock.lock_payments
        } else if action_name == release_sym {
            lock.lock_release
        } else if action_name == refund_sym {
            lock.lock_refund
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{client, make_invoice, setup_initialized};
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn set_and_get_timelock() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &Address::generate(&env), 1000, &token, 9_999);
        let now = env.ledger().timestamp();
        c.set_invoice_timelock(&creator, &id, &(now + 1000), &true, &false, &false);
        let lock = c.get_invoice_timelock(&id).expect("lock");
        assert_eq!(lock.unlock_at, now + 1000);
        assert!(lock.lock_payments);
        assert!(!lock.lock_release);
        assert!(!lock.lock_refund);
    }

    #[test]
    fn remove_timelock() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &Address::generate(&env), 1000, &token, 9_999);
        let now = env.ledger().timestamp();
        c.set_invoice_timelock(&creator, &id, &(now + 1000), &true, &true, &true);
        c.remove_invoice_timelock(&creator, &id);
        assert!(c.get_invoice_timelock(&id).is_none());
    }

    #[test]
    fn remove_nonexistent_timelock_errors() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &Address::generate(&env), 1000, &token, 9_999);
        assert!(c.try_remove_invoice_timelock(&creator, &id).is_err());
    }

    #[test]
    fn only_creator_can_set_timelock() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let other = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &Address::generate(&env), 1000, &token, 9_999);
        let now = env.ledger().timestamp();
        assert!(c
            .try_set_invoice_timelock(&other, &id, &(now + 1000), &true, &false, &false)
            .is_err());
    }

    #[test]
    fn is_action_timelocked_checks_correct_action() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &Address::generate(&env), 1000, &token, 9_999);
        let now = env.ledger().timestamp();
        // Lock payments only.
        c.set_invoice_timelock(&creator, &id, &(now + 10_000), &true, &false, &false);
        assert!(c.is_action_timelocked(&id, &symbol_short!("pay")));
        assert!(!c.is_action_timelocked(&id, &symbol_short!("release")));
        assert!(!c.is_action_timelocked(&id, &symbol_short!("refund")));
    }

    #[test]
    fn expired_lock_is_not_active() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &Address::generate(&env), 1000, &token, 9_999);
        let now = env.ledger().timestamp();
        // Set lock to expire at now+1.
        c.set_invoice_timelock(&creator, &id, &(now + 1), &true, &true, &true);
        // Advance time past the expiry.
        env.ledger().set_timestamp(now + 2);
        // Lock expired — no action should be timelocked.
        assert!(!c.is_action_timelocked(&id, &symbol_short!("pay")));
        assert!(!c.is_action_timelocked(&id, &symbol_short!("release")));
        assert!(!c.is_action_timelocked(&id, &symbol_short!("refund")));
    }

    #[test]
    fn past_unlock_at_errors() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &Address::generate(&env), 1000, &token, 9_999);
        let now = env.ledger().timestamp();
        // unlock_at == now should error (not strictly in future)
        assert!(c
            .try_set_invoice_timelock(&creator, &id, &now, &true, &false, &false)
            .is_err());
    }
}
