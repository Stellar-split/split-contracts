use soroban_sdk::{Env, Symbol};

use crate::error::ContractError;

/// Protocol-wide aggregate counters.
///
/// The counters are stored in instance storage, so they do not require a
/// persistent-storage TTL and survive independently of individual invoices.
pub struct ProtocolStats {
    pub total_invoices: u64,
    pub total_volume: i128,
    pub total_recipients_paid: u64,
}

const TOTAL_INVOICES: &str = "stats_total_invoices";
const TOTAL_VOLUME: &str = "stats_total_volume";
const TOTAL_RECIPIENTS_PAID: &str = "stats_total_recipients_paid";
const STATS_UPDATED: &str = "StatsUpdated";

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
pub fn get_stats(env: &Env) -> ProtocolStats {
    let storage = env.storage().instance();

    ProtocolStats {
        total_invoices: storage
            .get(&total_invoices_key(env))
            .unwrap_or(0u64),
        total_volume: storage
            .get(&total_volume_key(env))
            .unwrap_or(0i128),
        total_recipients_paid: storage
            .get(&total_recipients_paid_key(env))
            .unwrap_or(0u64),
    }
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
) -> Result<ProtocolStats, ContractError> {
    let current = get_stats(env);

    let next_invoices = current.total_invoices
        .checked_add(invoices)
        .ok_or(ContractError::StatsOverflow)?;
    let next_volume = current.total_volume
        .checked_add(volume)
        .ok_or(ContractError::StatsOverflow)?;
    let next_recipients_paid = current.total_recipients_paid
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

    Ok(ProtocolStats {
        total_invoices: next_invoices,
        total_volume: next_volume,
        total_recipients_paid: next_recipients_paid,
    })
}

/// Records one newly created invoice.
pub fn invoice_created(env: &Env) -> Result<ProtocolStats, ContractError> {
    increment(env, 1, 0, 0)
}

/// Records the volume of a payment received for an invoice.
pub fn volume_added(env: &Env, amount: i128) -> Result<ProtocolStats, ContractError> {
    if amount < 0 {
        return Err(ContractError::InvalidAmount);
    }

    increment(env, 0, amount, 0)
}

/// Records recipients paid when an invoice's funds are released.
pub fn recipients_paid(env: &Env, count: u64) -> Result<ProtocolStats, ContractError> {
    increment(env, 0, 0, count)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::Env;

    #[test]
    fn test_get_stats_initial_state() {
        let env = Env::default();
        let stats = get_stats(&env);
        assert_eq!(stats.total_invoices, 0);
        assert_eq!(stats.total_volume, 0);
        assert_eq!(stats.total_recipients_paid, 0);
    }

    #[test]
    fn test_invoice_created_increments_counter() {
        let env = Env::default();
        invoice_created(&env).unwrap();
        let stats = get_stats(&env);
        assert_eq!(stats.total_invoices, 1);
        assert_eq!(stats.total_volume, 0);
        assert_eq!(stats.total_recipients_paid, 0);
    }

    #[test]
    fn test_volume_added() {
        let env = Env::default();
        volume_added(&env, 1_000).unwrap();
        let stats = get_stats(&env);
        assert_eq!(stats.total_invoices, 0);
        assert_eq!(stats.total_volume, 1_000);
        assert_eq!(stats.total_recipients_paid, 0);
    }

    #[test]
    fn test_recipients_paid_increments() {
        let env = Env::default();
        recipients_paid(&env, 3).unwrap();
        let stats = get_stats(&env);
        assert_eq!(stats.total_invoices, 0);
        assert_eq!(stats.total_volume, 0);
        assert_eq!(stats.total_recipients_paid, 3);
    }

    #[test]
    fn test_increment_uses_named_fields() {
        let env = Env::default();
        let result = increment(&env, 2, 500, 4).unwrap();
        // Named fields — not positional
        assert_eq!(result.total_invoices, 2);
        assert_eq!(result.total_volume, 500);
        assert_eq!(result.total_recipients_paid, 4);
    }
}
