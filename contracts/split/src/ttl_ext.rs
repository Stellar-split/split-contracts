//! Issue #770: storage TTL extension automation for long-lived invoices.
//!
//! Anyone may call `bump_invoice_ttl` for a live (`Pending`) invoice; every
//! storage entry belonging to the invoice is extended by
//! [`crate::TTL_EXTENSION_LEDGERS`] (clamped to the network maximum).
//!
//! Soroban contracts cannot read an entry's TTL on-chain, so the ledger up to
//! which the invoice was last extended is recorded and `get_ttl` reports the
//! remaining ledgers from it.

use crate::error::ContractError;
use crate::storage_keys::InvoiceKey;
use crate::types::InvoiceStatus;
use crate::{SplitContract, SplitContractClient, TTL_EXTENSION_LEDGERS};
use soroban_sdk::{contractimpl, contracttype, panic_with_error, symbol_short, Env, IntoVal, Val};

/// Persistent storage keys owned by this module (issue #770).
#[contracttype]
#[derive(Clone)]
pub enum TtlKey {
    /// `u32` ledger sequence up to which the invoice was last extended.
    ExtendedUntil(u64),
}

fn extend_if_present<K: IntoVal<Env, Val>>(env: &Env, key: &K, to: u32) {
    let s = env.storage().persistent();
    if s.has(key) {
        s.extend_ttl(key, to, to);
    }
}

#[contractimpl]
impl SplitContract {
    /// Issue #770: extend the TTL of every storage key of a live invoice by
    /// `TTL_EXTENSION_LEDGERS`. Callable by anyone; panics with
    /// `InvoiceTerminated` for terminal invoices.
    pub fn bump_invoice_ttl(env: Env, invoice_id: u64) {
        let invoice = crate::load_invoice(&env, invoice_id);
        if invoice.status != InvoiceStatus::Pending {
            panic_with_error!(&env, ContractError::InvoiceTerminated);
        }
        let to = TTL_EXTENSION_LEDGERS.min(env.storage().max_ttl());
        let id = invoice_id;
        extend_if_present(&env, &crate::invoice_key(id), to);
        extend_if_present(&env, &crate::invoice_ext_key(id), to);
        extend_if_present(&env, &crate::invoice_ext2_key(id), to);
        extend_if_present(&env, &crate::invoice_compact_key(id), to);
        extend_if_present(&env, &InvoiceKey::RecipientsList(id), to);
        extend_if_present(&env, &InvoiceKey::AmountsList(id), to);
        extend_if_present(&env, &InvoiceKey::PaidFlags(id), to);
        extend_if_present(&env, &InvoiceKey::AuditLog(id), to);
        env.storage().instance().extend_ttl(to, to);
        env.storage()
            .persistent()
            .set(&TtlKey::ExtendedUntil(id), &(env.ledger().sequence() + to));
        extend_if_present(&env, &TtlKey::ExtendedUntil(id), to);
        env.events().publish(
            (symbol_short!("split"), symbol_short!("ttl_bump"), invoice_id),
            to,
        );
    }

    /// Remaining TTL in ledgers as of the last `bump_invoice_ttl` (0 if never bumped).
    pub fn get_ttl(env: Env, invoice_id: u64) -> u32 {
        let until: u32 = env
            .storage()
            .persistent()
            .get(&TtlKey::ExtendedUntil(invoice_id))
            .unwrap_or(0);
        until.saturating_sub(env.ledger().sequence())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{client, make_invoice, setup_initialized};
    use soroban_sdk::testutils::{Address as _, Ledger};
    use soroban_sdk::Address;

    #[test]
    fn bump_active_invoice_increases_ttl() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let id = make_invoice(&env, &c, &Address::generate(&env), &Address::generate(&env), 100, &token, 9_999);
        assert_eq!(c.get_ttl(&id), 0);
        c.bump_invoice_ttl(&id);
        let t = c.get_ttl(&id);
        assert!(t > 0 && t <= TTL_EXTENSION_LEDGERS);
        env.ledger().with_mut(|l| l.sequence_number += 100);
        assert_eq!(c.get_ttl(&id), t - 100);
    }

    #[test]
    fn bump_terminal_invoice_rejected() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &Address::generate(&env), 100, &token, 9_999);
        c.cancel_invoice(&creator, &id);
        assert!(c.try_bump_invoice_ttl(&id).is_err());
    }
}
