//! Contract monitoring and alerting (issue #1033).
//!
//! Builds on the usage analytics from issue #1034 to watch the contract for
//! unusual activity and raise alerts.
//!
//! - **Metric collection**: [`collect_metric`] reads a [`MetricKind`] from the
//!   analytics aggregates (today's bucket or lifetime totals). The size of the
//!   payment being processed is also available as a metric.
//! - **Alerting rules**: the admin registers up to [`MAX_ALERT_RULES`]
//!   [`AlertRule`]s, each comparing one metric against a threshold, with a
//!   severity and a cooldown so a rule cannot fire on every call.
//! - **Evaluation**: rules are checked automatically after each recorded usage
//!   event ([`on_usage`], called from `analytics::record`), but only rules for
//!   metrics that event can change. [`check_all`] evaluates every rule and is
//!   open to anyone, so a keeper can catch `Below` conditions such as "no
//!   payments today" that no event would trigger.
//! - **Alert delivery**: each alert is stored in a bounded history, emitted as
//!   a `(split, alert, rule_id)` event for off-chain relayers, and, when the
//!   admin has set a receiver contract, pushed to its `on_alert(alert)`
//!   function with `try_invoke_contract`. A failing receiver can never revert
//!   the operation that raised the alert.
//! - **Monitoring analytics**: [`MonitoringStats`] counts evaluations, alerts
//!   by severity, unacknowledged alerts and failed deliveries.
//! - **Dashboard**: [`dashboard`] returns stats, rules, current metric values
//!   and the most recent alerts in one read.
//!
//! When no rules are registered, [`on_usage`] returns after one instance read,
//! so the payment path pays almost nothing for monitoring by default.

use soroban_sdk::{contracttype, symbol_short, Address, Env, IntoVal, Symbol, Val, Vec};

use crate::analytics::{self, UsageKind};
use crate::error::ContractError;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum number of alert rules (bounds the evaluation loop).
pub const MAX_ALERT_RULES: u32 = 20;
/// Number of most recent alerts kept in history.
pub const MAX_ALERT_HISTORY: u64 = 50;
/// Number of alerts returned by the dashboard.
pub const DASHBOARD_RECENT_ALERTS: u64 = 10;
/// Function called on the alert receiver contract.
pub const RECEIVER_FN: &str = "on_alert";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A value the monitor can watch.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetricKind {
    /// Invoices created today.
    DailyInvoicesCreated,
    /// Payments received today.
    DailyPayments,
    /// Payment volume received today.
    DailyPaymentVolume,
    /// Refunds issued today.
    DailyRefunds,
    /// Refund volume issued today.
    DailyRefundVolume,
    /// Lifetime refunded invoices / created invoices, in basis points.
    RefundRateBps,
    /// Amount of the single payment being processed. Only evaluated when a
    /// payment is recorded, never by `check_all`.
    PaymentAmount,
}

/// How a metric is compared to a rule's threshold.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Comparator {
    /// Fires when the value is strictly greater than the threshold.
    Above,
    /// Fires when the value is strictly less than the threshold.
    Below,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

/// An alerting rule. `id`, `last_triggered_at` and `trigger_count` are set by
/// the contract; values passed in for them are ignored.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AlertRule {
    pub id: u32,
    pub metric: MetricKind,
    pub comparator: Comparator,
    pub threshold: i128,
    pub severity: AlertSeverity,
    /// Minimum seconds between two alerts from this rule.
    pub cooldown_secs: u64,
    pub enabled: bool,
    pub last_triggered_at: u64,
    pub trigger_count: u32,
}

/// One raised alert.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Alert {
    pub id: u64,
    pub rule_id: u32,
    pub metric: MetricKind,
    pub severity: AlertSeverity,
    pub value: i128,
    pub threshold: i128,
    pub triggered_at: u64,
    pub acknowledged: bool,
    /// `Some(true)` delivered to the receiver contract, `Some(false)` the
    /// receiver call failed, `None` no receiver was set.
    pub delivered: Option<bool>,
}

/// Counters about the monitor itself.
#[contracttype]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MonitoringStats {
    /// Rule evaluations performed.
    pub evaluations: u64,
    pub total_alerts: u64,
    pub info_alerts: u64,
    pub warning_alerts: u64,
    pub critical_alerts: u64,
    pub unacknowledged: u64,
    pub failed_deliveries: u64,
    pub last_alert_at: u64,
}

