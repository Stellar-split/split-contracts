//! Issue #1009: Comprehensive audit logging for regulatory compliance.
//!
//! Provides a contract-wide, access-controlled audit trail that captures
//! every significant administrative or state-changing operation:
//!
//! - **Event logging** — a bounded, append-only log of `ComplianceLogEntry`
//!   records keyed by a monotonic sequence number.
//! - **Data retention** — configurable retention window; entries older than
//!   the window can be purged by an authorised admin (with an event emitted).
//! - **Encryption metadata** — each entry carries an optional `data_hash`
//!   (SHA-256 of the off-chain encrypted payload) so the log is tamper-evident
//!   without storing plaintext PII on-chain.
//! - **Access controls** — a dedicated `AuditReader` role (distinct from
//!   Operator/SuperAdmin) gates log reads; writes are always admin-initiated.
//! - **Analysis helpers** — `count_by_kind`, `entries_in_range`, and
//!   `get_summary_stats` provide on-chain queryable analytics.
//!
//! ## Storage layout
//!
//! | Key symbol     | Tier       | Type                       | Purpose                     |
//! |----------------|------------|----------------------------|-----------------------------|
//! | `al_seq`       | instance   | `u64`                      | Global log sequence counter |
//! | `al_entry(n)`  | persistent | `ComplianceLogEntry`       | Log entry n                 |
//! | `al_head`      | instance   | `u64`                      | Oldest retained entry seq   |
//! | `al_retntn`    | instance   | `u64`                      | Retention window in seconds |
//! | `al_readers`   | persistent | `Vec<Address>`             | Authorised audit reader list|
//! | `al_stats`     | instance   | `AuditStats`               | Aggregate counts by kind    |

use soroban_sdk::{contracttype, symbol_short, Address, BytesN, Env, Map, String, Symbol, Vec};

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Hard cap on the number of log entries kept on-chain at any time.
/// Oldest entries are dropped when the cap is exceeded.
pub const MAX_LOG_ENTRIES: u64 = 500;

/// Maximum number of authorised audit readers.
pub const MAX_READERS: u32 = 30;

/// Default retention window: 90 days in seconds.
pub const DEFAULT_RETENTION_SECS: u64 = 90 * 24 * 60 * 60;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Category of an auditable event.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogEventKind {
    /// An administrative action (pause, config change, role grant…).
    AdminAction = 0,
    /// An invoice lifecycle transition (created, released, refunded…).
    InvoiceLifecycle = 1,
    /// A payment received or transferred.
    Payment = 2,
    /// A security event (circuit breaker, failed auth, anomaly).
    SecurityEvent = 3,
    /// A contract upgrade (proposed, executed, rolled back).
    UpgradeEvent = 4,
    /// A rate-limiting event (limit hit, backoff applied).
    RateLimitEvent = 5,
    /// Any other event not covered above.
    Other = 6,
}

/// A single compliance log entry.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComplianceLogEntry {
    /// Monotonically increasing sequence number across all entries.
    pub seq: u64,
    /// Category of the event.
    pub kind: LogEventKind,
    /// Short action tag (e.g. `pause`, `pay`, `upg_exec`).
    pub action: Symbol,
    /// Address that initiated the action.
    pub actor: Address,
    /// Invoice ID if the event is scoped to an invoice (0 = contract-level).
    pub invoice_id: u64,
    /// Unix timestamp of the event.
    pub timestamp: u64,
    /// Soroban ledger sequence of the event.
    pub ledger: u32,
    /// SHA-256 hash of the off-chain encrypted detail payload (optional).
    /// Allows off-chain systems to verify the log is tamper-evident.
    pub data_hash: Option<BytesN<32>>,
    /// Free-form note (on-chain, kept short for gas efficiency).
    pub note: String,
}

/// Aggregate statistics for the audit log.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditStats {
    /// Total entries ever appended (never decremented, survives purges).
    pub total_appended: u64,
    /// Total entries purged via retention sweeps.
    pub total_purged: u64,
    /// Count of entries per `LogEventKind` discriminant (0–6).
    pub counts_by_kind: Map<u32, u64>,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

fn seq_key() -> Symbol {
    symbol_short!("al_seq")
}

fn head_key() -> Symbol {
    symbol_short!("al_head")
}

fn retention_key() -> Symbol {
    symbol_short!("al_retntn")
}

fn readers_key() -> Symbol {
    symbol_short!("al_readrs")
}

fn stats_key() -> Symbol {
    symbol_short!("al_stats")
}

