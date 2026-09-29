//! Payment escrow with milestone triggers (issue #854).
//!
//! A payer pre-funds an escrow with a list of milestones. Each milestone pays
//! a fixed amount to the payee once its trigger condition holds:
//!
//! - [`MilestoneTrigger::PayerApproval`] — the payer signs the release.
//! - [`MilestoneTrigger::Timestamp`]     — anyone may release once the ledger
//!   time is reached; `trigger_due_milestones` releases every due timed
//!   milestone in one call so keepers can automate payouts.
//! - [`MilestoneTrigger::Arbiter`]       — a named arbiter / oracle signs.
//!
//! The escrow completes when every milestone is paid. Payer and payee can
//! jointly cancel (refunding the unreleased remainder to the payer), and the
//! contract admin can resolve a dispute by sending the remainder to either
//! side.

use soroban_sdk::{symbol_short, token, Address, Env, Symbol, Vec};

use crate::errors::Error;
use crate::types::{
    Milestone, MilestoneEscrow, MilestoneEscrowStatus, MilestoneInput, MilestoneTrigger,
};
use crate::{require_admin, require_initialized};

/// Maximum number of milestones in a single escrow.
pub(crate) const MAX_MILESTONES: u32 = 20;

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Instance storage: monotonic milestone-escrow counter.
fn counter_key() -> Symbol {
    symbol_short!("ms_count")
}

/// Persistent storage: milestone escrow keyed by ID.
fn escrow_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("ms_escrow"), id)
}

pub(crate) fn get_escrow(env: &Env, id: u64) -> Result<MilestoneEscrow, Error> {
    env.storage()
        .persistent()
        .get(&escrow_key(id))
        .ok_or(Error::MilestoneEscrowNotFound)
}

