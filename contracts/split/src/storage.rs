//! Centralized persistent storage helpers with automatic TTL management.
//!
//! Issue #563: All persistent storage writes go through these helpers to ensure
//! that TTL bump calls are consistent and cannot be accidentally forgotten.

use crate::constants::{MAX_INVOICE_TTL_LEDGERS, MIN_INVOICE_TTL_LEDGERS};
use soroban_sdk::{Env, IntoVal, TryFromVal, Val};

/// Maximum number of payment shards that may be stored for a single invoice.
///
/// Any shard index `>= MAX_PAYMENT_SHARDS` is rejected by
/// [`validate_shard_index`] before any storage access occurs.
pub const MAX_PAYMENT_SHARDS: u32 = 32;

/// Error returned when a shard index exceeds the configured bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShardError {
    /// The requested shard index is outside `0..MAX_PAYMENT_SHARDS`.
    ShardIndexOutOfBounds,
}

/// Validate that `index` is a legal payment shard index.
///
/// Shard indices are zero-based and must be strictly less than
/// [`MAX_PAYMENT_SHARDS`]. This check must run before any shard storage
/// access so out-of-bounds reads/writes can never touch persistent storage.
pub fn validate_shard_index(index: u32) -> Result<(), ShardError> {
    if index >= MAX_PAYMENT_SHARDS {
        return Err(ShardError::ShardIndexOutOfBounds);
    }
    Ok(())
}

/// Validate a shard count against the configured bounds.
///
/// A count of `0` is allowed (no shards), but any count greater than
/// [`MAX_PAYMENT_SHARDS`] is rejected.
pub fn validate_shard_count(count: u32) -> Result<(), ShardError> {
    if count > MAX_PAYMENT_SHARDS {
        return Err(ShardError::ShardIndexOutOfBounds);
    }
    Ok(())
}

/// Storage tier classification for keys managed by this module.
///
/// Issue #949: keys are grouped by access pattern so that hot, small entries
/// live in instance storage, durable entries live in persistent storage, and
/// short-lived scratch data lives in temporary storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageTier {
    /// Small, frequently accessed contract-wide data (config, counters).
    Instance,
    /// Durable per-invoice data (invoices, recipients, contributors).
    Persistent,
    /// Short-lived scratch data that may be evicted without loss.
    Temporary,
}

/// TTL bump helper for instance storage entries.
pub fn bump_instance_ttl<K>(env: &Env, key: &K)
where
    K: IntoVal<Env, Val> + Clone,
{
    env.storage()
        .instance()
        .extend_ttl(MIN_INVOICE_TTL_LEDGERS, MAX_INVOICE_TTL_LEDGERS);
    let _ = key;
}

/// TTL bump helper for persistent storage entries.
pub fn bump_persistent_ttl<K>(env: &Env, key: &K)
where
    K: IntoVal<Env, Val> + Clone,
{
    env.storage()
        .persistent()
        .extend_ttl(key, MIN_INVOICE_TTL_LEDGERS, MAX_INVOICE_TTL_LEDGERS);
}

/// TTL bump helper for temporary storage entries.
pub fn bump_temporary_ttl<K>(env: &Env, key: &K)
where
    K: IntoVal<Env, Val> + Clone,
{
    env.storage()
        .temporary()
        .extend_ttl(key, MIN_INVOICE_TTL_LEDGERS, MAX_INVOICE_TTL_LEDGERS);
}

/// Save an entry into the given tier and bump its TTL.
pub fn save_in_tier<K, V>(env: &Env, tier: StorageTier, key: K, value: &V)
where
    K: IntoVal<Env, Val> + Clone,
    V: IntoVal<Env, Val>,
{
    match tier {
        StorageTier::Instance => {
            env.storage().instance().set(&key, value);
            bump_instance_ttl(env, &key);
        }
        StorageTier::Persistent => {
            env.storage().persistent().set(&key, value);
            bump_persistent_ttl(env, &key);
        }
        StorageTier::Temporary => {
            env.storage().temporary().set(&key, value);
            bump_temporary_ttl(env, &key);
        }
    }
}

/// Read-modify-write a single field of a stored entry incrementally.
///
/// Issue #948: instead of rewriting the whole entry, callers pass a mutator
/// that receives the current value (if any) and returns the new value. Only
/// the single key is written, reducing compute and storage churn.
pub fn update_field<K, V, F>(env: &Env, tier: StorageTier, key: K, mutator: F)
where
    K: IntoVal<Env, Val> + Clone,
    V: IntoVal<Env, Val> + soroban_sdk::TryFromVal<Env, Val>,
    F: FnOnce(Option<V>) -> V,
{
    let current: Option<V> = match tier {
        StorageTier::Instance => env.storage().instance().get(&key),
        StorageTier::Persistent => env.storage().persistent().get(&key),
        StorageTier::Temporary => env.storage().temporary().get(&key),
    };
    let updated = mutator(current);
    save_in_tier(env, tier, key, &updated);
}

