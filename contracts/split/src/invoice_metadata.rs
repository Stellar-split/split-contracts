//! Issue #920: Per-invoice key-value metadata store.
//!
//! Creators attach small pieces of custom data to an invoice (order numbers,
//! PO references, external ids, content hashes, ...) without changing the
//! invoice struct. Complements the single off-chain `metadata_hash`
//! (issue #329) with structured, individually addressable entries.
//!
//! ## Validation
//! * Keys: 1 – `MAX_METADATA_KEY_LEN` bytes of `[a-z0-9_.-]`.
//! * Values: 1 – `MAX_METADATA_VALUE_LEN` bytes (opaque).
//! * At most `MAX_METADATA_ENTRIES` keys per invoice.
//! * Only the creator or a co-creator may write; cancelled or deleted
//!   invoices are read-only.
//! * Once locked via `lock_invoice_metadata`, the store is permanently
//!   read-only (useful once metadata is referenced by an audit or receipt).
//!
//! ## Storage
//! Each entry lives under its own key so single-key reads and writes touch
//! one ledger entry regardless of how many keys an invoice has:
//! * `(imeta_ent, invoice_id, key)` → [`MetadataEntry`] (persistent)
//! * `(imeta_idx, invoice_id)` → `Vec<String>` key index, insertion order (persistent)
//! * `(imeta_lck, invoice_id)` → `bool` lock flag (persistent)

use soroban_sdk::{contractimpl, contracttype, symbol_short, Address, Bytes, Env, Map, String, Symbol, Vec};

use crate::events;
use crate::types::{Invoice, InvoiceStatus};
use crate::{load_invoice, SplitContract, SplitContractArgs, SplitContractClient};

/// Maximum number of metadata keys per invoice.
pub const MAX_METADATA_ENTRIES: u32 = 20;

/// Maximum key length in bytes.
pub const MAX_METADATA_KEY_LEN: u32 = 32;

/// Maximum value length in bytes.
pub const MAX_METADATA_VALUE_LEN: u32 = 256;

/// Maximum number of entries written by one `set_invoice_metadata_batch` call.
pub const MAX_METADATA_BATCH: u32 = 10;

/// A single metadata entry.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetadataEntry {
    pub key: String,
    pub value: Bytes,
    /// Starts at 1 and increments on every overwrite of this key.
    pub version: u32,
    pub updated_by: Address,
    pub updated_at: u64,
}

fn metadata_entry_key(invoice_id: u64, key: &String) -> (Symbol, u64, String) {
    (symbol_short!("imeta_ent"), invoice_id, key.clone())
}

fn metadata_index_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("imeta_idx"), invoice_id)
}

fn metadata_lock_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("imeta_lck"), invoice_id)
}

/// `true` for bytes allowed in a metadata key.
fn is_valid_key_byte(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.' || b == b'-'
}

fn validate_key(key: &String) {
    let len = key.len();
    assert!(len > 0, "metadata key must not be empty");
    assert!(len <= MAX_METADATA_KEY_LEN, "metadata key too long");
    let mut buf = [0u8; MAX_METADATA_KEY_LEN as usize];
    let slice = &mut buf[..len as usize];
    key.copy_into_slice(slice);
    assert!(
        slice.iter().all(|b| is_valid_key_byte(*b)),
        "metadata key contains invalid characters"
    );
}

fn validate_value(value: &Bytes) {
    assert!(!value.is_empty(), "metadata value must not be empty");
    assert!(value.len() <= MAX_METADATA_VALUE_LEN, "metadata value too long");
}

fn load_index(env: &Env, invoice_id: u64) -> Vec<String> {
    env.storage()
        .persistent()
        .get(&metadata_index_key(invoice_id))
        .unwrap_or(Vec::new(env))
}

fn save_index(env: &Env, invoice_id: u64, index: &Vec<String>) {
    if index.is_empty() {
        env.storage().persistent().remove(&metadata_index_key(invoice_id));
    } else {
        env.storage()
            .persistent()
            .set(&metadata_index_key(invoice_id), index);
    }
}

fn is_locked(env: &Env, invoice_id: u64) -> bool {
    env.storage()
        .persistent()
        .get(&metadata_lock_key(invoice_id))
        .unwrap_or(false)
}

