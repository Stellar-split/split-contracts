use soroban_sdk::{Env, Symbol};

use crate::error::ContractError;

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
}
