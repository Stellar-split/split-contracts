//! Issue #780: per-recipient payment schedule.
//!
//! A creator may attach a `release_at` timestamp to each recipient of a
//! `Pending` invoice. Once the invoice is fully funded, `release_scheduled`
//! pays every recipient whose timestamp has passed (or who has none) and
//! records them as paid, so later calls only pay the remaining recipients.
//!
//! Assumptions: recipients receive their base `amounts[i]`; platform fees,
//! tax and other release-time adjustments of the standard `release` path are
//! not applied. Invoices with a schedule are never released through the
//! standard path (`_release` is a no-op for them).

use crate::error::ContractError;
use crate::{funding_token_for, load_invoice, save_invoice};
use crate::types::InvoiceStatus;
use soroban_sdk::{contracttype, panic_with_error, symbol_short, token, Address, Env, Vec};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScheduleKey {
    /// Vec<Option<u64>> aligned with `invoice.recipients`.
    Schedule(u64),
    /// Vec<bool> aligned with `invoice.recipients`: already paid.
    Paid(u64),
}

pub fn has_schedule(env: &Env, invoice_id: u64) -> bool {
    env.storage().persistent().has(&ScheduleKey::Schedule(invoice_id))
}

pub fn set_schedule(env: &Env, creator: &Address, invoice_id: u64, release_ats: Vec<Option<u64>>) {
    creator.require_auth();
    let invoice = load_invoice(env, invoice_id);
    if invoice.creator != *creator {
        panic_with_error!(env, ContractError::NotAuthorized);
    }
    if invoice.status != InvoiceStatus::Pending
        || invoice.funded != 0
        || release_ats.len() != invoice.recipients.len()
    {
        panic_with_error!(env, ContractError::ScheduleInvalid);
    }
    let mut paid: Vec<bool> = Vec::new(env);
    for _ in 0..release_ats.len() {
        paid.push_back(false);
    }
    env.storage().persistent().set(&ScheduleKey::Schedule(invoice_id), &release_ats);
    env.storage().persistent().set(&ScheduleKey::Paid(invoice_id), &paid);
}

pub fn pending_recipients(env: &Env, invoice_id: u64) -> Vec<Address> {
    let invoice = load_invoice(env, invoice_id);
    let mut out: Vec<Address> = Vec::new(env);
    let paid: Vec<bool> = env
        .storage()
        .persistent()
        .get(&ScheduleKey::Paid(invoice_id))
        .unwrap_or_else(|| Vec::new(env));
    for i in 0..invoice.recipients.len() {
        if !paid.get(i).unwrap_or(false) {
            out.push_back(invoice.recipients.get(i).unwrap());
        }
    }
    out
}

pub fn release_scheduled(env: &Env, invoice_id: u64) {
    let schedule: Vec<Option<u64>> = env
        .storage()
        .persistent()
        .get(&ScheduleKey::Schedule(invoice_id))
        .unwrap_or_else(|| panic_with_error!(env, ContractError::NoScheduleSet));
    let mut paid: Vec<bool> = env.storage().persistent().get(&ScheduleKey::Paid(invoice_id)).unwrap();
    let mut invoice = load_invoice(env, invoice_id);
    if invoice.status != InvoiceStatus::Pending {
        panic_with_error!(env, ContractError::InvalidStatus);
    }
    let total: i128 = invoice.amounts.iter().sum();
    if invoice.funded < total {
        panic_with_error!(env, ContractError::FundingInsufficient);
    }
    let now = env.ledger().timestamp();
    let token_client = token::Client::new(env, &funding_token_for(&invoice));
    let mut all_paid = true;
    for i in 0..invoice.recipients.len() {
        if paid.get(i).unwrap_or(false) {
            continue;
        }
        let due = match schedule.get(i).unwrap_or(None) {
            None => true,
            Some(t) => t <= now,
        };
        if !due {
            all_paid = false;
            continue;
        }
        let recipient = invoice.recipients.get(i).unwrap();
        let amount = invoice.amounts.get(i).unwrap();
        // Mark paid before transferring so a re-entrant call cannot double-pay.
        paid.set(i, true);
        env.storage().persistent().set(&ScheduleKey::Paid(invoice_id), &paid);
        if amount > 0 {
            token_client.transfer(&env.current_contract_address(), &recipient, &amount);
        }
        env.events().publish(
            (symbol_short!("split"), symbol_short!("rcp_paid"), invoice_id),
            (recipient, amount, now),
        );
    }
    if all_paid {
        invoice.status = InvoiceStatus::Released;
        invoice.completion_time = Some(now);
        save_invoice(env, invoice_id, &invoice);
    }
}