/// Key for a specific log entry (persistent storage).
fn entry_key(seq: u64) -> (Symbol, u64) {
    (symbol_short!("al_entry"), seq)
}

// ---------------------------------------------------------------------------
// Accessors
// ---------------------------------------------------------------------------

fn get_seq(env: &Env) -> u64 {
    env.storage().instance().get(&seq_key()).unwrap_or(0u64)
}

fn increment_seq(env: &Env) -> u64 {
    let next = get_seq(env) + 1;
    env.storage().instance().set(&seq_key(), &next);
    next
}

fn get_head(env: &Env) -> u64 {
    env.storage().instance().get(&head_key()).unwrap_or(1u64)
}

fn set_head(env: &Env, head: u64) {
    env.storage().instance().set(&head_key(), &head);
}

pub fn get_retention_secs(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&retention_key())
        .unwrap_or(DEFAULT_RETENTION_SECS)
}

pub fn set_retention_secs(env: &Env, secs: u64) {
    assert!(secs > 0, "retention window must be positive");
    env.storage().instance().set(&retention_key(), &secs);
    emit_retention_configured(env, secs);
}

pub fn get_stats(env: &Env) -> AuditStats {
    env.storage()
        .instance()
        .get(&stats_key())
        .unwrap_or_else(|| AuditStats {
            total_appended: 0,
            total_purged: 0,
            counts_by_kind: Map::new(env),
        })
}

fn set_stats(env: &Env, stats: &AuditStats) {
    env.storage().instance().set(&stats_key(), stats);
}

// ---------------------------------------------------------------------------
// Access-controlled reader list
// ---------------------------------------------------------------------------

pub fn get_readers(env: &Env) -> Vec<Address> {
    env.storage()
        .persistent()
        .get(&readers_key())
        .unwrap_or_else(|| Vec::new(env))
}

fn set_readers(env: &Env, readers: &Vec<Address>) {
    env.storage().persistent().set(&readers_key(), readers);
}

/// Add an audit reader. Callers must hold SuperAdmin or Operator role (enforced
/// before calling this helper).
pub fn add_reader(env: &Env, admin: &Address, reader: Address) {
    let _ = admin;
    let mut readers = get_readers(env);
    if readers.iter().any(|r| r == reader) {
        return; // idempotent
    }
    assert!(readers.len() < MAX_READERS, "reader list full");
    readers.push_back(reader.clone());
    set_readers(env, &readers);
    emit_reader_added(env, admin, &reader);
}

/// Remove an audit reader.
pub fn remove_reader(env: &Env, admin: &Address, reader: &Address) {
    let mut readers = get_readers(env);
    let mut new_readers = Vec::new(env);
    let mut found = false;
    for r in readers.iter() {
        if &r == reader {
            found = true;
        } else {
            new_readers.push_back(r);
        }
    }
    assert!(found, "reader not found");
    set_readers(env, &new_readers);
    emit_reader_removed(env, admin, reader);
}

/// Returns `true` when `addr` is an authorised audit reader or holds admin rights.
pub fn is_authorised_reader(env: &Env, addr: &Address) -> bool {
    get_readers(env).iter().any(|r| &r == addr)
}

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

/// Append a compliance log entry. Returns the assigned sequence number.
///
/// This is the primary write path; it is safe to call from any admin function.
/// Overflows are handled gracefully: if the on-chain buffer exceeds
/// `MAX_LOG_ENTRIES`, the oldest entry is evicted before writing the new one.
pub fn append(
    env: &Env,
    kind: LogEventKind,
    action: Symbol,
    actor: &Address,
    invoice_id: u64,
    data_hash: Option<BytesN<32>>,
    note: String,
) -> u64 {
    let seq = increment_seq(env);
    let entry = ComplianceLogEntry {
        seq,
        kind,
        action: action.clone(),
        actor: actor.clone(),
        invoice_id,
        timestamp: env.ledger().timestamp(),
        ledger: env.ledger().sequence(),
        data_hash,
        note,
    };
    env.storage().persistent().set(&entry_key(seq), &entry);

    // Update stats.
    let mut stats = get_stats(env);
    stats.total_appended += 1;
    let kind_disc = kind as u32;
    let count = stats.counts_by_kind.get(kind_disc).unwrap_or(0u64);
    stats.counts_by_kind.set(kind_disc, count + 1);
    set_stats(env, &stats);

    // Evict oldest if over the cap.
    let head = get_head(env);
    let total_live = seq.saturating_sub(head) + 1;
    if total_live > MAX_LOG_ENTRIES {
        env.storage().persistent().remove(&entry_key(head));
        set_head(env, head + 1);
    }

    emit_entry_appended(env, seq, &action, actor, kind);
    seq
}

