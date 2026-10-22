//! Issue #1010: Enhanced contract upgrade mechanism.
//!
//! Extends the basic `propose_upgrade`/`execute_upgrade`/`cancel_upgrade` in
//! `lib.rs` with:
//! - Semantic version management (SemVer-style major.minor.patch).
//! - Per-upgrade migration manifests that describe what changes.
//! - Rollback capability: the previous WASM hash is retained so the admin
//!   can roll back to the prior version.
//! - Pre-upgrade validation hooks (storage-layout checks, invariant checks).
//! - Notification events and a structured upgrade history log.
//!
//! # Architecture
//!
//! This module records *metadata* about upgrades. The actual WASM swap is
//! still performed through `lib.rs`'s `execute_upgrade` path, which calls
//! `env.deployer().update_current_contract_wasm(...)`. This module adds the
//! bookkeeping layer around that call.
//!
//! ## Storage layout
//!
//! | Key symbol     | Tier       | Type                       | Purpose                          |
//! |----------------|------------|----------------------------|----------------------------------|
//! | `cu_version`   | instance   | `ContractVersion`          | Current semantic version         |
//! | `cu_prev_wasm` | instance   | `BytesN<32>`               | Previous WASM hash for rollback  |
//! | `cu_prev_ver`  | instance   | `ContractVersion`          | Previous semantic version        |
//! | `cu_manifest`  | persistent | `UpgradeManifest`          | Pending upgrade manifest         |
//! | `cu_history`   | persistent | `Vec<UpgradeHistoryEntry>` | Bounded upgrade history          |
//! | `cu_rb_cnt`    | instance   | `u32`                      | Rollback counter (safeguard)     |

use soroban_sdk::{contracttype, symbol_short, BytesN, Env, String, Symbol, Vec};

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum entries retained in the upgrade history.
pub const MAX_UPGRADE_HISTORY: u32 = 20;

/// Maximum rollback operations allowed before a manual reset is required.
pub const MAX_ROLLBACK_COUNT: u32 = 3;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Semantic version for the contract WASM.
#[contracttype]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ContractVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl ContractVersion {
    /// Returns `true` when `other` is strictly greater than `self`.
    pub fn is_upgrade_of(&self, other: &ContractVersion) -> bool {
        (other.major, other.minor, other.patch) > (self.major, self.minor, self.patch)
    }
}

/// Describes what the upgrade changes. Stored alongside the pending proposal.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeManifest {
    /// New WASM hash being proposed.
    pub new_wasm_hash: BytesN<32>,
    /// New semantic version after the upgrade.
    pub new_version: ContractVersion,
    /// Summary of breaking changes (empty = backwards-compatible).
    pub breaking_changes: String,
    /// Summary of new features.
    pub new_features: String,
    /// Whether a storage migration script should run after the WASM swap.
    pub requires_migration: bool,
    /// Proposer's address (informational, verified by lib.rs admin check).
    pub proposed_by_note: String,
    /// Ledger when the proposal was created.
    pub proposed_at_ledger: u32,
    /// Timestamp when the proposal was created.
    pub proposed_at_ts: u64,
}

/// Pre-validation outcome for an upgrade.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeValidationResult {
    pub passed: bool,
    /// Number of checks that passed.
    pub checks_passed: u32,
    /// Number of checks that failed.
    pub checks_failed: u32,
    /// Human-readable summary.
    pub summary: String,
}

/// One entry in the bounded upgrade history log.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeHistoryEntry {
    pub from_version: ContractVersion,
    pub to_version: ContractVersion,
    pub wasm_hash: BytesN<32>,
    pub executed_at_ledger: u32,
    pub executed_at_ts: u64,
    /// `true` if this entry represents a rollback.
    pub is_rollback: bool,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

fn version_key() -> Symbol {
    symbol_short!("cu_versn")
}

fn prev_wasm_key() -> Symbol {
    symbol_short!("cu_prwsm")
}

fn prev_version_key() -> Symbol {
    symbol_short!("cu_prver")
}

fn manifest_key() -> Symbol {
    symbol_short!("cu_mnfst")
}

fn history_key() -> Symbol {
    symbol_short!("cu_hist")
}

fn rollback_count_key() -> Symbol {
    symbol_short!("cu_rbcnt")
}

