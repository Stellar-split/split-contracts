//! Issue #769: creator delegate system.
//!
//! A creator may appoint up to 3 delegates per invoice. Delegates may pause,
//! resume, add notes and extend the deadline; release, cancel and recipient
//! changes stay creator-only (delegates are rejected with
//! `DelegateNotAuthorised` on release/cancel).
//!
//! Note: the legacy single `delegate_invoice` (issue #43) is independent and
//! unchanged.

use crate::error::ContractError;
use crate::{SplitContract, SplitContractClient};
use soroban_sdk::{
    contractimpl, contracttype, panic_with_error, symbol_short, Address, Env, String, Vec,
};

/// Maximum number of delegates per invoice.
pub const MAX_DELEGATES: u32 = 3;

/// Persistent storage keys owned by this module (issue #769).
#[contracttype]
#[derive(Clone)]
pub enum DelegateKey {
    /// `Vec<Address>` of delegates for an invoice.
    Delegates(u64),
}

fn load(env: &Env, invoice_id: u64) -> Vec<Address> {
    env.storage()
        .persistent()
        .get(&DelegateKey::Delegates(invoice_id))
        .unwrap_or_else(|| Vec::new(env))
}

/// True when `who` is a delegate of the invoice.
pub(crate) fn is_delegate_of(env: &Env, invoice_id: u64, who: &Address) -> bool {
    load(env, invoice_id).contains(who)
}

/// Panic with `DelegateNotAuthorised` when `caller` is a delegate and not the
/// creator. Used by creator-only operations.
pub(crate) fn reject_delegate(env: &Env, invoice_id: u64, caller: &Address) {
    if is_delegate_of(env, invoice_id, caller) {
        let creator = crate::load_invoice(env, invoice_id).creator;
        if &creator != caller {
            panic_with_error!(env, ContractError::DelegateNotAuthorised);
        }
    }
}

fn require_creator(env: &Env, invoice_id: u64, creator: &Address) {
    creator.require_auth();
    let invoice = crate::load_invoice(env, invoice_id);
    assert!(&invoice.creator == creator, "only creator can manage delegates");
}

#[contractimpl]
impl SplitContract {
    /// Appoint a delegate (creator only, max 3 per invoice).
    pub fn add_delegate(env: Env, invoice_id: u64, creator: Address, delegate: Address) {
        require_creator(&env, invoice_id, &creator);
        let mut list = load(&env, invoice_id);
        if list.contains(&delegate) {
            return;
        }
        if list.len() >= MAX_DELEGATES {
            panic_with_error!(&env, ContractError::DelegateLimitReached);
        }
        list.push_back(delegate.clone());
        env.storage()
            .persistent()
            .set(&DelegateKey::Delegates(invoice_id), &list);
        env.events().publish(
            (symbol_short!("split"), symbol_short!("dlg_add"), invoice_id),
            delegate,
        );
    }

    /// Remove a delegate (creator only).
    pub fn remove_delegate(env: Env, invoice_id: u64, creator: Address, delegate: Address) {
        require_creator(&env, invoice_id, &creator);
        let mut list = load(&env, invoice_id);
        if let Some(i) = list.iter().position(|d| d == delegate) {
            list.remove(i as u32);
            env.storage()
                .persistent()
                .set(&DelegateKey::Delegates(invoice_id), &list);
            env.events().publish(
                (symbol_short!("split"), symbol_short!("dlg_rem"), invoice_id),
                delegate,
            );
        }
    }

    /// Whether `address` is a delegate of the invoice.
    pub fn is_delegate(env: Env, invoice_id: u64, address: Address) -> bool {
        is_delegate_of(&env, invoice_id, &address)
    }

    /// Set the invoice note. Callable by the creator, a co-creator or a delegate.
    pub fn add_note(env: Env, invoice_id: u64, caller: Address, note: String) {
        caller.require_auth();
        let invoice = crate::load_invoice(&env, invoice_id);
        assert!(
            invoice.creator == caller
                || invoice.co_creators.iter().any(|c| c == caller)
                || is_delegate_of(&env, invoice_id, &caller),
            "not authorised to add note"
        );
        let priority = crate::migrations::get_invoice_meta(&env, invoice_id)
            .map(|m| m.priority)
            .unwrap_or(0);
        env.storage().persistent().set(
            &crate::migrations::invoice_meta_key_v3(invoice_id),
            &crate::migrations::InvoiceMeta { note, priority },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{client, make_invoice, setup_initialized};
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn delegate_can_pause_resume_and_note_but_not_release_or_cancel() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &Address::generate(&env), 100, &token, 9_999);
        let d = Address::generate(&env);
        c.add_delegate(&id, &creator, &d);
        assert!(c.is_delegate(&id, &d));
        c.pause_invoice(&d, &id, &String::from_str(&env, "r"), &None);
        assert!(c.get_invoice(&id).frozen);
        c.resume_invoice(&d, &id);
        assert!(!c.get_invoice(&id).frozen);
        c.add_note(&id, &d, &String::from_str(&env, "hi"));
        assert!(c.try_cancel_invoice(&d, &id).is_err());
        assert!(c.try_release_invoice(&d, &id, &None).is_err());
    }

    #[test]
    fn max_three_delegates() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &Address::generate(&env), 100, &token, 9_999);
        for _ in 0..3 {
            c.add_delegate(&id, &creator, &Address::generate(&env));
        }
        assert!(c
            .try_add_delegate(&id, &creator, &Address::generate(&env))
            .is_err());
    }

    #[test]
    fn removed_delegate_loses_access() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &Address::generate(&env), 100, &token, 9_999);
        let d = Address::generate(&env);
        c.add_delegate(&id, &creator, &d);
        c.remove_delegate(&id, &creator, &d);
        assert!(!c.is_delegate(&id, &d));
        assert!(c
            .try_pause_invoice(&d, &id, &String::from_str(&env, "r"), &None)
            .is_err());
    }
}