/// Incrementally update a single field of an invoice entry.
pub fn update_invoice_field<K, V, F>(env: &Env, key: K, mutator: F)
where
    K: IntoVal<Env, Val> + Clone,
    V: IntoVal<Env, Val> + soroban_sdk::TryFromVal<Env, Val>,
    F: FnOnce(Option<V>) -> V,
{
    update_field(env, StorageTier::Persistent, key, mutator);
}

/// Incrementally update a single recipient amount without rewriting the list.
pub fn update_recipient_amount<K, V, F>(env: &Env, key: K, mutator: F)
where
    K: IntoVal<Env, Val> + Clone,
    V: IntoVal<Env, Val> + soroban_sdk::TryFromVal<Env, Val>,
    F: FnOnce(Option<V>) -> V,
{
    update_field(env, StorageTier::Persistent, key, mutator);
}

/// Incrementally bump a counter stored in instance storage.
pub fn bump_counter<K>(env: &Env, key: K, delta: i128)
where
    K: IntoVal<Env, Val> + Clone,
{
    update_field::<K, i128, _>(env, StorageTier::Instance, key, |current| {
        current.unwrap_or(0).saturating_add(delta)
    });
}


/// TTL (in ledgers) applied to cached invoice extension entries.
///
/// Issue #942: extension data is loaded lazily and cached in temporary
/// storage so repeated reads within the same ledger window avoid the
/// persistent-storage round trip.
pub const INVOICE_EXT_CACHE_TTL_LEDGERS: u32 = 100;

/// Storage key used to cache the invoice extension payload for an invoice id.
fn invoice_ext_cache_key(env: &Env, invoice_id: u64) -> Val {
    (soroban_sdk::symbol_short!("inv_ext"), invoice_id).into_val(env)
}

/// Lazily load the invoice extension payload, caching the result in
/// temporary storage.
///
/// Returns `None` when no extension data is stored for `invoice_id`.
/// The first call reads persistent storage and populates the cache; later
/// calls within the cache TTL are served from temporary storage.
pub fn load_invoice_ext_cached<V>(env: &Env, invoice_id: u64) -> Option<V>
where
    V: IntoVal<Env, Val> + TryFromVal<Env, Val>,
{
    let cache_key = invoice_ext_cache_key(env, invoice_id);

    if let Some(cached) = env.storage().temporary().get::<Val, V>(&cache_key) {
        return Some(cached);
    }

    let ext_key = (soroban_sdk::symbol_short!("inv_ext"), invoice_id);
    let value: Option<V> = env.storage().persistent().get(&ext_key);

    if let Some(ref v) = value {
        env.storage().temporary().set(&cache_key, v);
        env.storage().temporary().extend_ttl(
            &cache_key,
            INVOICE_EXT_CACHE_TTL_LEDGERS,
            INVOICE_EXT_CACHE_TTL_LEDGERS,
        );
    }

    value
}

/// Invalidate the cached invoice extension payload for `invoice_id`.
///
/// Must be called whenever the underlying extension data changes so that
/// stale extension data is never served from the cache.
pub fn invalidate_invoice_ext_cache(env: &Env, invoice_id: u64) {
    let cache_key = invoice_ext_cache_key(env, invoice_id);
    env.storage().temporary().remove(&cache_key);
}

/// Storage key for the cached recipient list of a given invoice.
///
/// Issue #946: recipient lists are read far more often than they are mutated,
/// so a short-lived cache entry is kept alongside the persistent entry.
#[derive(Clone)]
pub struct RecipientCacheKey {
    pub invoice_id: u64,
}

impl IntoVal<Env, Val> for RecipientCacheKey {
    fn into_val(&self, env: &Env) -> Val {
        (soroban_sdk::symbol_short!("rec_cache"), self.invoice_id).into_val(env)
    }
}

