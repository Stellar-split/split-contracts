use soroban_sdk::{Env, Symbol};

use crate::error::ContractError;

/// Protocol-wide aggregate counters.
///
/// The counters are stored in instance storage, so they do not require a
/// persistent-storage TTL and survive independently of individual invoices.
pub type Stats = (u64, i128, u64);

const TOTAL_INVOICES: &str = "stats_total_invoices";
const TOTAL_VOLUME: &str = "stats_total_volume";
const TOTAL_RECIPIENTS_PAID: &str = "stats_total_recipients_paid";
const STATS_UPDATED: &str = "StatsUpdated";

pub type Stats = (u64, i128, u64);

fn invoices_key(env: &Env) -> Symbol {
    Symbol::new(env, TOTAL_INVOICES)
}

fn volume_key(env: &Env) -> Symbol {
    Symbol::new(env, TOTAL_VOLUME)
}

fn recipients_paid_key(env: &Env) -> Symbol {
    Symbol::new(env, TOTAL_RECIPIENTS_PAID)
}

pub fn get_stats(env: &Env) -> Stats {
    let invoices = env
        .storage()
        .instance()
        .get::<Symbol, u64>(&invoices_key(env))
        .unwrap_or(0);
    let volume = env
        .storage()
        .instance()
        .get::<Symbol, i128>(&volume_key(env))
        .unwrap_or(0);
    let recipients_paid = env
        .storage()
        .instance()
        .get::<Symbol, u64>(&recipients_paid_key(env))
        .unwrap_or(0);

    (invoices, volume, recipients_paid)
}

fn publish_updated(env: &Env, stats: Stats) {
    env.events().publish(
        (Symbol::new(env, STATS_UPDATED),),
        (
            stats.0,
            stats.1,
            stats.2,
            env.ledger().sequence(),
        ),
    );
}

pub fn record_invoice_created(env: &Env) -> Result<(), ContractError> {
    let (invoices, volume, recipients_paid) = get_stats(env);
    let updated_invoices = invoices
        .checked_add(1)
        .ok_or(ContractError::StatsOverflow)?;

    env.storage()
        .instance()
        .set(&invoices_key(env), &updated_invoices);

    publish_updated(env, (updated_invoices, volume, recipients_paid));
    Ok(())
}

pub fn record_volume(env: &Env, amount: i128) -> Result<(), ContractError> {
    let (invoices, volume, recipients_paid) = get_stats(env);
    let updated_volume = volume
        .checked_add(amount)
        .ok_or(ContractError::StatsOverflow)?;

    env.storage()
        .instance()
        .set(&volume_key(env), &updated_volume);

    publish_updated(env, (invoices, updated_volume, recipients_paid));
    Ok(())
}

pub fn record_recipients_paid(env: &Env, count: u64) -> Result<(), ContractError> {
    let (invoices, volume, recipients_paid) = get_stats(env);
    let updated_recipients_paid = recipients_paid
        .checked_add(count)
        .ok_or(ContractError::StatsOverflow)?;

    env.storage()
        .instance()
        .set(&recipients_paid_key(env), &updated_recipients_paid);

    publish_updated(env, (invoices, volume, updated_recipients_paid));
    Ok(())
fn total_invoices_key(env: &Env) -> Symbol {
    Symbol::new(env, TOTAL_INVOICES)
}

fn total_volume_key(env: &Env) -> Symbol {
    Symbol::new(env, TOTAL_VOLUME)
}

fn total_recipients_paid_key(env: &Env) -> Symbol {
    Symbol::new(env, TOTAL_RECIPIENTS_PAID)
}

/// Returns all aggregate counters, defaulting missing instance entries to zero.
pub fn get_stats(env: &Env) -> Stats {
    let storage = env.storage().instance();

    (
        storage
            .get(&total_invoices_key(env))
            .unwrap_or(0u64),
        storage
            .get(&total_volume_key(env))
            .unwrap_or(0i128),
        storage
            .get(&total_recipients_paid_key(env))
            .unwrap_or(0u64),
    )
}

/// Applies a statistics delta atomically.
///
/// Every new value is calculated before any storage write occurs. If any
/// checked addition fails, the function returns StatsOverflow and leaves the
/// counters unchanged.
pub fn increment(
    env: &Env,
    invoices: u64,
    volume: i128,
    recipients_paid: u64,
) -> Result<Stats, ContractError> {
    let (current_invoices, current_volume, current_recipients_paid) = get_stats(env);

    let next_invoices = current_invoices
        .checked_add(invoices)
        .ok_or(ContractError::StatsOverflow)?;
    let next_volume = current_volume
        .checked_add(volume)
        .ok_or(ContractError::StatsOverflow)?;
    let next_recipients_paid = current_recipients_paid
        .checked_add(recipients_paid)
        .ok_or(ContractError::StatsOverflow)?;

    let mut storage = env.storage().instance();
    storage.set(&total_invoices_key(env), &next_invoices);
    storage.set(&total_volume_key(env), &next_volume);
    storage.set(&total_recipients_paid_key(env), &next_recipients_paid);

    env.events().publish(
        (Symbol::new(env, STATS_UPDATED),),
        (
            next_invoices,
            next_volume,
            next_recipients_paid,
            env.ledger().sequence(),
        ),
    );

    Ok((next_invoices, next_volume, next_recipients_paid))
}

/// Records one newly created invoice.
pub fn invoice_created(env: &Env) -> Result<Stats, ContractError> {
    increment(env, 1, 0, 0)
}

/// Records the volume of a payment received for an invoice.
pub fn volume_added(env: &Env, amount: i128) -> Result<Stats, ContractError> {
    if amount < 0 {
        return Err(ContractError::InvalidAmount);
    }

    increment(env, 0, amount, 0)
}

/// Records recipients paid when an invoice's funds are released.
pub fn recipients_paid(env: &Env, count: u64) -> Result<Stats, ContractError> {
    increment(env, 0, 0, count)
}