/// Authenticate `caller` and check they may modify `invoice_id`'s metadata.
fn require_metadata_writer(env: &Env, caller: &Address, invoice_id: u64) -> Invoice {
    crate::require_not_paused(env);
    caller.require_auth();
    let invoice = load_invoice(env, invoice_id);
    let is_creator =
        invoice.creator == *caller || invoice.co_creators.iter().any(|c| c == *caller);
    assert!(is_creator, "NotAuthorized: only creator or co-creator may edit metadata");
    assert!(
        invoice.status != InvoiceStatus::Cancelled && invoice.status != InvoiceStatus::Deleted,
        "metadata is read-only for cancelled or deleted invoices"
    );
    assert!(!is_locked(env, invoice_id), "invoice metadata is locked");
    invoice
}

/// Validate and write one entry, updating `index` in memory. Returns the
/// stored entry. The caller persists `index`.
fn write_entry(
    env: &Env,
    caller: &Address,
    invoice_id: u64,
    index: &mut Vec<String>,
    key: String,
    value: Bytes,
) -> MetadataEntry {
    validate_key(&key);
    validate_value(&value);

    let storage_key = metadata_entry_key(invoice_id, &key);
    let previous: Option<MetadataEntry> = env.storage().persistent().get(&storage_key);
    let version = match previous {
        Some(ref prev) => prev.version + 1,
        None => {
            assert!(index.len() < MAX_METADATA_ENTRIES, "too many metadata entries");
            index.push_back(key.clone());
            1
        }
    };

    let entry = MetadataEntry {
        key: key.clone(),
        value,
        version,
        updated_by: caller.clone(),
        updated_at: env.ledger().timestamp(),
    };
    env.storage().persistent().set(&storage_key, &entry);
    events::invoice_metadata_set(env, invoice_id, &key, version, caller);
    entry
}

#[contractimpl]
impl SplitContract {
    /// Issue #920: Create or overwrite one metadata entry on an invoice.
    ///
    /// # Panics
    /// * `"NotAuthorized: ..."` — caller is not the creator or a co-creator.
    /// * `"metadata key ..."` / `"metadata value ..."` — validation failure.
    /// * `"too many metadata entries"` — adding a new key past the limit.
    /// * `"invoice metadata is locked"`.
    pub fn set_invoice_metadata(
        env: Env,
        caller: Address,
        invoice_id: u64,
        key: String,
        value: Bytes,
    ) -> MetadataEntry {
        require_metadata_writer(&env, &caller, invoice_id);
        let mut index = load_index(&env, invoice_id);
        let entry = write_entry(&env, &caller, invoice_id, &mut index, key, value);
        save_index(&env, invoice_id, &index);
        entry
    }

    /// Issue #920: Write several entries in one call (all-or-nothing, since a
    /// failed validation aborts the transaction). Returns the number written.
    pub fn set_invoice_metadata_batch(
        env: Env,
        caller: Address,
        invoice_id: u64,
        entries: Map<String, Bytes>,
    ) -> u32 {
        assert!(!entries.is_empty(), "no metadata entries supplied");
        assert!(
            entries.len() <= MAX_METADATA_BATCH,
            "too many metadata entries in batch"
        );
        require_metadata_writer(&env, &caller, invoice_id);
        let mut index = load_index(&env, invoice_id);
        for (key, value) in entries.iter() {
            write_entry(&env, &caller, invoice_id, &mut index, key, value);
        }
        save_index(&env, invoice_id, &index);
        entries.len()
    }

    /// Issue #920: Delete one entry. Returns `false` when the key was absent.
    pub fn remove_invoice_metadata(env: Env, caller: Address, invoice_id: u64, key: String) -> bool {
        require_metadata_writer(&env, &caller, invoice_id);
        let storage_key = metadata_entry_key(invoice_id, &key);
        if !env.storage().persistent().has(&storage_key) {
            return false;
        }
        env.storage().persistent().remove(&storage_key);

        let mut index = load_index(&env, invoice_id);
        if let Some(pos) = index.first_index_of(&key) {
            index.remove(pos);
        }
        save_index(&env, invoice_id, &index);
        events::invoice_metadata_removed(&env, invoice_id, &key, &caller);
        true
    }

    /// Issue #920: Delete every entry on an invoice. Returns the number removed.
    pub fn clear_invoice_metadata(env: Env, caller: Address, invoice_id: u64) -> u32 {
        require_metadata_writer(&env, &caller, invoice_id);
        let index = load_index(&env, invoice_id);
        for key in index.iter() {
            env.storage()
                .persistent()
                .remove(&metadata_entry_key(invoice_id, &key));
            events::invoice_metadata_removed(&env, invoice_id, &key, &caller);
        }
        save_index(&env, invoice_id, &Vec::new(&env));
        index.len()
    }