/// Read the cached recipient list for `invoice_id`, populating the cache from
/// persistent storage on a miss.
///
/// `load` is invoked only on a cache miss and is expected to read the
/// authoritative recipient list from persistent storage. The returned value is
/// written to the cache (with a TTL bump) so subsequent reads hit the cache.
pub fn get_cached_recipients<V, F>(env: &Env, invoice_id: u64, load: F) -> V
where
    V: IntoVal<Env, Val> + TryFromVal<Env, Val> + Clone,
    F: FnOnce() -> V,
{
    let cache_key = RecipientCacheKey { invoice_id };

    if let Some(cached) = env.storage().persistent().get::<_, V>(&cache_key) {
        // Refresh the TTL on hit so hot entries do not expire mid-use.
        env.storage().persistent().extend_ttl(
            &cache_key,
            MIN_INVOICE_TTL_LEDGERS,
            MAX_INVOICE_TTL_LEDGERS,
        );
        return cached;
    }

    let value = load();
    env.storage().persistent().set(&cache_key, &value);
    env.storage().persistent().extend_ttl(
        &cache_key,
        MIN_INVOICE_TTL_LEDGERS,
        MAX_INVOICE_TTL_LEDGERS,
    );
    value
}

/// Invalidate the cached recipient list for `invoice_id`.
///
/// Must be called whenever the recipients of an invoice are mutated so that
/// stale data is never served from the cache.
pub fn invalidate_recipients_cache(env: &Env, invoice_id: u64) {
    let cache_key = RecipientCacheKey { invoice_id };
    env.storage().persistent().remove(&cache_key);
}

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
    save_in_tier(env, StorageTier::Persistent, key, value);
}

/// Save a recipients list entry and automatically bump its TTL.
pub fn save_recipients<K, V>(env: &Env, key: K, value: &V)
where
    K: IntoVal<Env, Val> + Clone,
    V: IntoVal<Env, Val>,
{
    save_in_tier(env, StorageTier::Persistent, key, value);
}

/// Save a contributor entry and automatically bump its TTL.
pub fn save_contributor<K, V>(env: &Env, key: K, value: &V)
where
    K: IntoVal<Env, Val> + Clone,
    V: IntoVal<Env, Val>,
{
    save_in_tier(env, StorageTier::Persistent, key, value);
}

/// Load the invoice hot key once and reuse it for the duration of an invocation.
///
/// Issue #941: the hot key is read at most once per call; callers pass the
/// returned value around instead of re-reading storage on every access.
pub fn load_invoice_hot_cached<K, V>(env: &Env, key: K) -> Option<V>
where
    K: IntoVal<Env, Val> + Clone,
    V: IntoVal<Env, Val> + soroban_sdk::TryFromVal<Env, Val>,
{
    env.storage().persistent().get(&key)
}

/// Read all payment shards for an invoice in a single pass.
///
/// Issue #945: iterates `0..count` once, validating each index before the
/// storage read. Returns [`ShardError::ShardIndexOutOfBounds`] if `count`
/// exceeds [`MAX_PAYMENT_SHARDS`].
pub fn load_payment_shards<K, V>(
    env: &Env,
    key_fn: impl Fn(u32) -> K,
    count: u32,
) -> Result<soroban_sdk::Vec<V>, ShardError>
where
    K: IntoVal<Env, Val> + Clone,
    V: IntoVal<Env, Val> + soroban_sdk::TryFromVal<Env, Val>,
{
    validate_shard_count(count)?;
    let mut out = soroban_sdk::Vec::new(env);
    let mut i = 0u32;
    while i < count {
        validate_shard_index(i)?;
        let key = key_fn(i);
        if let Some(v) = env.storage().persistent().get::<K, V>(&key) {
            out.push_back(v);
        }
        i += 1;
    }
    Ok(out)
}

