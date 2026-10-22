//! Contract usage analytics (issue #1034).
//!
//! Tracks how the contract is used so operators and dashboards can see
//! activity without replaying the full event history.
//!
//! - **Event tracking**: [`record`] is called from the lifecycle event helpers
//!   in `events.rs` (`invoice_created`, `payment_received`, `invoice_released`,
//!   `invoice_refunded`). Every code path that emits one of those events is
//!   counted, with no changes needed at each call site.
//! - **Aggregation**: activity is rolled up into lifetime [`UsageTotals`]
//!   (instance storage) and per-UTC-day [`DailyUsage`] buckets (persistent
//!   storage, keyed by `timestamp / 86_400`).
//! - **Reporting**: [`report`] returns the daily buckets for a day range,
//!   including empty days, so charts have no gaps.
//! - **Dashboard**: [`dashboard`] bundles lifetime totals, today's bucket and
//!   derived insights in one read.
//! - **Insights**: [`insights`] derives averages, conversion rates and the
//!   week-over-week payment trend from the stored aggregates.
//!
//! Recording writes storage but never publishes events, so it does not change
//! the event stream that indexers and tests rely on. The admin can switch
//! recording off with [`set_enabled`]; reads keep working on the data already
//! collected.

use soroban_sdk::{contracttype, symbol_short, Address, Env, Symbol, Vec};

use crate::error::ContractError;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Seconds per analytics bucket (one UTC day).
pub const SECONDS_PER_DAY: u64 = 86_400;
/// Maximum number of days a single report may span (bounds the read loop).
pub const MAX_REPORT_DAYS: u64 = 90;
/// Length of the comparison windows used for the trend insight.
pub const TREND_WINDOW_DAYS: u64 = 7;
/// Basis-point scale used for rates.
const BPS: i128 = 10_000;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// The kinds of contract usage that are tracked.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsageKind {
    InvoiceCreated,
    PaymentReceived,
    InvoiceReleased,
    InvoiceRefunded,
}

/// Lifetime usage counters.
#[contracttype]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UsageTotals {
    pub invoices_created: u64,
    pub payments: u64,
    pub releases: u64,
    pub refunds: u64,
    /// Sum of invoice totals at creation.
    pub created_volume: i128,
    /// Sum of payment amounts received.
    pub payment_volume: i128,
    /// Sum of amounts refunded.
    pub refund_volume: i128,
    /// Distinct addresses that have created an invoice.
    pub unique_creators: u64,
    /// Distinct addresses that have paid into an invoice.
    pub unique_payers: u64,
    /// Timestamp of the first recorded activity (0 if none).
    pub first_activity_at: u64,
    /// Timestamp of the most recent recorded activity (0 if none).
    pub last_activity_at: u64,
}

/// Usage aggregated over one UTC day.
#[contracttype]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DailyUsage {
    /// Day index: `timestamp / 86_400`.
    pub day: u64,
    pub invoices_created: u32,
    pub payments: u32,
    pub releases: u32,
    pub refunds: u32,
    pub created_volume: i128,
    pub payment_volume: i128,
    pub refund_volume: i128,
}

/// Values derived from the aggregates.
#[contracttype]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UsageInsights {
    /// Average payment amount (0 when there are no payments).
    pub avg_payment: i128,
    /// Average invoice total at creation (0 when there are no invoices).
    pub avg_invoice_total: i128,
    /// Average payments per created invoice, scaled by 100 (e.g. 250 = 2.5).
    pub payments_per_invoice_x100: u64,
    /// Released invoices as a share of created invoices, in basis points.
    pub release_rate_bps: u32,
    /// Refunded invoices as a share of created invoices, in basis points.
    pub refund_rate_bps: u32,
    /// Payments in the last 7 days (today included).
    pub payments_this_week: u32,
    /// Payments in the 7 days before that.
    pub payments_last_week: u32,
    /// Week-over-week change in payment count, in basis points. `None` when
    /// the previous week had no payments, so there is nothing to compare to.
    pub payment_growth_bps: Option<i64>,
    /// Day index with the most payments in the last 14 days (0 if none).
    pub busiest_day: u64,
    /// Payment count on `busiest_day`.
    pub busiest_day_payments: u32,
}

