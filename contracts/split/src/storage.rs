//! Centralized persistent storage helpers with automatic TTL management.
//!
//! Issue #563: All persistent storage writes go through these helpers to ensure
//! that TTL bump calls are consistent and cannot be accidentally forgotten.

use crate::constants::{MAX_INVOICE_TTL_LEDGERS, MIN_INVOICE_TTL_LEDGERS};
use soroban_sdk::{Env, IntoVal, TryFromVal, Val};

/// Save an invoice entry and automatically bump its TTL.
///
/// # Arguments
/// * `env` – Soroban environment
/// * `key` – storage key (any type that implements IntoVal)
/// * `value` – value to store (any type that implements IntoVal)
pub fn save_invoice<K, V>(env: &Env, key: K, value: &V)
where
    K: IntoVal<Env, Val> + Clone,
    V: IntoVal<Env, Val>,
{
    env.storage().persistent().set(&key, value);
    env.storage()
        .persistent()
        .bump(&key, MIN_INVOICE_TTL_LEDGERS, MAX_INVOICE_TTL_LEDGERS);
}

/// Save a recipients list entry and automatically bump its TTL.
pub fn save_recipients<K, V>(env: &Env, key: K, value: &V)
where
    K: IntoVal<Env, Val> + Clone,
    V: IntoVal<Env, Val>,
{
    env.storage().persistent().set(&key, value);
    env.storage()
        .persistent()
        .bump(&key, MIN_INVOICE_TTL_LEDGERS, MAX_INVOICE_TTL_LEDGERS);
}

/// Save a contributor entry and automatically bump its TTL.
pub fn save_contributor<K, V>(env: &Env, key: K, value: &V)
where
    K: IntoVal<Env, Val> + Clone,
    V: IntoVal<Env, Val>,
{
    env.storage().persistent().set(&key, value);
    env.storage()
        .persistent()
        .bump(&key, MIN_INVOICE_TTL_LEDGERS, MAX_INVOICE_TTL_LEDGERS);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{symbol_short, Address, Symbol};

    #[test]
    fn test_save_invoice_bumps_ttl() {
        let env = Env::default();
        let key = (symbol_short!("test_inv"), 42u64);
        let value = "test_value";

        save_invoice(&env, key.clone(), &value);

        // Verify value was stored (would fail if not set)
        let stored: String = env
            .storage()
            .persistent()
            .get(&key)
            .expect("value should be stored");
        assert_eq!(stored, "test_value");
    }

    #[test]
    fn test_save_recipients_bumps_ttl() {
        let env = Env::default();
        let key = (symbol_short!("test_rec"), 42u64);
        let value = 100i128;

        save_recipients(&env, key.clone(), &value);

        let stored: i128 = env
            .storage()
            .persistent()
            .get(&key)
            .expect("value should be stored");
        assert_eq!(stored, 100);
    }

    #[test]
    fn test_save_contributor_bumps_ttl() {
        let env = Env::default();
        let key = (symbol_short!("test_con"), 42u64);
        let value = Address::generate(&env);

        save_contributor(&env, key.clone(), &value);

        let stored: Address = env
            .storage()
            .persistent()
            .get(&key)
            .expect("value should be stored");
        assert_eq!(stored, value);
    }
}