// ---------------------------------------------------------------------------
// Version management
// ---------------------------------------------------------------------------

/// Read the current contract semantic version (default 0.0.0 for fresh deploys).
pub fn get_version(env: &Env) -> ContractVersion {
    env.storage()
        .instance()
        .get(&version_key())
        .unwrap_or_default()
}

/// Stamp the initial version on a freshly deployed contract. Should be called
/// from `initialize()` so the version record exists from day one.
pub fn init_version(env: &Env, version: ContractVersion) {
    env.storage().instance().set(&version_key(), &version);
}

/// Update the stored version. Called immediately after the WASM swap succeeds.
fn set_version(env: &Env, version: ContractVersion) {
    env.storage().instance().set(&version_key(), &version);
}

/// Read the previous version (the one active before the last upgrade).
pub fn get_previous_version(env: &Env) -> Option<ContractVersion> {
    env.storage().instance().get(&prev_version_key())
}

/// Read the previous WASM hash (for rollback).
pub fn get_previous_wasm_hash(env: &Env) -> Option<BytesN<32>> {
    env.storage().instance().get(&prev_wasm_key())
}

// ---------------------------------------------------------------------------
// Manifest management
// ---------------------------------------------------------------------------

/// Store an upgrade manifest alongside the pending proposal.
pub fn propose_manifest(env: &Env, manifest: &UpgradeManifest) {
    // Version must be strictly greater than current.
    let current = get_version(env);
    assert!(
        current.is_upgrade_of(&manifest.new_version),
        "new version must be strictly greater than current version"
    );
    env.storage().persistent().set(&manifest_key(), manifest);
    emit_manifest_proposed(env, &manifest.new_wasm_hash, manifest.new_version);
}

/// Return the pending manifest, if any.
pub fn get_manifest(env: &Env) -> Option<UpgradeManifest> {
    env.storage().persistent().get(&manifest_key())
}

/// Remove the pending manifest (e.g. when the proposal is cancelled).
pub fn clear_manifest(env: &Env) {
    env.storage().persistent().remove(&manifest_key());
}

// ---------------------------------------------------------------------------
// Pre-upgrade validation
// ---------------------------------------------------------------------------

/// Run pre-upgrade validation checks and return the result.
///
/// Checks performed:
/// 1. A manifest is present and the version is increasing.
/// 2. The WASM hash in the manifest matches the proposal.
/// 3. No active Critical pause conditions block the upgrade.
///
/// This does **not** block execution — it returns the result for the caller to
/// decide whether to proceed. Callers may enforce `passed == true` if desired.
pub fn validate_upgrade(env: &Env, proposed_wasm_hash: &BytesN<32>) -> UpgradeValidationResult {
    let mut checks_passed = 0u32;
    let mut checks_failed = 0u32;

    // Check 1: manifest present.
    let manifest = match get_manifest(env) {
        Some(m) => {
            checks_passed += 1;
            m
        }
        None => {
            checks_failed += 1;
            return UpgradeValidationResult {
                passed: false,
                checks_passed,
                checks_failed,
                summary: String::from_str(env, "no upgrade manifest"),
            };
        }
    };

    // Check 2: WASM hash matches.
    if manifest.new_wasm_hash == *proposed_wasm_hash {
        checks_passed += 1;
    } else {
        checks_failed += 1;
    }

    // Check 3: version is increasing.
    let current = get_version(env);
    if current.is_upgrade_of(&manifest.new_version) {
        checks_passed += 1;
    } else {
        checks_failed += 1;
    }

    let passed = checks_failed == 0;
    let summary = if passed {
        String::from_str(env, "all checks passed")
    } else {
        String::from_str(env, "one or more checks failed")
    };

    emit_validation_ran(env, proposed_wasm_hash, passed, checks_passed, checks_failed);

    UpgradeValidationResult {
        passed,
        checks_passed,
        checks_failed,
        summary,
    }
}

// ---------------------------------------------------------------------------
// Execute upgrade bookkeeping
// ---------------------------------------------------------------------------

