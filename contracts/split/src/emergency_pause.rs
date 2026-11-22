//! Issue #1011: Enhanced emergency pause mechanism.
//!
//! Extends the basic pause/unpause in `lib.rs` with:
//! - Named pause conditions that can be checked individually.
//! - A recovery process requiring multi-step acknowledgement.
//! - Notification hooks so external contracts are informed.
//! - Analytics: pause/unpause counters, average pause duration.
//!
//! # Architecture
//!
//! The authoritative pause flag is still the instance-storage `paused` key
//! managed by `lib.rs`. This module adds an **augmentation layer** that sits
//! on top: it records *why* a pause was triggered, tracks recovery steps,
//! and keeps analytics. Every entry point here ultimately calls into the
//! existing `pause`/`unpause` helpers — it does not bypass them.
//!
//! ## Storage layout
//!
//! | Key symbol     | Tier        | Type                    | Purpose                         |
//! |----------------|-------------|-------------------------|---------------------------------|
//! | `ep_conds`     | persistent  | `Vec<PauseCondition>`   | Registered named conditions     |
//! | `ep_active`    | persistent  | `Vec<Symbol>`           | Currently triggered condition ids |
//! | `ep_recovery`  | persistent  | `Option<RecoveryPlan>`  | In-progress recovery plan       |
//! | `ep_notify`    | persistent  | `Vec<Address>`          | Notification subscriber list    |
//! | `ep_analytics` | persistent  | `PauseAnalytics`        | Aggregate pause statistics      |
//! | `ep_last_ts`   | persistent  | `u64`                   | Timestamp of last pause start   |

use soroban_sdk::{contracttype, symbol_short, Address, Env, IntoVal, String, Symbol, Vec};

use crate::error::ContractError;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum number of named pause conditions that can be registered.
pub const MAX_PAUSE_CONDITIONS: u32 = 20;

/// Maximum number of notification subscribers.
pub const MAX_PAUSE_SUBSCRIBERS: u32 = 10;

/// Maximum recovery steps in a plan.
pub const MAX_RECOVERY_STEPS: u32 = 10;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Severity level for a pause condition.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PauseSeverity {
    /// Informational — does not trigger an automatic pause.
    Info = 0,
    /// Warning — may trigger a partial/function-level pause.
    Warning = 1,
    /// Critical — triggers a full contract pause immediately.
    Critical = 2,
}

/// A named, registered condition that can trigger an emergency pause.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PauseCondition {
    /// Short identifier for this condition (e.g. `oracle_fail`).
    pub id: Symbol,
    /// Human-readable description of what triggers this condition.
    pub description: String,
    pub severity: PauseSeverity,
    /// If `true`, triggering this condition immediately pauses the contract.
    pub auto_pause: bool,
    /// Address allowed to trigger this specific condition (None = any admin).
    pub trigger_authority: Option<Address>,
}

/// A single step in the recovery plan.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryStep {
    /// Human-readable description of what must be done in this step.
    pub description: String,
    /// Whether this step has been completed.
    pub completed: bool,
    /// Admin who completed this step (None if not yet completed).
    pub completed_by: Option<Address>,
    /// Timestamp when completed (0 if not yet).
    pub completed_at: u64,
}

/// An active recovery plan that must be completed before unpause is allowed.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryPlan {
    /// The condition IDs being recovered from.
    pub condition_ids: Vec<Symbol>,
    /// Steps that must all be completed before recovery is finalised.
    pub steps: Vec<RecoveryStep>,
    /// Admin who initiated the recovery plan.
    pub initiated_by: Address,
    /// Ledger sequence when the plan was created.
    pub created_at_ledger: u32,
    /// Timestamp when the plan was created.
    pub created_at_ts: u64,
    /// Whether all steps are complete and the contract may be unpaused.
    pub finalised: bool,
}

/// Aggregate analytics tracked across all pause/unpause events.
#[contracttype]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PauseAnalytics {
    /// Total number of times the contract has been paused (emergency or manual).
    pub total_pause_count: u32,
    /// Total number of times the contract has been unpaused.
    pub total_unpause_count: u32,
    /// Cumulative pause duration in seconds across all historical pauses.
    pub cumulative_pause_secs: u64,
    /// Timestamp of the most recent pause event (0 if never paused).
    pub last_paused_at: u64,
    /// Timestamp of the most recent unpause event (0 if never unpaused).
    pub last_unpaused_at: u64,
    /// Number of times the contract was paused due to a critical condition.
    pub critical_trigger_count: u32,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

