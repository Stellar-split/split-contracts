//! Payment velocity limits (issue #912).
//!
//! Prevents a single payer from flooding the contract with rapid payments.
//! Unlike the per-invoice `velocity_limit` and the global amount cap, these
//! limits are contract-wide and track **both** payment count and volume per
//! payer over a fixed window, plus an optional minimum spacing between
//! payments.
//!
//! - **Configuration** — the admin sets a default [`VelocityConfig`] and may
//!   override it per payer. A field set to `0` disables that particular
//!   limit; a config with every limit at `0` disables enforcement entirely.
//! - **State** — a [`VelocityState`] is stored per payer and reset whenever
//!   the current window has elapsed.
//! - **Enforcement** — [`enforce`] is called from the payment path; it is a
//!   no-op when no limits are configured, so existing behaviour is unchanged.

use soroban_sdk::{contracttype, symbol_short, Address, Env, Symbol};

use crate::error::ContractError;
use crate::events;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Velocity limits applied to a payer. `0` disables the individual limit.
#[contracttype]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VelocityConfig {
    /// Maximum number of payments allowed per window.
    pub max_payments: u32,
    /// Maximum total amount allowed per window.
    pub max_amount: i128,
    /// Window length in seconds. Required when either window limit is set.
    pub window_secs: u64,
    /// Minimum number of seconds between two payments from the same payer.
    pub cooldown_secs: u64,
}

impl VelocityConfig {
    /// `true` when at least one limit is active.
    pub fn is_enabled(&self) -> bool {
        self.max_payments > 0 || self.max_amount > 0 || self.cooldown_secs > 0
    }
}

/// Rolling velocity state for one payer.
#[contracttype]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VelocityState {
    /// Timestamp the current window started.
    pub window_start: u64,
    /// Payments counted in the current window.
    pub payment_count: u32,
    /// Amount paid in the current window.
    pub amount_total: i128,
    /// Timestamp of the most recent accepted payment (0 if none).
    pub last_payment_at: u64,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

fn default_config_key() -> Symbol {
    symbol_short!("pv_cfg")
}

fn payer_override_key(payer: &Address) -> (Symbol, Address) {
    (symbol_short!("pv_ovr"), payer.clone())
}

fn payer_state_key(payer: &Address) -> (Symbol, Address) {
    (symbol_short!("pv_st"), payer.clone())
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Reject configurations that cannot be enforced meaningfully.
pub fn validate_config(env: &Env, config: &VelocityConfig) {
    if config.max_amount < 0 {
        env.panic_with_error(ContractError::InvalidAmount);
    }
    // A window limit without a window would never reset.
    if (config.max_payments > 0 || config.max_amount > 0) && config.window_secs == 0 {
        env.panic_with_error(ContractError::InvalidAmount);
    }
}

pub fn get_default_config(env: &Env) -> VelocityConfig {
    env.storage()
        .instance()
        .get(&default_config_key())
        .unwrap_or_default()
}

pub fn set_default_config(env: &Env, config: &VelocityConfig) {
    validate_config(env, config);
    env.storage().instance().set(&default_config_key(), config);
    events::velocity_config_set(env, None, config.max_payments, config.max_amount, config.window_secs);
}

pub fn get_payer_override(env: &Env, payer: &Address) -> Option<VelocityConfig> {
    env.storage().persistent().get(&payer_override_key(payer))
}

/// Set (`Some`) or clear (`None`) a per-payer override.
pub fn set_payer_override(env: &Env, payer: &Address, config: &Option<VelocityConfig>) {
    match config {
        Some(c) => {
            validate_config(env, c);
            env.storage().persistent().set(&payer_override_key(payer), c);
            events::velocity_config_set(
                env,
                Some(payer.clone()),
                c.max_payments,
                c.max_amount,
                c.window_secs,
            );
        }
        None => {
            env.storage().persistent().remove(&payer_override_key(payer));
            events::velocity_config_set(env, Some(payer.clone()), 0, 0, 0);
        }
    }
}

/// The config that applies to `payer`: the override if present, else the default.
pub fn effective_config(env: &Env, payer: &Address) -> VelocityConfig {
    get_payer_override(env, payer).unwrap_or_else(|| get_default_config(env))
}

// ---------------------------------------------------------------------------
// State + checks
// ---------------------------------------------------------------------------

pub fn get_state(env: &Env, payer: &Address) -> VelocityState {
    env.storage()
        .persistent()
        .get(&payer_state_key(payer))
        .unwrap_or_default()
}

/// Pure check: compute the state that would result from a payment of
/// `amount` at time `now`, or the error that blocks it.
pub fn check(
    config: &VelocityConfig,
    state: &VelocityState,
    amount: i128,
    now: u64,
) -> Result<VelocityState, ContractError> {
    if amount <= 0 {
        return Err(ContractError::InvalidAmount);
    }
    let mut next = state.clone();

    // Cooldown between consecutive payments.
    if config.cooldown_secs > 0 && state.last_payment_at > 0 {
        let earliest = state.last_payment_at.saturating_add(config.cooldown_secs);
        if now < earliest {
            return Err(ContractError::RateLimitExceeded);
        }
    }

    // Reset the window when it has elapsed (or was never started).
    let window_end = state.window_start.saturating_add(config.window_secs);
    if state.payment_count == 0 || now >= window_end {
        next.window_start = now;
        next.payment_count = 0;
        next.amount_total = 0;
    }

    if config.max_payments > 0 && next.payment_count >= config.max_payments {
        return Err(ContractError::RateLimitExceeded);
    }
    let new_total = next
        .amount_total
        .checked_add(amount)
        .ok_or(ContractError::ArithmeticOverflow)?;
    if config.max_amount > 0 && new_total > config.max_amount {
        return Err(ContractError::RateLimitExceeded);
    }

    next.payment_count = next.payment_count.saturating_add(1);
    next.amount_total = new_total;
    next.last_payment_at = now;
    Ok(next)
}

/// Read-only check used by `check_payer_velocity`.
pub fn would_allow(env: &Env, payer: &Address, amount: i128) -> bool {
    let config = effective_config(env, payer);
    if !config.is_enabled() {
        return amount > 0;
    }
    check(&config, &get_state(env, payer), amount, env.ledger().timestamp()).is_ok()
}

/// Enforce velocity limits for a payment and persist the updated state.
/// No-op when no limits are configured for `payer`.
pub fn enforce(env: &Env, payer: &Address, amount: i128) {
    let config = effective_config(env, payer);
    if !config.is_enabled() {
        return;
    }
    let state = get_state(env, payer);
    match check(&config, &state, amount, env.ledger().timestamp()) {
        Ok(next) => {
            env.storage().persistent().set(&payer_state_key(payer), &next);
        }
        Err(e) => env.panic_with_error(e),
    }
}

/// Clear a payer's velocity state (admin recovery tool).
pub fn reset_state(env: &Env, payer: &Address) {
    env.storage().persistent().remove(&payer_state_key(payer));
}