/// A metric and its current value.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetricValue {
    pub metric: MetricKind,
    pub value: i128,
}

/// Everything a monitoring dashboard needs in one call.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MonitoringDashboard {
    pub stats: MonitoringStats,
    pub rules: Vec<AlertRule>,
    pub metrics: Vec<MetricValue>,
    /// Most recent alerts, newest first.
    pub recent_alerts: Vec<Alert>,
    pub receiver: Option<Address>,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

fn rules_key() -> Symbol {
    symbol_short!("mon_rul")
}

fn next_rule_id_key() -> Symbol {
    symbol_short!("mon_rid")
}

fn next_alert_id_key() -> Symbol {
    symbol_short!("mon_aid")
}

fn stats_key() -> Symbol {
    symbol_short!("mon_st")
}

fn receiver_key() -> Symbol {
    symbol_short!("mon_rcv")
}

fn alert_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("mon_al"), id)
}

// ---------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------

pub fn get_rules(env: &Env) -> Vec<AlertRule> {
    env.storage()
        .instance()
        .get(&rules_key())
        .unwrap_or_else(|| Vec::new(env))
}

fn save_rules(env: &Env, rules: &Vec<AlertRule>) {
    env.storage().instance().set(&rules_key(), rules);
}

/// Add a rule and return its id. The caller must check admin auth.
pub fn add_rule(env: &Env, rule: AlertRule) -> Result<u32, ContractError> {
    if rule.metric == MetricKind::RefundRateBps && rule.threshold > 10_000 {
        return Err(ContractError::InvalidArgument);
    }
    if rule.threshold < 0 {
        return Err(ContractError::InvalidArgument);
    }
    let mut rules = get_rules(env);
    if rules.len() >= MAX_ALERT_RULES {
        return Err(ContractError::BatchLimitExceeded);
    }
    let id: u32 = env.storage().instance().get(&next_rule_id_key()).unwrap_or(1);
    env.storage()
        .instance()
        .set(&next_rule_id_key(), &id.saturating_add(1));

    rules.push_back(AlertRule {
        id,
        last_triggered_at: 0,
        trigger_count: 0,
        ..rule
    });
    save_rules(env, &rules);
    env.events()
        .publish((symbol_short!("split"), symbol_short!("mon_add")), id);
    Ok(id)
}

/// Remove a rule. The caller must check admin auth.
pub fn remove_rule(env: &Env, rule_id: u32) -> Result<(), ContractError> {
    let rules = get_rules(env);
    let index = find_rule(&rules, rule_id).ok_or(ContractError::MissingStorageEntry)?;
    let mut rules = rules;
    rules.remove(index);
    save_rules(env, &rules);
    env.events()
        .publish((symbol_short!("split"), symbol_short!("mon_rm")), rule_id);
    Ok(())
}

/// Enable or disable a rule. The caller must check admin auth.
pub fn set_rule_enabled(env: &Env, rule_id: u32, enabled: bool) -> Result<(), ContractError> {
    let mut rules = get_rules(env);
    let index = find_rule(&rules, rule_id).ok_or(ContractError::MissingStorageEntry)?;
    let mut rule = rules.get_unchecked(index);
    rule.enabled = enabled;
    rules.set(index, rule);
    save_rules(env, &rules);
    Ok(())
}

