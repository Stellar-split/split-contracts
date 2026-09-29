//! Issue #781: optional KYC/AML registry gate for invoice payments.
//!
//! Admin sets a registry contract exposing `is_approved(payer: Address) -> bool`.
//! Invoices created with `require_kyc = true` then require the payer to be
//! approved by that registry at payment time.

use crate::error::ContractError;
use soroban_sdk::{contracttype, panic_with_error, Address, Env, IntoVal, Symbol};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ComplianceKey {
    /// Instance storage: `Address` of the KYC registry contract.
    KycRegistry,
}

pub fn set_registry(env: &Env, registry: &Address) {
    env.storage().instance().set(&ComplianceKey::KycRegistry, registry);
}

pub fn get_registry(env: &Env) -> Option<Address> {
    env.storage().instance().get(&ComplianceKey::KycRegistry)
}

/// Returns `true` when a registry is configured and approved the payer,
/// `false` when no registry is configured (caller falls back to legacy
/// behaviour). Panics with `KycNotApproved` when the registry rejects.
pub fn check_kyc(env: &Env, payer: &Address) -> bool {
    match get_registry(env) {
        None => false,
        Some(registry) => {
            let ok: bool = env.invoke_contract(
                &registry,
                &Symbol::new(env, "is_approved"),
                (payer.clone(),).into_val(env),
            );
            if !ok {
                panic_with_error!(env, ContractError::KycNotApproved);
            }
            true
        }
    }
}