fn conditions_key() -> Symbol {
    symbol_short!("ep_conds")
}

fn active_conditions_key() -> Symbol {
    symbol_short!("ep_active")
}

fn recovery_plan_key() -> Symbol {
    symbol_short!("ep_recov")
}

fn subscribers_key() -> Symbol {
    symbol_short!("ep_notify")
}

fn analytics_key() -> Symbol {
    symbol_short!("ep_anlytc")
}

fn last_pause_ts_key() -> Symbol {
    symbol_short!("ep_last_t")
}

// ---------------------------------------------------------------------------
// Storage accessors
// ---------------------------------------------------------------------------

pub fn get_conditions(env: &Env) -> Vec<PauseCondition> {
    env.storage()
        .persistent()
        .get(&conditions_key())
        .unwrap_or_else(|| Vec::new(env))
}

fn set_conditions(env: &Env, conditions: &Vec<PauseCondition>) {
    env.storage().persistent().set(&conditions_key(), conditions);
}

pub fn get_active_condition_ids(env: &Env) -> Vec<Symbol> {
    env.storage()
        .persistent()
        .get(&active_conditions_key())
        .unwrap_or_else(|| Vec::new(env))
}

fn set_active_conditions(env: &Env, ids: &Vec<Symbol>) {
    env.storage().persistent().set(&active_conditions_key(), ids);
}

pub fn get_recovery_plan(env: &Env) -> Option<RecoveryPlan> {
    env.storage().persistent().get(&recovery_plan_key())
}

fn set_recovery_plan(env: &Env, plan: &RecoveryPlan) {
    env.storage().persistent().set(&recovery_plan_key(), plan);
}

fn remove_recovery_plan(env: &Env) {
    env.storage().persistent().remove(&recovery_plan_key());
}

pub fn get_subscribers(env: &Env) -> Vec<Address> {
    env.storage()
        .persistent()
        .get(&subscribers_key())
        .unwrap_or_else(|| Vec::new(env))
}

fn set_subscribers(env: &Env, subs: &Vec<Address>) {
    env.storage().persistent().set(&subscribers_key(), subs);
}

pub fn get_analytics(env: &Env) -> PauseAnalytics {
    env.storage()
        .persistent()
        .get(&analytics_key())
        .unwrap_or_default()
}

fn set_analytics(env: &Env, a: &PauseAnalytics) {
    env.storage().persistent().set(&analytics_key(), a);
}

fn get_last_pause_ts(env: &Env) -> u64 {
    env.storage()
        .persistent()
        .get(&last_pause_ts_key())
        .unwrap_or(0u64)
}

fn set_last_pause_ts(env: &Env, ts: u64) {
    env.storage().persistent().set(&last_pause_ts_key(), &ts);
}

// ---------------------------------------------------------------------------
// Condition management
// ---------------------------------------------------------------------------

/// Register a new named pause condition. `admin` must be a super-admin.
pub fn register_condition(
    env: &Env,
    admin: &Address,
    id: Symbol,
    description: String,
    severity: PauseSeverity,
    auto_pause: bool,
    trigger_authority: Option<Address>,
) {
    let _ = admin; // auth is checked by the caller before delegating here
    let mut conditions = get_conditions(env);
    // Prevent duplicates.
    for c in conditions.iter() {
        assert!(c.id != id, "condition id already registered");
    }
    assert!(
        conditions.len() < MAX_PAUSE_CONDITIONS,
        "max pause conditions reached"
    );
    conditions.push_back(PauseCondition {
        id: id.clone(),
        description,
        severity,
        auto_pause,
        trigger_authority,
    });
    set_conditions(env, &conditions);
    emit_condition_registered(env, admin, &id, severity, auto_pause);
}