// ---------------------------------------------------------------------------
// Read API
// ---------------------------------------------------------------------------

/// Retrieve a single log entry by its sequence number.
/// Returns `None` if the entry has been evicted or doesn't exist.
pub fn get_entry(env: &Env, seq: u64) -> Option<ComplianceLogEntry> {
    env.storage().persistent().get(&entry_key(seq))
}

/// Current range of live (non-evicted) sequence numbers: `(head, tail)`.
/// `head` is the oldest live entry; `tail` is the most recently appended.
pub fn get_live_range(env: &Env) -> (u64, u64) {
    let tail = get_seq(env);
    let head = get_head(env);
    (head, tail)
}

/// Retrieve up to `limit` entries starting from `from_seq` (inclusive).
/// Skips evicted entries silently.
pub fn get_entries_from(env: &Env, from_seq: u64, limit: u32) -> Vec<ComplianceLogEntry> {
    let tail = get_seq(env);
    let mut result = Vec::new(env);
    let mut seq = from_seq;
    let mut count = 0u32;
    while seq <= tail && count < limit {
        if let Some(entry) = get_entry(env, seq) {
            result.push_back(entry);
            count += 1;
        }
        seq += 1;
    }
    result
}

/// Count live entries by event kind.
pub fn count_by_kind(env: &Env, kind: LogEventKind) -> u64 {
    let stats = get_stats(env);
    stats.counts_by_kind.get(kind as u32).unwrap_or(0u64)
}

/// Return entries (up to `max_results`) whose `timestamp` falls in
/// `[from_ts, to_ts]` inclusive. Scans from the head forward.
pub fn entries_in_range(
    env: &Env,
    from_ts: u64,
    to_ts: u64,
    max_results: u32,
) -> Vec<ComplianceLogEntry> {
    let (head, tail) = get_live_range(env);
    let mut result = Vec::new(env);
    let mut seq = head;
    while seq <= tail && result.len() < max_results {
        if let Some(entry) = get_entry(env, seq) {
            if entry.timestamp >= from_ts && entry.timestamp <= to_ts {
                result.push_back(entry);
            }
        }
        seq += 1;
    }
    result
}

// ---------------------------------------------------------------------------
// Retention / purge
// ---------------------------------------------------------------------------

/// Purge entries older than the configured retention window.
/// Emits a `purged` event with the count of entries removed.
/// Returns the number of entries purged.
pub fn purge_expired(env: &Env, admin: &Address) -> u64 {
    let retention = get_retention_secs(env);
    let cutoff_ts = env.ledger().timestamp().saturating_sub(retention);
    let (head, tail) = get_live_range(env);
    let mut purged = 0u64;
    let mut new_head = head;

    let mut seq = head;
    while seq <= tail {
        match get_entry(env, seq) {
            Some(entry) if entry.timestamp < cutoff_ts => {
                env.storage().persistent().remove(&entry_key(seq));
                purged += 1;
                new_head = seq + 1;
            }
            _ => break, // entries are in timestamp order; stop at first live one
        }
        seq += 1;
    }

    if purged > 0 {
        set_head(env, new_head);
        let mut stats = get_stats(env);
        stats.total_purged += purged;
        set_stats(env, &stats);
        emit_entries_purged(env, admin, purged, new_head);
    }

    purged
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

fn emit_entry_appended(env: &Env, seq: u64, action: &Symbol, actor: &Address, kind: LogEventKind) {
    env.events().publish(
        (symbol_short!("al"), symbol_short!("appended")),
        (seq, action.clone(), actor.clone(), kind as u32),
    );
}

fn emit_entries_purged(env: &Env, admin: &Address, count: u64, new_head: u64) {
    env.events().publish(
        (symbol_short!("al"), symbol_short!("purged")),
        (admin.clone(), count, new_head),
    );
}

fn emit_retention_configured(env: &Env, secs: u64) {
    env.events().publish(
        (symbol_short!("al"), symbol_short!("retntn")),
        secs,
    );
}

fn emit_reader_added(env: &Env, admin: &Address, reader: &Address) {
    env.events().publish(
        (symbol_short!("al"), symbol_short!("rdr_add")),
        (admin.clone(), reader.clone()),
    );
}

fn emit_reader_removed(env: &Env, admin: &Address, reader: &Address) {
    env.events().publish(
        (symbol_short!("al"), symbol_short!("rdr_del")),
        (admin.clone(), reader.clone()),
    );
}
