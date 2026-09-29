//! Issue #783: pluggable per-invoice payment validator.
//!
//! A creator may attach a validator contract implementing
//! `validate_payment(invoice_id: u64, payer: Address, amount: i128) -> bool`
//! (see [`PaymentValidator`]). It is invoked before every payment: `false`
//! rejects with `PaymentRejectedByValidator`; a trapping/malformed call is
//! surfaced as `ValidatorCallFailed`. A temporary-storage lock prevents the
//! validator from re-entering the payment path during its own call.

use crate::error::ContractError;
use crate::{load_invoice, save_invoice};
use soroban_sdk::{contractclient, contracttype, panic_with_error, Address, Env, IntoVal, Symbol};

/// Interface a validator contract must implement.
#[contractclient(name = "PaymentValidatorClient")]
pub trait PaymentValidator {
    fn validate_payment(env: Env, invoice_id: u64, payer: Address, amount: i128) -> bool;
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidatorKey {
    /// Persistent: validator `Address` for an invoice.
    Validator(u64),
    /// Temporary: re-entrancy lock held during the external call.
    Busy,
}

pub fn set_validator(env: &Env, creator: &Address, invoice_id: u64, validator: Option<Address>) {
    creator.require_auth();
    let invoice = load_invoice(env, invoice_id);
    if invoice.creator != *creator {
        panic_with_error!(env, ContractError::NotAuthorized);
    }
    match validator {
        Some(v) => env.storage().persistent().set(&ValidatorKey::Validator(invoice_id), &v),
        None => env.storage().persistent().remove(&ValidatorKey::Validator(invoice_id)),
    }
    // Keep the invoice record touched so TTL is bumped alongside the setting.
    save_invoice(env, invoice_id, &invoice);
}

pub fn get_validator(env: &Env, invoice_id: u64) -> Option<Address> {
    env.storage().persistent().get(&ValidatorKey::Validator(invoice_id))
}

pub fn validate(env: &Env, invoice_id: u64, payer: &Address, amount: i128) {
    let Some(validator) = get_validator(env, invoice_id) else {
        return;
    };
    if env.storage().temporary().has(&ValidatorKey::Busy) {
        panic_with_error!(env, ContractError::ValidatorCallFailed);
    }
    env.storage().temporary().set(&ValidatorKey::Busy, &true);
    let res = env.try_invoke_contract::<bool, soroban_sdk::Error>(
        &validator,
        &Symbol::new(env, "validate_payment"),
        (invoice_id, payer.clone(), amount).into_val(env),
    );
    env.storage().temporary().remove(&ValidatorKey::Busy);
    match res {
        Ok(Ok(true)) => {}
        Ok(Ok(false)) => panic_with_error!(env, ContractError::PaymentRejectedByValidator),
        _ => panic_with_error!(env, ContractError::ValidatorCallFailed),
    }
}