/// Remove a previously registered condition.
pub fn unregister_condition(env: &Env, admin: &Address, id: &Symbol) {
    let mut conditions = get_conditions(env);
    let mut found = false;
    let mut new_conditions = Vec::new(env);
    for c in conditions.iter() {
        if &c.id == id {
            found = true;
        } else {
            new_conditions.push_back(c);
        }
    }
    assert!(found, "condition not found");
    set_conditions(env, &new_conditions);
    emit_condition_unregistered(env, admin, id);
}

/// Retrieve a single condition by id.
pub fn get_condition(env: &Env, id: &Symbol) -> Option<PauseCondition> {
    for c in get_conditions(env).iter() {
        if &c.id == id {
            return Some(c);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Trigger / clear
// ---------------------------------------------------------------------------

/// Trigger a named condition. If `auto_pause` is set and the severity is
/// Critical, the contract is also immediately paused via the instance flag.
///
/// Returns `true` when the call caused the contract to be paused.
pub fn trigger_condition(
    env: &Env,
    caller: &Address,
    condition_id: &Symbol,
    // A mutable bool that the caller sets to reflect actual pause state.
    is_currently_paused: bool,
    set_paused_fn: &mut dyn FnMut(&Env, bool),
) -> bool {
    let condition = get_condition(env, condition_id)
        .expect("condition not registered");

    // Enforce trigger authority.
    if let Some(ref authority) = condition.trigger_authority {
        assert!(caller == authority, "caller not authorised to trigger condition");
    }

    // Mark condition as active.
    let mut active = get_active_condition_ids(env);
    if !active.iter().any(|a| &a == condition_id) {
        active.push_back(condition_id.clone());
        set_active_conditions(env, &active);
    }

    let mut newly_paused = false;

    // Auto-pause for Critical severity.
    if condition.auto_pause && condition.severity == PauseSeverity::Critical && !is_currently_paused {
        set_paused_fn(env, true);
        newly_paused = true;
        record_pause_analytics(env);
        notify_subscribers(env, caller, condition_id, true);
        emit_emergency_pause_triggered(env, caller, condition_id, condition.severity);
    } else {
        emit_condition_triggered(env, caller, condition_id, condition.severity, is_currently_paused || newly_paused);
    }

    newly_paused
}

/// Clear a named condition (mark it as resolved). Removes it from the active set.
pub fn clear_condition(env: &Env, admin: &Address, condition_id: &Symbol) {
    let mut active = get_active_condition_ids(env);
    let before_len = active.len();
    let mut new_active = Vec::new(env);
    for a in active.iter() {
        if &a != condition_id {
            new_active.push_back(a);
        }
    }
    assert!(new_active.len() < before_len, "condition not active");
    set_active_conditions(env, &new_active);
    emit_condition_cleared(env, admin, condition_id);
}

/// True if any condition with severity ≥ `min_severity` is currently active.
pub fn has_active_conditions_at_severity(env: &Env, min_severity: PauseSeverity) -> bool {
    let active = get_active_condition_ids(env);
    if active.is_empty() {
        return false;
    }
    for id in active.iter() {
        if let Some(c) = get_condition(env, &id) {
            if (c.severity as u32) >= (min_severity as u32) {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Recovery
// ---------------------------------------------------------------------------

/// Initiate a recovery plan. This is mandatory before unpausing when active
/// Critical conditions exist. Providing steps forces admins to document the
/// remediation work.
pub fn initiate_recovery(
    env: &Env,
    admin: &Address,
    steps: Vec<String>,
) {
    assert!(
        get_recovery_plan(env).is_none(),
        "recovery plan already active"
    );
    assert!(!steps.is_empty(), "recovery plan must have at least one step");
    assert!(
        steps.len() <= MAX_RECOVERY_STEPS as u32,
        "too many recovery steps"
    );

    let active = get_active_condition_ids(env);
    let mut recovery_steps = Vec::new(env);
    for desc in steps.iter() {
        recovery_steps.push_back(RecoveryStep {
            description: desc,
            completed: false,
            completed_by: None,
            completed_at: 0,
        });
    }

    let plan = RecoveryPlan {
        condition_ids: active,
        steps: recovery_steps,
        initiated_by: admin.clone(),
        created_at_ledger: env.ledger().sequence(),
        created_at_ts: env.ledger().timestamp(),
        finalised: false,
    };
    set_recovery_plan(env, &plan);
    emit_recovery_initiated(env, admin, plan.steps.len());
}

/// Mark a recovery step as completed. Steps are 0-indexed.
pub fn complete_recovery_step(env: &Env, admin: &Address, step_index: u32) {
    let mut plan = get_recovery_plan(env).expect("no active recovery plan");
    assert!(!plan.finalised, "recovery plan already finalised");
    assert!(
        (step_index as u32) < plan.steps.len(),
        "invalid step index"
    );
    let mut step: RecoveryStep = plan.steps.get(step_index).unwrap();
    assert!(!step.completed, "step already completed");
    step.completed = true;
    step.completed_by = Some(admin.clone());
    step.completed_at = env.ledger().timestamp();
    plan.steps.set(step_index, step);

    // Check if all steps are now complete.
    let all_done = plan.steps.iter().all(|s| s.completed);
    if all_done {
        plan.finalised = true;
        emit_recovery_finalised(env, admin);
    }

    set_recovery_plan(env, &plan);
    emit_recovery_step_completed(env, admin, step_index);
}

/// Discard an in-progress recovery plan (e.g. if the situation changes).
pub fn cancel_recovery(env: &Env, admin: &Address) {
    assert!(get_recovery_plan(env).is_some(), "no active recovery plan");
    remove_recovery_plan(env);
    emit_recovery_cancelled(env, admin);
}

/// Check that recovery prerequisites are met before allowing an unpause.
/// Returns `Err(ContractError::RecoveryIncomplete)` if they are not.
pub fn assert_recovery_complete(env: &Env) -> Result<(), ContractError> {
    // If there are active Critical conditions, a finalised recovery plan is required.
    if has_active_conditions_at_severity(env, PauseSeverity::Critical) {
        match get_recovery_plan(env) {
            Some(plan) if plan.finalised => Ok(()),
            _ => Err(ContractError::RecoveryIncomplete),
        }
    } else {
        // No critical conditions — no recovery plan required.
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Notification subscribers
// ---------------------------------------------------------------------------

/// Subscribe an external contract to pause/unpause notifications.
pub fn add_subscriber(env: &Env, admin: &Address, subscriber: Address) {
    let _ = admin;
    let mut subs = get_subscribers(env);
    if subs.iter().any(|s| s == subscriber) {
        return; // idempotent
    }
    assert!(subs.len() < MAX_PAUSE_SUBSCRIBERS, "subscriber list full");
    subs.push_back(subscriber.clone());
    set_subscribers(env, &subs);
    emit_subscriber_added(env, admin, &subscriber);
}

/// Remove a notification subscriber.
pub fn remove_subscriber(env: &Env, admin: &Address, subscriber: &Address) {
    let mut subs = get_subscribers(env);
    let mut new_subs = Vec::new(env);
    let mut found = false;
    for s in subs.iter() {
        if &s == subscriber {
            found = true;
        } else {
            new_subs.push_back(s);
        }
    }
    assert!(found, "subscriber not found");
    set_subscribers(env, &new_subs);
    emit_subscriber_removed(env, admin, subscriber);
}

/// Dispatch notifications to all registered subscribers.
/// Each subscriber is called with `(paused: bool, condition_id: Symbol)`.
/// Failures are swallowed so a bad subscriber cannot block the pause.
pub fn notify_subscribers(env: &Env, actor: &Address, condition_id: &Symbol, paused: bool) {
    let _ = actor;
    let subs = get_subscribers(env);
    for sub in subs.iter() {
        // Best-effort: ignore any error from a subscriber.
        let _ = env.try_invoke_contract::<(), soroban_sdk::Error>(
            &sub,
            &symbol_short!("on_pause"),
            (paused, condition_id.clone()).into_val(env),
        );
    }
}

// ---------------------------------------------------------------------------
// Analytics
// ---------------------------------------------------------------------------

/// Record that a pause just started. Called when the contract transitions to
/// paused state via this module.
pub fn record_pause_analytics(env: &Env) {
    let now = env.ledger().timestamp();
    let mut a = get_analytics(env);
    a.total_pause_count = a.total_pause_count.saturating_add(1);
    a.last_paused_at = now;
    set_analytics(env, &a);
    set_last_pause_ts(env, now);
}

/// Record that the contract was just unpaused. Accumulates the pause duration.
pub fn record_unpause_analytics(env: &Env) {
    let now = env.ledger().timestamp();
    let last = get_last_pause_ts(env);
    let duration = if last > 0 && now > last { now - last } else { 0 };
    let mut a = get_analytics(env);
    a.total_unpause_count = a.total_unpause_count.saturating_add(1);
    a.cumulative_pause_secs = a.cumulative_pause_secs.saturating_add(duration);
    a.last_unpaused_at = now;
    set_analytics(env, &a);
}

/// Increment the critical-trigger counter in analytics.
pub fn record_critical_trigger(env: &Env) {
    let mut a = get_analytics(env);
    a.critical_trigger_count = a.critical_trigger_count.saturating_add(1);
    set_analytics(env, &a);
}

/// Average pause duration in seconds (0 if never unpaused).
pub fn average_pause_duration_secs(env: &Env) -> u64 {
    let a = get_analytics(env);
    if a.total_unpause_count == 0 {
        return 0;
    }
    a.cumulative_pause_secs / a.total_unpause_count as u64
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

fn emit_condition_registered(
    env: &Env,
    admin: &Address,
    id: &Symbol,
    severity: PauseSeverity,
    auto_pause: bool,
) {
    env.events().publish(
        (symbol_short!("ep"), symbol_short!("cond_reg")),
        (admin.clone(), id.clone(), severity as u32, auto_pause),
    );
}

fn emit_condition_unregistered(env: &Env, admin: &Address, id: &Symbol) {
    env.events().publish(
        (symbol_short!("ep"), symbol_short!("cond_del")),
        (admin.clone(), id.clone()),
    );
}

fn emit_condition_triggered(
    env: &Env,
    caller: &Address,
    id: &Symbol,
    severity: PauseSeverity,
    contract_paused: bool,
) {
    env.events().publish(
        (symbol_short!("ep"), symbol_short!("cond_trg")),
        (caller.clone(), id.clone(), severity as u32, contract_paused),
    );
}

fn emit_emergency_pause_triggered(
    env: &Env,
    caller: &Address,
    condition_id: &Symbol,
    severity: PauseSeverity,
) {
    env.events().publish(
        (symbol_short!("ep"), symbol_short!("emrg_pau")),
        (caller.clone(), condition_id.clone(), severity as u32, env.ledger().timestamp()),
    );
}

fn emit_condition_cleared(env: &Env, admin: &Address, id: &Symbol) {
    env.events().publish(
        (symbol_short!("ep"), symbol_short!("cond_clr")),
        (admin.clone(), id.clone()),
    );
}

fn emit_recovery_initiated(env: &Env, admin: &Address, step_count: u32) {
    env.events().publish(
        (symbol_short!("ep"), symbol_short!("recov_ini")),
        (admin.clone(), step_count),
    );
}

fn emit_recovery_step_completed(env: &Env, admin: &Address, step_index: u32) {
    env.events().publish(
        (symbol_short!("ep"), symbol_short!("recov_stp")),
        (admin.clone(), step_index),
    );
}

fn emit_recovery_finalised(env: &Env, admin: &Address) {
    env.events().publish(
        (symbol_short!("ep"), symbol_short!("recov_fin")),
        (admin.clone(), env.ledger().timestamp()),
    );
}

fn emit_recovery_cancelled(env: &Env, admin: &Address) {
    env.events().publish(
        (symbol_short!("ep"), symbol_short!("recov_cxl")),
        admin.clone(),
    );
}

fn emit_subscriber_added(env: &Env, admin: &Address, subscriber: &Address) {
    env.events().publish(
        (symbol_short!("ep"), symbol_short!("sub_add")),
        (admin.clone(), subscriber.clone()),
    );
}

fn emit_subscriber_removed(env: &Env, admin: &Address, subscriber: &Address) {
    env.events().publish(
        (symbol_short!("ep"), symbol_short!("sub_del")),
        (admin.clone(), subscriber.clone()),
    );
}
