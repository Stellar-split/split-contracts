//! Issue #772: optional post-funding escrow hold period (in seconds).
//!
//! Complements the pre-existing ledger-based `escrow_hold_period` field: this
//! hold is timestamp based, opt-in per invoice via [`SplitContract::set_escrow_hold_secs`]
//! (the creator may set it while the invoice is unfunded), and is recorded in
//! its own key enum so no shared `StorageKey`/`InvoiceKey` variant is consumed.
//!
//! When an invoice first becomes fully funded, `fully_funded_at` is stored, the
//! `EscrowHoldStarted` event is emitted and automatic release is suppressed;
//! `release` then panics with `EscrowHoldActive` until
//! `now >= fully_funded_at + hold_secs`.

use soroban_sdk::{contractimpl, contracttype, symbol_short, Address, Env, Symbol};

use crate::{load_invoice, InvoiceStatus, SplitContract};

#[contracttype]
#[derive(Clone)]
pub enum HoldKey {
    /// (invoice_id) -> u64 hold length in seconds
    Secs(u64),
    /// (invoice_id) -> u64 timestamp at which the invoice first became fully funded
    FundedAt(u64),
}

pub(crate) fn hold_secs(env: &Env, invoice_id: u64) -> Option<u64> {
    env.storage().persistent().get(&HoldKey::Secs(invoice_id))
}

pub(crate) fn fully_funded_at(env: &Env, invoice_id: u64) -> Option<u64> {
    env.storage().persistent().get(&HoldKey::FundedAt(invoice_id))
}

/// Called when an invoice reaches full funding. Returns `true` when a hold is
/// active, in which case the caller must not auto-release.
pub(crate) fn on_fully_funded(env: &Env, invoice_id: u64) -> bool {
    let Some(secs) = hold_secs(env, invoice_id) else {
        return false;
    };
    if fully_funded_at(env, invoice_id).is_none() {
        let now = env.ledger().timestamp();
        env.storage()
            .persistent()
            .set(&HoldKey::FundedAt(invoice_id), &now);
        env.events().publish(
            (
                symbol_short!("split"),
                Symbol::new(env, "EscrowHoldStarted"),
                invoice_id,
            ),
            now.saturating_add(secs),
        );
    }
    true
}

/// Panics with `EscrowHoldActive` while the hold has not elapsed.
pub(crate) fn enforce_release(env: &Env, invoice_id: u64) {
    if let (Some(secs), Some(at)) = (hold_secs(env, invoice_id), fully_funded_at(env, invoice_id)) {
        let allowed = at.saturating_add(secs);
        if env.ledger().timestamp() < allowed {
            panic!("EscrowHoldActive: release_allowed_at={}", allowed);
        }
    }
}

#[contractimpl]
impl SplitContract {
    /// Issue #772: set the timestamp-based escrow hold (seconds) for an
    /// unfunded pending invoice. Creator only.
    pub fn set_escrow_hold_secs(env: Env, creator: Address, invoice_id: u64, secs: u64) {
        creator.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        assert!(invoice.creator == creator, "NotAuthorized");
        assert!(
            invoice.status == InvoiceStatus::Pending && invoice.funded == 0,
            "hold can only be set on an unfunded pending invoice"
        );
        env.storage()
            .persistent()
            .set(&HoldKey::Secs(invoice_id), &secs);
    }

    /// Issue #772: configured hold in seconds, if any.
    pub fn get_escrow_hold_secs(env: Env, invoice_id: u64) -> Option<u64> {
        hold_secs(&env, invoice_id)
    }

    /// Issue #772: timestamp at which the invoice first became fully funded.
    pub fn get_fully_funded_at(env: Env, invoice_id: u64) -> Option<u64> {
        fully_funded_at(&env, invoice_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ext_test_util::*;
    use soroban_sdk::testutils::{Address as _, Ledger};

    fn funded_with_hold(hold: Option<u64>) -> (crate::ext_test_util::Fixture<'static>, u64) {
        let f = fixture();
        f.env.ledger().set_timestamp(1_000);
        let (id, creator, _r) = new_invoice(&f, 100);
        if let Some(h) = hold {
            f.c.set_escrow_hold_secs(&creator, &id, &h);
        }
        let payer = Address::generate(&f.env);
        mint(&f.env, &f.token, &payer, 100);
        pay(&f, &payer, id, 100);
        (f, id)
    }

    #[test]
    fn no_hold_releases_immediately() {
        let (f, id) = funded_with_hold(None);
        assert_eq!(f.c.get_invoice(&id).status, InvoiceStatus::Released);
    }

    #[test]
    fn hold_records_timestamp_and_blocks_auto_release() {
        let (f, id) = funded_with_hold(Some(500));
        assert_eq!(f.c.get_fully_funded_at(&id), Some(1_000));
        assert_eq!(f.c.get_invoice(&id).status, InvoiceStatus::Pending);
    }

    #[test]
    #[should_panic(expected = "EscrowHoldActive: release_allowed_at=1500")]
    fn release_during_hold_panics_with_timestamp() {
        let (f, id) = funded_with_hold(Some(500));
        f.env.ledger().set_timestamp(1_499);
        f.c.release(&id);
    }

    #[test]
    fn release_after_hold_succeeds() {
        let (f, id) = funded_with_hold(Some(500));
        f.env.ledger().set_timestamp(1_500);
        f.c.release(&id);
        assert_eq!(f.c.get_invoice(&id).status, InvoiceStatus::Released);
    }
}
