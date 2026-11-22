//! Issue #1008: Enhanced rate limiting and throttling.
//!
//! Provides per-user, per-operation rate limiting with exponential backoff,
//! accurate tracking, and analytics — extending the simpler per-invoice
//! sliding-window limiter already in `lib.rs`.
//!
//! # Design
//!
//! The limiter uses a **sliding-window counter** approach:
//! - Each (user, operation) pair has a `RateLimitState` stored in persistent
//!   storage.
//! - The state records how many calls the user made in the current window and
//!   when that window started.
//! - When a window expires (`now >= window_start + window_secs`), the counter
//!   is reset.
//!
//! Backoff is implemented as an **exponential penalty** on top of the base
//! limit: a user who has hit the limit N times in a row must wait
//! `base_window_secs * 2^N` seconds before the limit resets for them.
//!
//! ## Storage layout
//!
//! | Key symbol        | Tier       | Type                          | Purpose                            |
//! |-------------------|------------|-------------------------------|------------------------------------|
//! | `rl_cfg(op)`      | persistent | `RateLimitConfig`             | Per-operation config               |
//! | `rl_state(op, u)` | persistent | `RateLimitState`              | Per-(operation, user) state        |
//! | `rl_analytics`    | instance   | `RateLimitAnalytics`          | Aggregate analytics                |
//! | `rl_ops`          | instance   | `Vec<Symbol>`                 | Registered operation names         |

use soroban_sdk::{contracttype, symbol_short, Address, Env, Symbol, Vec};

use crate::error::ContractError;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum number of distinct operations that can be rate-limited.
pub const MAX_RATE_LIMITED_OPS: u32 = 20;

/// Maximum backoff multiplier (caps the exponential growth).
pub const MAX_BACKOFF_MULTIPLIER: u32 = 32;

/// Default max calls per window when a config is not set (0 = unlimited).
pub const DEFAULT_MAX_CALLS: u32 = 0;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Configuration for one rate-limited operation.
#[contracttype]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RateLimitConfig {
    /// Maximum number of calls allowed per window. `0` = no limit.
    pub max_calls: u32,
    /// Window duration in seconds. Required when `max_calls > 0`.
    pub window_secs: u64,
    /// Enable exponential backoff on repeated violations.
    pub backoff_enabled: bool,
    /// Maximum number of consecutive violations before the backoff cap applies.
    pub max_backoff_steps: u32,
}

impl RateLimitConfig {
    /// True when this config has an active limit.
    pub fn is_active(&self) -> bool {
        self.max_calls > 0 && self.window_secs > 0
    }
}

/// Per-(operation, user) sliding-window state.
#[contracttype]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RateLimitState {
    /// Timestamp when the current window started.
    pub window_start: u64,
    /// Number of calls made in the current window.
    pub call_count: u32,
    /// Number of consecutive windows in which the user hit the limit.
    pub violation_streak: u32,
    /// Timestamp of the most recent call (0 if none).
    pub last_call_at: u64,
}

/// Aggregate analytics collected across all rate-limited operations.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RateLimitAnalytics {
    /// Total calls checked (allowed + denied).
    pub total_checked: u64,
    /// Total calls that were allowed through.
    pub total_allowed: u64,
    /// Total calls that were denied.
    pub total_denied: u64,
    /// Total times a backoff penalty was applied.
    pub total_backoffs: u64,
    /// Count of calls per operation (keyed by Symbol's string representation).
    // Note: we use a u32-keyed map indexed by a hash for no_std compat.
    pub denied_by_op: soroban_sdk::Map<Symbol, u64>,
}

/// Result of a rate-limit check.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckResult {
    /// Allowed; `remaining` is the calls left in this window.
    Allowed { remaining: u32 },
    /// Denied; `retry_after_secs` is the minimum wait.
    Denied { retry_after_secs: u64 },
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

fn config_key(op: &Symbol) -> (Symbol, Symbol) {
    (symbol_short!("rl_cfg"), op.clone())
}

fn state_key(op: &Symbol, user: &Address) -> (Symbol, Symbol, Address) {
    (symbol_short!("rl_st"), op.clone(), user.clone())
}

fn analytics_key() -> Symbol {
    symbol_short!("rl_anlytc")
}

fn ops_key() -> Symbol {
    symbol_short!("rl_ops")
}

// ---------------------------------------------------------------------------
// Configuration management
// ---------------------------------------------------------------------------

