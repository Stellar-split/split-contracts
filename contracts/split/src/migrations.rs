//! Storage migration framework (versioned schema keys).
//!
//! `schema_version` is a `u32` kept in instance storage that records which
//! on-chain storage shape the contract currently has. The shared guard
//! helpers used across the contract (`require_admin`, `require_admin_role`,
//! `require_not_paused`, `check_not_paused`, `require_not_frozen`) all call
//! [`require_schema_current`] first, so a contract left on a stale schema
//! after a Wasm upgrade panics with `MigrationRequired` instead of reading or
//! writing storage in the wrong shape. `SplitContract::migrate` is the only
//! entry point exempt from that guard, since it is what performs the
//! upgrade.
//!
//! Fresh deployments call `initialize()`, which stamps the schema at
//! [`CURRENT_SCHEMA_VERSION`] immediately via [`init_schema_version`] — there
//! is nothing to migrate for storage that never existed in an older shape.
//! Only a contract whose Wasm was upgraded out from under existing data
//! (leaving `schema_version` at a stale value) needs `migrate`.

use super::*;
use soroban_sdk::contracttype;

/// Latest schema version this contract build understands. Bump when adding a
/// new `migration_vN` and wire it into [`run_pending_migrations`].
pub const CURRENT_SCHEMA_VERSION: u32 = 3;

/// Instance storage: current on-chain schema version.
pub(crate) fn schema_version_key() -> Symbol {
    symbol_short!("schm_ver")
}

/// Current schema version. Contracts deployed before this framework existed
/// never wrote the key, so absence means "version 1" — the original,
/// pre-migration-framework storage layout.
pub fn schema_version(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&schema_version_key())
        .unwrap_or(1)
}

fn set_schema_version(env: &Env, version: u32) {
    env.storage()
        .instance()
        .set(&schema_version_key(), &version);
}

/// Stamp a freshly-initialized contract at the current schema version.
pub fn init_schema_version(env: &Env) {
    set_schema_version(env, CURRENT_SCHEMA_VERSION);
}

/// Guard applied by every entry point except `migrate` itself: refuse to
/// operate on stale storage so a partially-upgraded contract can't silently
/// read or write data in the wrong shape.
pub fn require_schema_current(env: &Env) {
    if schema_version(env) < CURRENT_SCHEMA_VERSION {
        panic!("MigrationRequired");
    }
}

// ---------------------------------------------------------------------------
// v1 -> v2: introduce a per-invoice note record.
// ---------------------------------------------------------------------------
// Every invoice created under schema v1 has no associated `InvoiceMeta`. This
// migration backfills a default record for each existing invoice id so later
// code can assume the record exists once schema_version >= 2.

/// Per-invoice metadata introduced at schema v2, stored independently of the
/// core `Invoice` record so existing invoice fields are untouched.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct InvoiceMeta {
    pub note: String,
    pub priority: u32,
}

pub(crate) fn invoice_meta_key_v2(id: u64) -> (Symbol, u64) {
    (symbol_short!("inv_meta"), id)
}

fn migration_v2(env: &Env) {
    // The invoice counter holds the highest assigned id (ids are 1-based),
    // not a next-id cursor — see `_create_invoice_inner`.
    let last_id: u64 = env.storage().persistent().get(&counter_key()).unwrap_or(0);
    let default_meta = InvoiceMeta {
        note: String::from_str(env, ""),
        priority: 0,
    };
    for id in 1..=last_id {
        if env.storage().persistent().has(&invoice_key(id))
            && !env.storage().persistent().has(&invoice_meta_key_v2(id))
        {
            env.storage()
                .persistent()
                .set(&invoice_meta_key_v2(id), &default_meta);
        }
    }
}

// ---------------------------------------------------------------------------
// v2 -> v3: rename the InvoiceMeta storage key.
// ---------------------------------------------------------------------------
// The `inv_meta` key introduced at v2 is renamed to `inv_note` to match the
// public `get_invoice_note` entry point. This demonstrates a pure key rename:
// read under the old key, write under the new one, drop the old entry.

pub(crate) fn invoice_meta_key_v3(id: u64) -> (Symbol, u64) {
    (symbol_short!("inv_note"), id)
}

fn migration_v3(env: &Env) {
    let last_id: u64 = env.storage().persistent().get(&counter_key()).unwrap_or(0);
    for id in 1..=last_id {
        let old_key = invoice_meta_key_v2(id);
        if let Some(meta) = env.storage().persistent().get::<_, InvoiceMeta>(&old_key) {
            env.storage()
                .persistent()
                .set(&invoice_meta_key_v3(id), &meta);
            env.storage().persistent().remove(&old_key);
        }
    }
}

/// Look up an invoice's note under its current (post-v3) storage key.
pub fn get_invoice_meta(env: &Env, id: u64) -> Option<InvoiceMeta> {
    env.storage().persistent().get(&invoice_meta_key_v3(id))
}

/// Run every migration between the stored schema version and
/// [`CURRENT_SCHEMA_VERSION`], in order, bumping the stored version after
/// each step so a partially-upgraded contract resumes from wherever it left
/// off if `migrate` is called again (e.g. after hitting the instruction
/// budget on a contract with many invoices).
pub fn run_pending_migrations(env: &Env) -> u32 {
    let mut version = schema_version(env);
    if version < 2 {
        migration_v2(env);
        version = 2;
        set_schema_version(env, version);
    }
    if version < 3 {
        migration_v3(env);
        version = 3;
        set_schema_version(env, version);
    }
    version
}