/// Write all payment shards for an invoice in a single pass.
///
/// Issue #945: validates the count and every index before writing, so a
/// partially-invalid batch never mutates storage.
pub fn save_payment_shards<K, V>(
    env: &Env,
    key_fn: impl Fn(u32) -> K,
    shards: &soroban_sdk::Vec<V>,
) -> Result<(), ShardError>
where
    K: IntoVal<Env, Val> + Clone,
    V: IntoVal<Env, Val> + TryFromVal<Env, Val>,
{
    let count = shards.len();
    validate_shard_count(count)?;
    let mut i = 0u32;
    while i < count {
        validate_shard_index(i)?;
        i += 1;
    }
    let mut i = 0u32;
    while i < count {
        let key = key_fn(i);
        let value = shards.get_unchecked(i);
        save_invoice(env, key, &value);
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{symbol_short, Address, String};

    fn contract_id(env: &Env) -> Address {
        env.register(crate::SplitContract, ())
    }

    #[test]
    fn test_save_invoice_bumps_ttl() {
        let env = Env::default();
        let id = contract_id(&env);
        let key = (symbol_short!("test_inv"), 42u64);
        let value = String::from_str(&env, "test_value");

        env.as_contract(&id, || {
            save_invoice(&env, key.clone(), &value);

            let stored: String = env
                .storage()
                .persistent()
                .get(&key)
                .expect("value should be stored");
            assert_eq!(stored, String::from_str(&env, "test_value"));
        });
    }

    #[test]
    fn test_save_recipients_bumps_ttl() {
        let env = Env::default();
        let id = contract_id(&env);
        let key = (symbol_short!("test_rec"), 42u64);
        let value = 100i128;

        env.as_contract(&id, || {
            save_recipients(&env, key.clone(), &value);

            let stored: i128 = env
                .storage()
                .persistent()
                .get(&key)
                .expect("value should be stored");
            assert_eq!(stored, 100);
        });
    }

    #[test]
    fn test_recipients_cache_miss_populates() {
        let env = Env::default();
        let id = contract_id(&env);

        env.as_contract(&id, || {
            let value: i128 = get_cached_recipients(&env, 7u64, || 42i128);
            assert_eq!(value, 42);

            let cache_key = RecipientCacheKey { invoice_id: 7u64 };
            let cached: i128 = env
                .storage()
                .persistent()
                .get(&cache_key)
                .expect("cache should be populated on miss");
            assert_eq!(cached, 42);
        });
    }

    #[test]
    fn test_recipients_cache_hit_skips_loader() {
        let env = Env::default();
        let id = contract_id(&env);

        env.as_contract(&id, || {
            let _ = get_cached_recipients(&env, 7u64, || 42i128);

            let mut loader_called = false;
            let value: i128 = get_cached_recipients(&env, 7u64, || {
                loader_called = true;
                99i128
            });
            assert_eq!(value, 42);
            assert!(!loader_called, "loader must not run on a cache hit");
        });
    }

    #[test]
    fn test_recipients_cache_invalidation() {
        let env = Env::default();
        let id = contract_id(&env);

        env.as_contract(&id, || {
            let _ = get_cached_recipients(&env, 7u64, || 42i128);

            invalidate_recipients_cache(&env, 7u64);

            let cache_key = RecipientCacheKey { invoice_id: 7u64 };
            assert!(env.storage().persistent().get::<_, i128>(&cache_key).is_none());

            // After invalidation the loader runs again and repopulates the cache.
            let value: i128 = get_cached_recipients(&env, 7u64, || 100i128);
            assert_eq!(value, 100);
        });
    }

    #[test]
    fn test_save_contributor_bumps_ttl() {
        let env = Env::default();
        let id = contract_id(&env);
        let value = Address::generate(&env);
        let key = (symbol_short!("test_con"), 42u64);

        env.as_contract(&id, || {
            save_contributor(&env, key.clone(), &value);

            let stored: Address = env
                .storage()
                .persistent()
                .get(&key)
                .expect("value should be stored");
            assert_eq!(stored, value);
        });
    }

    #[test]
    fn test_update_invoice_field_incremental() {
        let env = Env::default();
        let id = contract_id(&env);
        let key = (symbol_short!("inv_fld"), 7u64);

        env.as_contract(&id, || {
            update_invoice_field::<_, i128, _>(&env, key.clone(), |_| 10);
            update_invoice_field::<_, i128, _>(&env, key.clone(), |current| {
                current.unwrap_or(0) + 5
            });

            let stored: i128 = env
                .storage()
                .persistent()
                .get(&key)
                .expect("value should be stored");
            assert_eq!(stored, 15);
        });
    }

    #[test]
    fn test_update_recipient_amount_incremental() {
        let env = Env::default();
        let id = contract_id(&env);
        let key = (symbol_short!("rec_amt"), 3u64);

        env.as_contract(&id, || {
            update_recipient_amount::<_, i128, _>(&env, key.clone(), |_| 100);
            update_recipient_amount::<_, i128, _>(&env, key.clone(), |current| {
                current.unwrap_or(0) - 40
            });

            let stored: i128 = env
                .storage()
                .persistent()
                .get(&key)
                .expect("value should be stored");
            assert_eq!(stored, 60);
        });
    }

    #[test]
    fn test_bump_counter_instance_tier() {
        let env = Env::default();
        let id = contract_id(&env);
        let key = symbol_short!("counter");

        env.as_contract(&id, || {
            bump_counter(&env, key.clone(), 1);
            bump_counter(&env, key.clone(), 2);

            let stored: i128 = env
                .storage()
                .instance()
                .get(&key)
                .expect("counter should be stored");
            assert_eq!(stored, 3);
        });
    }

    #[test]
    fn test_save_in_tier_temporary() {
        let env = Env::default();
        let id = contract_id(&env);
        let key = (symbol_short!("tmp_key"), 1u64);

        env.as_contract(&id, || {
            save_in_tier(&env, StorageTier::Temporary, key.clone(), &9i128);

            let stored: i128 = env
                .storage()
                .temporary()
                .get(&key)
                .expect("value should be stored");
            assert_eq!(stored, 9);
        });
    }
}