    /// Issue #920: Permanently make an invoice's metadata read-only.
    pub fn lock_invoice_metadata(env: Env, caller: Address, invoice_id: u64) {
        require_metadata_writer(&env, &caller, invoice_id);
        env.storage()
            .persistent()
            .set(&metadata_lock_key(invoice_id), &true);
        events::invoice_metadata_locked(&env, invoice_id, &caller);
    }

    /// Issue #920: Whether an invoice's metadata has been locked.
    pub fn is_invoice_metadata_locked(env: Env, invoice_id: u64) -> bool {
        is_locked(&env, invoice_id)
    }

    /// Issue #920: Value stored under `key`, if any. Single ledger read.
    pub fn get_invoice_metadata(env: Env, invoice_id: u64, key: String) -> Option<Bytes> {
        env.storage()
            .persistent()
            .get::<_, MetadataEntry>(&metadata_entry_key(invoice_id, &key))
            .map(|e| e.value)
    }

    /// Issue #920: Full entry (value plus version / author / timestamp) for `key`.
    pub fn get_invoice_metadata_entry(env: Env, invoice_id: u64, key: String) -> Option<MetadataEntry> {
        env.storage()
            .persistent()
            .get(&metadata_entry_key(invoice_id, &key))
    }

    /// Issue #920: Whether `key` is set on the invoice.
    pub fn has_invoice_metadata(env: Env, invoice_id: u64, key: String) -> bool {
        env.storage()
            .persistent()
            .has(&metadata_entry_key(invoice_id, &key))
    }

    /// Issue #920: All keys set on the invoice, in insertion order.
    pub fn get_invoice_metadata_keys(env: Env, invoice_id: u64) -> Vec<String> {
        load_index(&env, invoice_id)
    }

    /// Issue #920: Every key/value pair on the invoice.
    pub fn get_all_invoice_metadata(env: Env, invoice_id: u64) -> Map<String, Bytes> {
        let mut all = Map::new(&env);
        for key in load_index(&env, invoice_id).iter() {
            if let Some(entry) = env
                .storage()
                .persistent()
                .get::<_, MetadataEntry>(&metadata_entry_key(invoice_id, &key))
            {
                all.set(key, entry.value);
            }
        }
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_keys_accepted() {
        let env = Env::default();
        validate_key(&String::from_str(&env, "order_id"));
        validate_key(&String::from_str(&env, "po-2026.10"));
        validate_key(&String::from_str(&env, "a"));
        validate_key(&String::from_str(&env, "abcdefghijklmnopqrstuvwxyz012345"));
    }

    #[test]
    #[should_panic(expected = "metadata key must not be empty")]
    fn test_empty_key_rejected() {
        let env = Env::default();
        validate_key(&String::from_str(&env, ""));
    }

    #[test]
    #[should_panic(expected = "metadata key too long")]
    fn test_long_key_rejected() {
        let env = Env::default();
        validate_key(&String::from_str(&env, "abcdefghijklmnopqrstuvwxyz0123456"));
    }

    #[test]
    #[should_panic(expected = "metadata key contains invalid characters")]
    fn test_uppercase_key_rejected() {
        let env = Env::default();
        validate_key(&String::from_str(&env, "OrderId"));
    }

    #[test]
    #[should_panic(expected = "metadata key contains invalid characters")]
    fn test_space_in_key_rejected() {
        let env = Env::default();
        validate_key(&String::from_str(&env, "order id"));
    }

    #[test]
    #[should_panic(expected = "metadata value must not be empty")]
    fn test_empty_value_rejected() {
        let env = Env::default();
        validate_value(&Bytes::new(&env));
    }

    #[test]
    #[should_panic(expected = "metadata value too long")]
    fn test_long_value_rejected() {
        let env = Env::default();
        validate_value(&Bytes::from_array(&env, &[7u8; (MAX_METADATA_VALUE_LEN + 1) as usize]));
    }

    #[test]
    fn test_max_value_accepted() {
        let env = Env::default();
        validate_value(&Bytes::from_array(&env, &[7u8; MAX_METADATA_VALUE_LEN as usize]));
    }

    #[test]
    fn test_reads_on_unknown_invoice_are_empty() {
        let env = Env::default();
        let contract_id = env.register(SplitContract, ());
        let client = SplitContractClient::new(&env, &contract_id);
        let key = String::from_str(&env, "order_id");
        assert_eq!(client.get_invoice_metadata(&42, &key), None);
        assert!(!client.has_invoice_metadata(&42, &key));
        assert!(client.get_invoice_metadata_keys(&42).is_empty());
        assert!(client.get_all_invoice_metadata(&42).is_empty());
        assert!(!client.is_invoice_metadata_locked(&42));
    }
}