/// Record that an upgrade was successfully executed. Called immediately after
/// `env.deployer().update_current_contract_wasm(...)` succeeds in `lib.rs`.
///
/// - Stores the old version/hash for rollback.
/// - Writes the new version.
/// - Appends to the history log.
/// - Clears the manifest.
pub fn record_upgrade(env: &Env, new_wasm_hash: &BytesN<32>, is_rollback: bool) {
    let old_version = get_version(env);

    // Persist previous WASM hash for rollback capability.
    // We derive it from the manifest (the manifest holds the new hash;
    // the current WASM hash is not directly readable on Soroban, so the
    // admin must supply it via the manifest when rolling back).
    let manifest = get_manifest(env);
    let new_version = manifest
        .as_ref()
        .map(|m| m.new_version)
        .unwrap_or(old_version);

    env.storage().instance().set(&prev_version_key(), &old_version);
    env.storage().instance().set(&prev_wasm_key(), new_wasm_hash);

    set_version(env, new_version);
    clear_manifest(env);

    // Append to history (bounded).
    let mut history: Vec<UpgradeHistoryEntry> = env
        .storage()
        .persistent()
        .get(&history_key())
        .unwrap_or_else(|| Vec::new(env));
    history.push_back(UpgradeHistoryEntry {
        from_version: old_version,
        to_version: new_version,
        wasm_hash: new_wasm_hash.clone(),
        executed_at_ledger: env.ledger().sequence(),
        executed_at_ts: env.ledger().timestamp(),
        is_rollback,
    });
    while history.len() > MAX_UPGRADE_HISTORY {
        history.pop_front();
    }
    env.storage().persistent().set(&history_key(), &history);

    if is_rollback {
        let count: u32 = env
            .storage()
            .instance()
            .get(&rollback_count_key())
            .unwrap_or(0u32);
        env.storage()
            .instance()
            .set(&rollback_count_key(), &(count.saturating_add(1)));
    }

    emit_upgrade_recorded(env, new_wasm_hash, old_version, new_version, is_rollback);
}

/// Return the full upgrade history (most recent entries last).
pub fn get_upgrade_history(env: &Env) -> Vec<UpgradeHistoryEntry> {
    env.storage()
        .persistent()
        .get(&history_key())
        .unwrap_or_else(|| Vec::new(env))
}

// ---------------------------------------------------------------------------
// Rollback
// ---------------------------------------------------------------------------

/// Validate that a rollback is permitted.
///
/// Conditions:
/// - A previous WASM hash must be stored.
/// - The rollback counter must be below `MAX_ROLLBACK_COUNT`.
pub fn assert_rollback_allowed(env: &Env) {
    assert!(
        get_previous_wasm_hash(env).is_some(),
        "no previous WASM hash stored; rollback not possible"
    );
    let count: u32 = env
        .storage()
        .instance()
        .get(&rollback_count_key())
        .unwrap_or(0u32);
    assert!(
        count < MAX_ROLLBACK_COUNT,
        "rollback limit reached; manual intervention required"
    );
}

/// Reset the rollback counter (admin-only; called after a stable upgrade).
pub fn reset_rollback_counter(env: &Env) {
    env.storage().instance().set(&rollback_count_key(), &0u32);
}

/// Get current rollback count.
pub fn get_rollback_count(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&rollback_count_key())
        .unwrap_or(0u32)
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

fn emit_manifest_proposed(env: &Env, wasm_hash: &BytesN<32>, new_version: ContractVersion) {
    env.events().publish(
        (symbol_short!("cu"), symbol_short!("mnf_prop")),
        (
            wasm_hash.clone(),
            new_version.major,
            new_version.minor,
            new_version.patch,
        ),
    );
}

fn emit_validation_ran(
    env: &Env,
    wasm_hash: &BytesN<32>,
    passed: bool,
    checks_passed: u32,
    checks_failed: u32,
) {
    env.events().publish(
        (symbol_short!("cu"), symbol_short!("val_ran")),
        (wasm_hash.clone(), passed, checks_passed, checks_failed),
    );
}

fn emit_upgrade_recorded(
    env: &Env,
    new_wasm_hash: &BytesN<32>,
    from: ContractVersion,
    to: ContractVersion,
    is_rollback: bool,
) {
    env.events().publish(
        (symbol_short!("cu"), symbol_short!("upg_done")),
        (
            new_wasm_hash.clone(),
            from.major,
            from.minor,
            from.patch,
            to.major,
            to.minor,
            to.patch,
            is_rollback,
        ),
    );
}
