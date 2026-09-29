//! Issue #782: per-ledger streaming payments.
//!
//! A payer commits to `amount_per_ledger` toward an invoice. Accrued debt is
//! `(current_ledger - last_settled_ledger) * amount_per_ledger`; `settle`
//! charges it and advances `last_settled_ledger` (so repeated settles never
//! double-charge), `cancel` settles and deactivates the stream.
//!
//! Assumption: settled amounts are credited to `invoice.funded` (and held by
//! the contract) but do not themselves trigger auto-release.

use crate::error::ContractError;
use crate::types::InvoiceStatus;
use crate::{funding_token_for, load_invoice, save_invoice};
use soroban_sdk::{contracttype, panic_with_error, symbol_short, token, Address, Env};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Stream {
    pub id: u64,
    pub invoice_id: u64,
    pub payer: Address,
    pub amount_per_ledger: i128,
    pub start_ledger: u32,
    pub last_settled_ledger: u32,
    pub total_settled: i128,
    pub active: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamKey {
    /// Persistent: `Stream` by id.
    Stream(u64),
    /// Instance: last issued stream id.
    Counter,
}

pub fn get_stream(env: &Env, stream_id: u64) -> Stream {
    env.storage()
        .persistent()
        .get(&StreamKey::Stream(stream_id))
        .unwrap_or_else(|| panic_with_error!(env, ContractError::StreamNotFound))
}

pub fn start(env: &Env, invoice_id: u64, payer: &Address, amount_per_ledger: i128) -> u64 {
    payer.require_auth();
    if amount_per_ledger <= 0 {
        panic_with_error!(env, ContractError::InvalidAmount);
    }
    let invoice = load_invoice(env, invoice_id);
    if invoice.status != InvoiceStatus::Pending {
        panic_with_error!(env, ContractError::InvalidStatus);
    }
    let id: u64 = env.storage().instance().get(&StreamKey::Counter).unwrap_or(0u64) + 1;
    env.storage().instance().set(&StreamKey::Counter, &id);
    let seq = env.ledger().sequence();
    let stream = Stream {
        id,
        invoice_id,
        payer: payer.clone(),
        amount_per_ledger,
        start_ledger: seq,
        last_settled_ledger: seq,
        total_settled: 0,
        active: true,
    };
    env.storage().persistent().set(&StreamKey::Stream(id), &stream);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("strm_st"), invoice_id),
        (id, payer.clone(), amount_per_ledger),
    );
    id
}

/// Charge the accrued amount and credit the invoice; returns the amount.
fn charge(env: &Env, stream: &mut Stream) -> i128 {
    let seq = env.ledger().sequence();
    let elapsed = seq.saturating_sub(stream.last_settled_ledger) as i128;
    let amount = elapsed
        .checked_mul(stream.amount_per_ledger)
        .unwrap_or_else(|| panic_with_error!(env, ContractError::InvalidAmount));
    stream.last_settled_ledger = seq;
    stream.total_settled += amount;
    // Persist stream state before the external token call.
    env.storage().persistent().set(&StreamKey::Stream(stream.id), &*stream);
    if amount > 0 {
        let mut invoice = load_invoice(env, stream.invoice_id);
        if invoice.status != InvoiceStatus::Pending {
            panic_with_error!(env, ContractError::InvalidStatus);
        }
        let tk = token::Client::new(env, &funding_token_for(&invoice));
        tk.transfer(&stream.payer, &env.current_contract_address(), &amount);
        invoice.funded += amount;
        save_invoice(env, stream.invoice_id, &invoice);
    }
    amount
}

pub fn settle(env: &Env, stream_id: u64, payer: &Address) {
    payer.require_auth();
    let mut stream = get_stream(env, stream_id);
    if stream.payer != *payer {
        panic_with_error!(env, ContractError::NotAuthorized);
    }
    if !stream.active {
        panic_with_error!(env, ContractError::StreamNotActive);
    }
    let amount = charge(env, &mut stream);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("strm_set"), stream.invoice_id),
        (stream_id, amount),
    );
}

pub fn cancel(env: &Env, stream_id: u64, payer: &Address) {
    payer.require_auth();
    let mut stream = get_stream(env, stream_id);
    if stream.payer != *payer {
        panic_with_error!(env, ContractError::NotAuthorized);
    }
    if !stream.active {
        panic_with_error!(env, ContractError::StreamNotActive);
    }
    stream.active = false;
    let amount = charge(env, &mut stream);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("strm_can"), stream.invoice_id),
        (stream_id, amount),
    );
}