fn find_rule(rules: &Vec<AlertRule>, rule_id: u32) -> Option<u32> {
    for (i, rule) in rules.iter().enumerate() {
        if rule.id == rule_id {
            return Some(i as u32);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Receiver
// ---------------------------------------------------------------------------

pub fn get_receiver(env: &Env) -> Option<Address> {
    env.storage().instance().get(&receiver_key())
}

/// Set (`Some`) or clear (`None`) the alert receiver contract. The caller
/// must check admin auth.
pub fn set_receiver(env: &Env, receiver: &Option<Address>) {
    match receiver {
        Some(addr) => env.storage().instance().set(&receiver_key(), addr),
        None => env.storage().instance().remove(&receiver_key()),
    }
}

// ---------------------------------------------------------------------------
// Metric collection
// ---------------------------------------------------------------------------

/// Current value of `metric`. `PaymentAmount` has no stored value and
/// returns `None`; it is only known while a payment is being recorded.
pub fn collect_metric(env: &Env, metric: MetricKind) -> Option<i128> {
    let today = analytics::get_day(env, analytics::day_of(env.ledger().timestamp()));
    match metric {
        MetricKind::DailyInvoicesCreated => Some(today.invoices_created as i128),
        MetricKind::DailyPayments => Some(today.payments as i128),
        MetricKind::DailyPaymentVolume => Some(today.payment_volume),
        MetricKind::DailyRefunds => Some(today.refunds as i128),
        MetricKind::DailyRefundVolume => Some(today.refund_volume),
        MetricKind::RefundRateBps => {
            let totals = analytics::get_totals(env);
            if totals.invoices_created == 0 {
                Some(0)
            } else {
                Some(totals.refunds as i128 * 10_000 / totals.invoices_created as i128)
            }
        }
        MetricKind::PaymentAmount => None,
    }
}

/// Metrics that a usage event of `kind` can change.
fn affects(kind: UsageKind, metric: MetricKind) -> bool {
    match kind {
        UsageKind::InvoiceCreated => matches!(
            metric,
            MetricKind::DailyInvoicesCreated | MetricKind::RefundRateBps
        ),
        UsageKind::PaymentReceived => matches!(
            metric,
            MetricKind::DailyPayments | MetricKind::DailyPaymentVolume | MetricKind::PaymentAmount
        ),
        UsageKind::InvoiceRefunded => matches!(
            metric,
            MetricKind::DailyRefunds | MetricKind::DailyRefundVolume | MetricKind::RefundRateBps
        ),
        UsageKind::InvoiceReleased => false,
    }
}

/// Whether `value` breaches the rule's threshold.
pub fn breaches(comparator: Comparator, value: i128, threshold: i128) -> bool {
    match comparator {
        Comparator::Above => value > threshold,
        Comparator::Below => value < threshold,
    }
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

/// Called after every recorded usage event. Evaluates only the enabled rules
/// whose metric this event can change.
pub fn on_usage(env: &Env, kind: UsageKind, amount: i128) {
    if !env.storage().instance().has(&rules_key()) {
        return;
    }
    evaluate(env, Some((kind, amount)));
}

/// Evaluate every enabled rule (except `PaymentAmount`, which needs a
/// payment). Returns the number of alerts raised.
pub fn check_all(env: &Env) -> u32 {
    evaluate(env, None)
}

fn evaluate(env: &Env, trigger: Option<(UsageKind, i128)>) -> u32 {
    let mut rules = get_rules(env);
    if rules.is_empty() {
        return 0;
    }
    let now = env.ledger().timestamp();
    let mut stats = get_stats(env);
    let mut raised = 0u32;
    let mut changed = false;

    for i in 0..rules.len() {
        let mut rule = rules.get_unchecked(i);
        if !rule.enabled {
            continue;
        }
        let value = match trigger {
            Some((kind, amount)) => {
                if !affects(kind, rule.metric) {
                    continue;
                }
                if rule.metric == MetricKind::PaymentAmount {
                    Some(amount)
                } else {
                    collect_metric(env, rule.metric)
                }
            }
            None => collect_metric(env, rule.metric),
        };
        let Some(value) = value else { continue };

        stats.evaluations = stats.evaluations.saturating_add(1);
        changed = true;

        if !breaches(rule.comparator, value, rule.threshold) {
            continue;
        }
        let cooled_down = rule.trigger_count == 0
            || now >= rule.last_triggered_at.saturating_add(rule.cooldown_secs);
        if !cooled_down {
            continue;
        }

        rule.last_triggered_at = now;
        rule.trigger_count = rule.trigger_count.saturating_add(1);
        rules.set(i, rule.clone());
        raise(env, &rule, value, now, &mut stats);
        raised += 1;
    }

    if raised > 0 {
        save_rules(env, &rules);
    }
    if changed {
        save_stats(env, &stats);
    }
    raised
}

/// Store, publish and deliver one alert.
fn raise(env: &Env, rule: &AlertRule, value: i128, now: u64, stats: &mut MonitoringStats) {
    let id: u64 = env.storage().instance().get(&next_alert_id_key()).unwrap_or(1);
    env.storage()
        .instance()
        .set(&next_alert_id_key(), &id.saturating_add(1));

    let mut alert = Alert {
        id,
        rule_id: rule.id,
        metric: rule.metric,
        severity: rule.severity,
        value,
        threshold: rule.threshold,
        triggered_at: now,
        acknowledged: false,
        delivered: None,
    };

    env.events().publish(
        (symbol_short!("split"), symbol_short!("alert"), rule.id),
        (id, rule.metric, rule.severity, value, rule.threshold),
    );

    if let Some(receiver) = get_receiver(env) {
        let mut args: Vec<Val> = Vec::new(env);
        args.push_back(alert.clone().into_val(env));
        let res = env.try_invoke_contract::<Val, soroban_sdk::Error>(
            &receiver,
            &Symbol::new(env, RECEIVER_FN),
            args,
        );
        let ok = matches!(res, Ok(Ok(_)));
        alert.delivered = Some(ok);
        if !ok {
            stats.failed_deliveries = stats.failed_deliveries.saturating_add(1);
        }
    }

    env.storage().persistent().set(&alert_key(id), &alert);
    // Keep only the most recent MAX_ALERT_HISTORY alerts.
    if id > MAX_ALERT_HISTORY {
        let old = alert_key(id - MAX_ALERT_HISTORY);
        if let Some(old_alert) = env.storage().persistent().get::<_, Alert>(&old) {
            if !old_alert.acknowledged {
                stats.unacknowledged = stats.unacknowledged.saturating_sub(1);
            }
            env.storage().persistent().remove(&old);
        }
    }

    stats.total_alerts = stats.total_alerts.saturating_add(1);
    stats.unacknowledged = stats.unacknowledged.saturating_add(1);
    stats.last_alert_at = now;
    match rule.severity {
        AlertSeverity::Info => stats.info_alerts = stats.info_alerts.saturating_add(1),
        AlertSeverity::Warning => stats.warning_alerts = stats.warning_alerts.saturating_add(1),
        AlertSeverity::Critical => stats.critical_alerts = stats.critical_alerts.saturating_add(1),
    }
}

// ---------------------------------------------------------------------------
// Alerts
// ---------------------------------------------------------------------------

pub fn get_alert(env: &Env, alert_id: u64) -> Option<Alert> {
    env.storage().persistent().get(&alert_key(alert_id))
}

/// Mark an alert as handled. The caller must check admin auth.
pub fn acknowledge(env: &Env, alert_id: u64) -> Result<(), ContractError> {
    let mut alert = get_alert(env, alert_id).ok_or(ContractError::MissingStorageEntry)?;
    if alert.acknowledged {
        return Ok(());
    }
    alert.acknowledged = true;
    env.storage().persistent().set(&alert_key(alert_id), &alert);
    let mut stats = get_stats(env);
    stats.unacknowledged = stats.unacknowledged.saturating_sub(1);
    save_stats(env, &stats);
    env.events()
        .publish((symbol_short!("split"), symbol_short!("alert_ack")), alert_id);
    Ok(())
}

/// Up to `limit` most recent alerts, newest first (capped at the history size).
pub fn recent_alerts(env: &Env, limit: u64) -> Vec<Alert> {
    let mut out = Vec::new(env);
    let next: u64 = env.storage().instance().get(&next_alert_id_key()).unwrap_or(1);
    let limit = limit.min(MAX_ALERT_HISTORY);
    let mut id = next;
    while id > 1 && (out.len() as u64) < limit {
        id -= 1;
        match get_alert(env, id) {
            Some(alert) => out.push_back(alert),
            None => break,
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Stats + dashboard
// ---------------------------------------------------------------------------

pub fn get_stats(env: &Env) -> MonitoringStats {
    env.storage().instance().get(&stats_key()).unwrap_or_default()
}

fn save_stats(env: &Env, stats: &MonitoringStats) {
    env.storage().instance().set(&stats_key(), stats);
}

/// Current value of every collectable metric.
pub fn snapshot(env: &Env) -> Vec<MetricValue> {
    let mut out = Vec::new(env);
    for metric in [
        MetricKind::DailyInvoicesCreated,
        MetricKind::DailyPayments,
        MetricKind::DailyPaymentVolume,
        MetricKind::DailyRefunds,
        MetricKind::DailyRefundVolume,
        MetricKind::RefundRateBps,
    ] {
        if let Some(value) = collect_metric(env, metric) {
            out.push_back(MetricValue { metric, value });
        }
    }
    out
}

pub fn dashboard(env: &Env) -> MonitoringDashboard {
    MonitoringDashboard {
        stats: get_stats(env),
        rules: get_rules(env),
        metrics: snapshot(env),
        recent_alerts: recent_alerts(env, DASHBOARD_RECENT_ALERTS),
        receiver: get_receiver(env),
    }
}