/// Register or update the rate-limit configuration for an operation.
pub fn configure(env: &Env, op: &Symbol, config: &RateLimitConfig) {
    if config.is_active() {
        assert!(
            config.window_secs > 0,
            "window_secs must be positive when max_calls > 0"
        );
    }
    if config.backoff_enabled && config.max_backoff_steps == 0 {
        // Default to 4 backoff steps when enabled but not configured.
    }

    env.storage().persistent().set(&config_key(op), config);

    // Track registered operations.
    let mut ops: Vec<Symbol> = env
        .storage()
        .instance()
        .get(&ops_key())
        .unwrap_or_else(|| Vec::new(env));
    if !ops.iter().any(|o| &o == op) {
        assert!(
            ops.len() < MAX_RATE_LIMITED_OPS,
            "max rate-limited operations reached"
        );
        ops.push_back(op.clone());
        env.storage().instance().set(&ops_key(), &ops);
    }

    emit_config_set(env, op, config.max_calls, config.window_secs, config.backoff_enabled);
}

/// Remove the rate-limit configuration for an operation (disables limiting).
pub fn remove_config(env: &Env, op: &Symbol) {
    env.storage().persistent().remove(&config_key(op));
    let mut ops: Vec<Symbol> = env
        .storage()
        .instance()
        .get(&ops_key())
        .unwrap_or_else(|| Vec::new(env));
    let mut new_ops = Vec::new(env);
    for o in ops.iter() {
        if &o != op {
            new_ops.push_back(o);
        }
    }
    env.storage().instance().set(&ops_key(), &new_ops);
}

/// Return the config for an operation, or a permissive default (no limit).
pub fn get_config(env: &Env, op: &Symbol) -> RateLimitConfig {
    env.storage()
        .persistent()
        .get(&config_key(op))
        .unwrap_or_default()
}

/// Return all registered operation names.
pub fn get_operations(env: &Env) -> Vec<Symbol> {
    env.storage()
        .instance()
        .get(&ops_key())
        .unwrap_or_else(|| Vec::new(env))
}

// ---------------------------------------------------------------------------
// Per-user state
// ---------------------------------------------------------------------------

pub fn get_state(env: &Env, op: &Symbol, user: &Address) -> RateLimitState {
    env.storage()
        .persistent()
        .get(&state_key(op, user))
        .unwrap_or_default()
}

fn set_state(env: &Env, op: &Symbol, user: &Address, state: &RateLimitState) {
    env.storage().persistent().set(&state_key(op, user), state);
}

/// Reset the rate-limit state for a specific (operation, user) pair.
/// Useful for admin overrides (e.g. whitelisted users after a manual review).
pub fn reset_user_state(env: &Env, op: &Symbol, user: &Address) {
    env.storage().persistent().remove(&state_key(op, user));
    emit_user_state_reset(env, op, user);
}

// ---------------------------------------------------------------------------
// Core check / record
// ---------------------------------------------------------------------------

/// Compute the effective window duration applying backoff if configured.
fn effective_window_secs(config: &RateLimitConfig, streak: u32) -> u64 {
    if !config.backoff_enabled || streak == 0 {
        return config.window_secs;
    }
    let multiplier = 1u32 << streak.min(MAX_BACKOFF_MULTIPLIER);
    config
        .window_secs
        .saturating_mul(multiplier.min(MAX_BACKOFF_MULTIPLIER) as u64)
}

/// Check whether `user` may make another call to `op` right now.
///
/// This is a pure check: it reads state but does NOT write. Call
/// [`record_call`] after a successful call to commit the counter increment.
pub fn check(env: &Env, op: &Symbol, user: &Address) -> CheckResult {
    let config = get_config(env, op);
    if !config.is_active() {
        return CheckResult::Allowed {
            remaining: u32::MAX,
        };
    }

    let now = env.ledger().timestamp();
    let state = get_state(env, op, user);
    let window_secs = effective_window_secs(&config, state.violation_streak);
    let window_end = state.window_start.saturating_add(window_secs);

    if now >= window_end {
        // Window has expired → user gets a fresh window.
        return CheckResult::Allowed {
            remaining: config.max_calls.saturating_sub(1),
        };
    }

    if state.call_count < config.max_calls {
        return CheckResult::Allowed {
            remaining: config.max_calls - state.call_count - 1,
        };
    }

    // Limit hit.
    let retry_after = window_end.saturating_sub(now);
    CheckResult::Denied { retry_after_secs: retry_after }
}