/// Everything a usage dashboard needs in one call.
#[contracttype]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UsageDashboard {
    pub enabled: bool,
    pub totals: UsageTotals,
    pub today: DailyUsage,
    pub insights: UsageInsights,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

fn enabled_key() -> Symbol {
    symbol_short!("an_on")
}

fn totals_key() -> Symbol {
    symbol_short!("an_tot")
}

fn day_key(day: u64) -> (Symbol, u64) {
    (symbol_short!("an_day"), day)
}

fn creator_seen_key(addr: &Address) -> (Symbol, Address) {
    (symbol_short!("an_crt"), addr.clone())
}

fn payer_seen_key(addr: &Address) -> (Symbol, Address) {
    (symbol_short!("an_pyr"), addr.clone())
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Whether usage recording is on. Defaults to `true`.
pub fn is_enabled(env: &Env) -> bool {
    env.storage().instance().get(&enabled_key()).unwrap_or(true)
}

/// Turn usage recording on or off. The caller must check admin auth.
pub fn set_enabled(env: &Env, enabled: bool) {
    env.storage().instance().set(&enabled_key(), &enabled);
    env.events()
        .publish((symbol_short!("split"), symbol_short!("an_on")), enabled);
}

// ---------------------------------------------------------------------------
// Recording
// ---------------------------------------------------------------------------

/// Day index for a ledger timestamp.
pub fn day_of(timestamp: u64) -> u64 {
    timestamp / SECONDS_PER_DAY
}

/// Record one usage event.
///
/// * `actor` is the creator for `InvoiceCreated` and the payer for
///   `PaymentReceived`; it is ignored for the other kinds.
/// * `amount` is the invoice total, payment amount or refund amount; it is
///   ignored for `InvoiceReleased`. Negative amounts are treated as zero.
///
/// Counters saturate instead of failing, so analytics can never revert the
/// operation being recorded.
pub fn record(env: &Env, kind: UsageKind, actor: Option<&Address>, amount: i128) {
    if !is_enabled(env) {
        return;
    }
    let now = env.ledger().timestamp();
    let amount = if amount > 0 { amount } else { 0 };

    let mut totals = get_totals(env);
    let day = day_of(now);
    let mut bucket = get_day(env, day);

    match kind {
        UsageKind::InvoiceCreated => {
            totals.invoices_created = totals.invoices_created.saturating_add(1);
            totals.created_volume = totals.created_volume.saturating_add(amount);
            bucket.invoices_created = bucket.invoices_created.saturating_add(1);
            bucket.created_volume = bucket.created_volume.saturating_add(amount);
            if let Some(addr) = actor {
                if mark_seen(env, creator_seen_key(addr)) {
                    totals.unique_creators = totals.unique_creators.saturating_add(1);
                }
            }
        }
        UsageKind::PaymentReceived => {
            totals.payments = totals.payments.saturating_add(1);
            totals.payment_volume = totals.payment_volume.saturating_add(amount);
            bucket.payments = bucket.payments.saturating_add(1);
            bucket.payment_volume = bucket.payment_volume.saturating_add(amount);
            if let Some(addr) = actor {
                if mark_seen(env, payer_seen_key(addr)) {
                    totals.unique_payers = totals.unique_payers.saturating_add(1);
                }
            }
        }
        UsageKind::InvoiceReleased => {
            totals.releases = totals.releases.saturating_add(1);
            bucket.releases = bucket.releases.saturating_add(1);
        }
        UsageKind::InvoiceRefunded => {
            totals.refunds = totals.refunds.saturating_add(1);
            totals.refund_volume = totals.refund_volume.saturating_add(amount);
            bucket.refunds = bucket.refunds.saturating_add(1);
            bucket.refund_volume = bucket.refund_volume.saturating_add(amount);
        }
    }

    if totals.first_activity_at == 0 {
        totals.first_activity_at = now;
    }
    totals.last_activity_at = now;

    env.storage().instance().set(&totals_key(), &totals);
    env.storage().persistent().set(&day_key(day), &bucket);
}

/// Mark an address as seen. Returns `true` the first time.
fn mark_seen(env: &Env, key: (Symbol, Address)) -> bool {
    let storage = env.storage().persistent();
    if storage.has(&key) {
        false
    } else {
        storage.set(&key, &true);
        true
    }
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

/// Lifetime totals (all zero before any activity).
pub fn get_totals(env: &Env) -> UsageTotals {
    env.storage().instance().get(&totals_key()).unwrap_or_default()
}

/// The bucket for `day`, or an empty bucket if nothing happened that day.
pub fn get_day(env: &Env, day: u64) -> DailyUsage {
    env.storage()
        .persistent()
        .get(&day_key(day))
        .unwrap_or(DailyUsage { day, ..Default::default() })
}

/// Daily buckets for `from_day..=to_day`, one entry per day (empty days
/// included). Fails with `InvalidArgument` when the range is reversed or
/// longer than [`MAX_REPORT_DAYS`].
pub fn report(env: &Env, from_day: u64, to_day: u64) -> Result<Vec<DailyUsage>, ContractError> {
    if from_day > to_day || to_day - from_day >= MAX_REPORT_DAYS {
        return Err(ContractError::InvalidArgument);
    }
    let mut out = Vec::new(env);
    for offset in 0..=(to_day - from_day) {
        out.push_back(get_day(env, from_day + offset));
    }
    Ok(out)
}

/// Sum of payments over `days` days ending at `last_day` (inclusive).
fn payments_in_window(env: &Env, last_day: u64, days: u64) -> u32 {
    let mut sum: u32 = 0;
    for offset in 0..days {
        if offset > last_day {
            break;
        }
        sum = sum.saturating_add(get_day(env, last_day - offset).payments);
    }
    sum
}

fn rate_bps(part: u64, whole: u64) -> u32 {
    if whole == 0 {
        return 0;
    }
    let bps = (part as i128).saturating_mul(BPS) / whole as i128;
    bps.min(u32::MAX as i128) as u32
}

/// Derived insights from the stored aggregates.
pub fn insights(env: &Env) -> UsageInsights {
    let totals = get_totals(env);
    let today = day_of(env.ledger().timestamp());

    let avg_payment = if totals.payments > 0 {
        totals.payment_volume / totals.payments as i128
    } else {
        0
    };
    let avg_invoice_total = if totals.invoices_created > 0 {
        totals.created_volume / totals.invoices_created as i128
    } else {
        0
    };
    let payments_per_invoice_x100 = if totals.invoices_created > 0 {
        totals.payments.saturating_mul(100) / totals.invoices_created
    } else {
        0
    };

    let this_week = payments_in_window(env, today, TREND_WINDOW_DAYS);
    let last_week = if today >= TREND_WINDOW_DAYS {
        payments_in_window(env, today - TREND_WINDOW_DAYS, TREND_WINDOW_DAYS)
    } else {
        0
    };
    let payment_growth_bps = if last_week > 0 {
        let diff = this_week as i64 - last_week as i64;
        Some(diff * BPS as i64 / last_week as i64)
    } else {
        None
    };

    let mut busiest_day = 0u64;
    let mut busiest_day_payments = 0u32;
    for offset in 0..(TREND_WINDOW_DAYS * 2) {
        if offset > today {
            break;
        }
        let bucket = get_day(env, today - offset);
        if bucket.payments > busiest_day_payments {
            busiest_day = bucket.day;
            busiest_day_payments = bucket.payments;
        }
    }

    UsageInsights {
        avg_payment,
        avg_invoice_total,
        payments_per_invoice_x100,
        release_rate_bps: rate_bps(totals.releases, totals.invoices_created),
        refund_rate_bps: rate_bps(totals.refunds, totals.invoices_created),
        payments_this_week: this_week,
        payments_last_week: last_week,
        payment_growth_bps,
        busiest_day,
        busiest_day_payments,
    }
}

/// Totals, today's bucket and insights in one read.
pub fn dashboard(env: &Env) -> UsageDashboard {
    UsageDashboard {
        enabled: is_enabled(env),
        totals: get_totals(env),
        today: get_day(env, day_of(env.ledger().timestamp())),
        insights: insights(env),
    }
}