fn save_escrow(env: &Env, id: u64, escrow: &MilestoneEscrow) {
    env.storage().persistent().set(&escrow_key(id), escrow);
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Topics: `(milestone, created, id)` — Data: `(payer, payee, total_amount, milestone_count)`
fn emit_created(env: &Env, id: u64, escrow: &MilestoneEscrow) {
    env.events().publish(
        (symbol_short!("milestone"), symbol_short!("created"), id),
        (
            escrow.payer.clone(),
            escrow.payee.clone(),
            escrow.total_amount,
            escrow.milestones.len(),
        ),
    );
}

/// Topics: `(milestone, released, id)` — Data: `(index, amount, released_total)`
fn emit_released(env: &Env, id: u64, index: u32, amount: i128, released_total: i128) {
    env.events().publish(
        (symbol_short!("milestone"), symbol_short!("released"), id),
        (index, amount, released_total),
    );
}

/// Topics: `(milestone, completed, id)` — Data: `total_amount`
fn emit_completed(env: &Env, id: u64, total: i128) {
    env.events().publish(
        (symbol_short!("milestone"), symbol_short!("completed"), id),
        total,
    );
}

/// Topics: `(milestone, cancelled, id)` — Data: `refunded_to_payer`
fn emit_cancelled(env: &Env, id: u64, refunded: i128) {
    env.events().publish(
        (symbol_short!("milestone"), symbol_short!("cancelled"), id),
        refunded,
    );
}

/// Topics: `(milestone, resolved, id)` — Data: `(paid_to_payee, amount)`
fn emit_resolved(env: &Env, id: u64, pay_payee: bool, amount: i128) {
    env.events().publish(
        (symbol_short!("milestone"), symbol_short!("resolved"), id),
        (pay_payee, amount),
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load_active(env: &Env, id: u64) -> Result<MilestoneEscrow, Error> {
    let escrow = get_escrow(env, id)?;
    if escrow.status != MilestoneEscrowStatus::Active {
        return Err(Error::EscrowClosed);
    }
    Ok(escrow)
}

/// Mark milestone `index` released, pay the payee and emit events.
/// The caller is responsible for persisting `escrow` afterwards.
fn release_one(env: &Env, id: u64, escrow: &mut MilestoneEscrow, index: u32, mut m: Milestone) {
    let amount = m.amount;
    m.released = true;
    escrow.milestones.set(index, m);
    escrow.released_amount = escrow
        .released_amount
        .checked_add(amount)
        .expect("released_amount overflow");

    token::Client::new(env, &escrow.token).transfer(
        &env.current_contract_address(),
        &escrow.payee,
        &amount,
    );
    emit_released(env, id, index, amount, escrow.released_amount);

    if escrow.released_amount >= escrow.total_amount {
        escrow.status = MilestoneEscrowStatus::Completed;
        emit_completed(env, id, escrow.total_amount);
    }
}

// ---------------------------------------------------------------------------
// Entry points (wrapped by the contract impl in lib.rs)
// ---------------------------------------------------------------------------

pub(crate) fn create(
    env: &Env,
    payer: Address,
    payee: Address,
    token: Address,
    inputs: Vec<MilestoneInput>,
) -> Result<u64, Error> {
    require_initialized(env)?;
    if payer == payee {
        return Err(Error::SelfDealing);
    }
    let n = inputs.len();
    if n == 0 || n > MAX_MILESTONES {
        return Err(Error::InvalidMilestones);
    }
    payer.require_auth();

    let mut total: i128 = 0;
    let mut milestones: Vec<Milestone> = Vec::new(env);
    for input in inputs.iter() {
        if input.amount <= 0 {
            return Err(Error::InvalidMilestones);
        }
        total = total
            .checked_add(input.amount)
            .ok_or(Error::InvalidMilestones)?;
        milestones.push_back(Milestone {
            amount: input.amount,
            trigger: input.trigger,
            released: false,
        });
    }

    let id: u64 = env
        .storage()
        .instance()
        .get(&counter_key())
        .unwrap_or(0u64);
    let next_id = id.checked_add(1).expect("milestone counter overflow");
    env.storage().instance().set(&counter_key(), &next_id);

    token::Client::new(env, &token).transfer(&payer, &env.current_contract_address(), &total);

    let escrow = MilestoneEscrow {
        payer,
        payee,
        token,
        total_amount: total,
        released_amount: 0,
        milestones,
        status: MilestoneEscrowStatus::Active,
        created_at: env.ledger().timestamp(),
    };
    save_escrow(env, id, &escrow);
    emit_created(env, id, &escrow);
    Ok(id)
}

pub(crate) fn trigger(env: &Env, caller: Address, id: u64, index: u32) -> Result<i128, Error> {
    caller.require_auth();
    let mut escrow = load_active(env, id)?;
    let m = escrow
        .milestones
        .get(index)
        .ok_or(Error::MilestoneIndexOutOfRange)?;
    if m.released {
        return Err(Error::MilestoneAlreadyReleased);
    }
    let satisfied = match &m.trigger {
        MilestoneTrigger::PayerApproval => caller == escrow.payer,
        MilestoneTrigger::Timestamp(at) => env.ledger().timestamp() >= *at,
        MilestoneTrigger::Arbiter(arbiter) => caller == *arbiter,
    };
    if !satisfied {
        return Err(Error::TriggerNotSatisfied);
    }
    let amount = m.amount;
    release_one(env, id, &mut escrow, index, m);
    save_escrow(env, id, &escrow);
    Ok(amount)
}

pub(crate) fn trigger_due(env: &Env, id: u64) -> Result<u32, Error> {
    let mut escrow = load_active(env, id)?;
    let now = env.ledger().timestamp();
    let mut count: u32 = 0;
    for index in 0..escrow.milestones.len() {
        let m = escrow.milestones.get_unchecked(index);
        if m.released {
            continue;
        }
        if let MilestoneTrigger::Timestamp(at) = &m.trigger {
            if now >= *at {
                release_one(env, id, &mut escrow, index, m);
                count += 1;
            }
        }
    }
    save_escrow(env, id, &escrow);
    Ok(count)
}

pub(crate) fn cancel(env: &Env, id: u64) -> Result<i128, Error> {
    let mut escrow = load_active(env, id)?;
    escrow.payer.require_auth();
    escrow.payee.require_auth();

    let refund = escrow.total_amount - escrow.released_amount;
    escrow.status = MilestoneEscrowStatus::Cancelled;
    save_escrow(env, id, &escrow);
    if refund > 0 {
        token::Client::new(env, &escrow.token).transfer(
            &env.current_contract_address(),
            &escrow.payer,
            &refund,
        );
    }
    emit_cancelled(env, id, refund);
    Ok(refund)
}

pub(crate) fn resolve(env: &Env, id: u64, pay_payee: bool) -> Result<i128, Error> {
    require_initialized(env)?;
    require_admin(env);
    let mut escrow = load_active(env, id)?;

    let remaining = escrow.total_amount - escrow.released_amount;
    let to = if pay_payee {
        escrow.released_amount = escrow.total_amount;
        escrow.status = MilestoneEscrowStatus::Completed;
        escrow.payee.clone()
    } else {
        escrow.status = MilestoneEscrowStatus::Cancelled;
        escrow.payer.clone()
    };
    save_escrow(env, id, &escrow);
    if remaining > 0 {
        token::Client::new(env, &escrow.token).transfer(
            &env.current_contract_address(),
            &to,
            &remaining,
        );
    }
    emit_resolved(env, id, pay_payee, remaining);
    Ok(remaining)
}
