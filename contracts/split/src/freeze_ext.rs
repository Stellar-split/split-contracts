//! Issue #773: emergency global freeze of all mutating operations.
//!
//! Distinct from `pause` (function-level, operator-managed) and from
//! `freeze_for_upgrade` (upgrade checkpoint): this is a single instance-storage
//! flag toggled by the admin. It is enforced inside the shared guard helpers
//! (`require_not_paused` / `check_not_paused`, used by pay, release, contribute
//! and most mutating entry points) plus explicit checks at the top of `refund`
//! and `cancel_invoice`. Read-only entry points never call those guards and
//! remain available. `unfreeze_contract` and `is_frozen` are never blocked.
//!
//! Assumption: admin authorisation follows the same rule as `pause` (the stored
//! admin, or an Operator+ role when no single admin key is set).

use soroban_sdk::{contractimpl, contracttype, panic_with_error, symbol_short, Address, Env, Symbol};

use crate::error::ContractError;
use crate::{admin_key, require_admin_role_unguarded, AdminRole, SplitContract};

#[contracttype]
#[derive(Clone)]
pub enum FreezeKey {
    /// Instance flag: bool
    Frozen,
}

pub(crate) fn is_frozen(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&FreezeKey::Frozen)
        .unwrap_or(false)
}

/// Panics with `ContractError::ContractFrozen` when the contract is frozen.
pub(crate) fn require_not_frozen_global(env: &Env) {
    if is_frozen(env) {
        panic_with_error!(env, ContractError::ContractFrozen);
    }
}

pub(crate) fn require_admin(env: &Env, admin: &Address) {
    admin.require_auth();
    if let Some(stored) = env.storage().instance().get::<_, Address>(&admin_key()) {
        assert!(*admin == stored, "NotAuthorized");
    } else {
        require_admin_role_unguarded(env, admin, AdminRole::Operator);
    }
}

#[contractimpl]
impl SplitContract {
    /// Issue #773: freeze every mutating operation. Admin only.
    pub fn freeze_contract(env: Env, admin: Address) {
        require_admin(&env, &admin);
        env.storage().instance().set(&FreezeKey::Frozen, &true);
        env.events().publish(
            (symbol_short!("split"), Symbol::new(&env, "ContractFrozen")),
            (admin, env.ledger().timestamp()),
        );
    }

    /// Issue #773: lift the freeze. Admin only.
    pub fn unfreeze_contract(env: Env, admin: Address) {
        require_admin(&env, &admin);
        env.storage().instance().set(&FreezeKey::Frozen, &false);
        env.events().publish(
            (symbol_short!("split"), Symbol::new(&env, "ContractUnfrozen")),
            (admin, env.ledger().timestamp()),
        );
    }

    /// Issue #773: whether the emergency freeze is active (never blocked).
    pub fn is_frozen(env: Env) -> bool {
        is_frozen(&env)
    }
}

#[cfg(test)]
mod tests {
    use crate::ext_test_util::*;
    use soroban_sdk::{testutils::Address as _, Address};

    #[test]
    fn freeze_blocks_pay_release_and_cancel_then_unfreeze_restores() {
        let f = fixture();
        let (id, creator, _r) = new_invoice(&f, 100);
        let payer = Address::generate(&f.env);
        mint(&f.env, &f.token, &payer, 100);

        assert!(!f.c.is_frozen());
        f.c.freeze_contract(&f.admin);
        assert!(f.c.is_frozen());

        assert!(f.c.try_pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None).is_err());
        assert!(f.c.try_release(&id).is_err());
        assert!(f.c.try_refund(&id).is_err());
        assert!(f.c.try_cancel_invoice(&creator, &id).is_err());

        // reads still work while frozen
        assert_eq!(f.c.get_invoice(&id).funded, 0);

        f.c.unfreeze_contract(&f.admin);
        assert!(!f.c.is_frozen());
        pay(&f, &payer, id, 100);
        assert_eq!(f.c.get_invoice(&id).funded, 100);
    }

    #[test]
    fn non_admin_cannot_freeze() {
        let f = fixture();
        let other = Address::generate(&f.env);
        assert!(f.c.try_freeze_contract(&other).is_err());
        assert!(!f.c.is_frozen());
    }
}