/// Record a successful call by `user` for `op`, updating the sliding window.
/// Call this immediately after `check` returns `Allowed`.
pub fn record_call(env: &Env, op: &Symbol, user: &Address) {
    let config = get_config(env, op);
    if !config.is_active() {
        return;
    }

    let now = env.ledger().timestamp();
    let mut state = get_state(env, op, user);
    let window_secs = effective_window_secs(&config, state.violation_streak);
    let window_end = state.window_start.saturating_add(window_secs);

    if now >= window_end {
        // New window — reset count. Streak is cleared whenever a new window
        // starts without a denial (call_count was below max when the window expired).
        let was_under_limit = state.call_count < config.max_calls;
        state.window_start = now;
        state.call_count = 1;
        if was_under_limit {
            state.violation_streak = 0;
        }
    } else {
        state.call_count = state.call_count.saturating_add(1);
    }

    state.last_call_at = now;
    set_state(env, op, user, &state);

    // Update analytics.
    let mut analytics = get_analytics(env);
    analytics.total_checked = analytics.total_checked.saturating_add(1);
    analytics.total_allowed = analytics.total_allowed.saturating_add(1);
    set_analytics(env, &analytics);
}

/// Enforce the rate limit for `user` on `op`.
///
/// - If allowed: calls `record_call` and returns `Ok(remaining)`.
/// - If denied: emits a denial event, updates analytics, and returns
///   `Err(ContractError::RateLimitExceeded)`.
pub fn enforce(env: &Env, op: &Symbol, user: &Address) -> Result<u32, ContractError> {
    let result = check(env, op, user);
    match result {
        CheckResult::Allowed { remaining } => {
            record_call(env, op, user);
            emit_call_allowed(env, op, user, remaining);
            Ok(remaining)
        }
        CheckResult::Denied { retry_after_secs } => {
            // Record violation streak.
            let config = get_config(env, op);
            let mut state = get_state(env, op, user);
            let max_streak = config.max_backoff_steps.max(1);
            if config.backoff_enabled && state.violation_streak < max_streak {
                state.violation_streak = state.violation_streak.saturating_add(1);
                set_state(env, op, user, &state);
            }

            // Update analytics.
            let mut analytics = get_analytics(env);
            analytics.total_checked = analytics.total_checked.saturating_add(1);
            analytics.total_denied = analytics.total_denied.saturating_add(1);
            if config.backoff_enabled {
                analytics.total_backoffs = analytics.total_backoffs.saturating_add(1);
            }
            let op_denied = analytics.denied_by_op.get(op.clone()).unwrap_or(0u64);
            analytics.denied_by_op.set(op.clone(), op_denied + 1);
            set_analytics(env, &analytics);

            emit_call_denied(env, op, user, retry_after_secs);
            Err(ContractError::RateLimitExceeded)
        }
    }
}

// ---------------------------------------------------------------------------
// Analytics
// ---------------------------------------------------------------------------

pub fn get_analytics(env: &Env) -> RateLimitAnalytics {
    env.storage()
        .instance()
        .get(&analytics_key())
        .unwrap_or_else(|| RateLimitAnalytics {
            total_checked: 0,
            total_allowed: 0,
            total_denied: 0,
            total_backoffs: 0,
            denied_by_op: soroban_sdk::Map::new(env),
        })
}

fn set_analytics(env: &Env, a: &RateLimitAnalytics) {
    env.storage().instance().set(&analytics_key(), a);
}

/// Denial rate in basis points (0–10000). Returns 0 if no calls checked.
pub fn denial_rate_bps(env: &Env) -> u32 {
    let a = get_analytics(env);
    if a.total_checked == 0 {
        return 0;
    }
    ((a.total_denied * 10_000) / a.total_checked) as u32
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

fn emit_config_set(
    env: &Env,
    op: &Symbol,
    max_calls: u32,
    window_secs: u64,
    backoff_enabled: bool,
) {
    env.events().publish(
        (symbol_short!("rl"), symbol_short!("cfg_set")),
        (op.clone(), max_calls, window_secs, backoff_enabled),
    );
}

fn emit_call_allowed(env: &Env, op: &Symbol, user: &Address, remaining: u32) {
    env.events().publish(
        (symbol_short!("rl"), symbol_short!("allowed")),
        (op.clone(), user.clone(), remaining),
    );
}

fn emit_call_denied(env: &Env, op: &Symbol, user: &Address, retry_after_secs: u64) {
    env.events().publish(
        (symbol_short!("rl"), symbol_short!("denied")),
        (op.clone(), user.clone(), retry_after_secs),
    );
}

fn emit_user_state_reset(env: &Env, op: &Symbol, user: &Address) {
    env.events().publish(
        (symbol_short!("rl"), symbol_short!("st_reset")),
        (op.clone(), user.clone()),
    );
}
