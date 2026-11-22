//! Payment schedule optimizer (issue #1163), first slice.
//!
//! Pure helpers that turn a total amount into an installment schedule.
//! Not wired to contract storage or entrypoints.

#![allow(dead_code)]

/// Maximum installments in one suggested schedule.
pub const MAX_INSTALLMENTS: usize = 12;

/// Errors returned by the optimizer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleError {
    /// Total amount is zero or negative.
    InvalidAmount,
    /// Count is zero or above [`MAX_INSTALLMENTS`], or interval is zero.
    InvalidConfig,
    /// Total is too small for every installment to meet `min_installment`.
    BelowMinimum,
    /// Ledger arithmetic overflowed.
    Overflow,
}

/// Customisation options for schedule generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScheduleConfig {
    /// Number of installments requested.
    pub count: u32,
    /// Ledgers between installments.
    pub interval_ledgers: u32,
    /// Smallest allowed installment amount (0 disables the check).
    pub min_installment: i128,
    /// When true, rounding remainder goes to the first installments;
    /// otherwise it goes to the last.
    pub front_load_remainder: bool,
}

/// One scheduled payment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Installment {
    /// Ledger at which the payment is due.
    pub due_ledger: u32,
    /// Amount due.
    pub amount: i128,
}

/// A suggested schedule (fixed capacity, no allocation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Schedule {
    items: [Installment; MAX_INSTALLMENTS],
    len: usize,
}

impl Schedule {
    /// Installments in due order.
    pub fn installments(&self) -> &[Installment] {
        &self.items[..self.len]
    }

    /// Sum of all installment amounts.
    pub fn total(&self) -> i128 {
        self.installments().iter().fold(0i128, |a, i| a.saturating_add(i.amount))
    }
}

/// Counters describing optimizer usage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OptimizerStats {
    /// Schedules successfully produced.
    pub schedules_built: u32,
    /// Requests rejected.
    pub rejected: u32,
}

/// Builds an even schedule starting at `start_ledger`, spreading any rounding
/// remainder one unit at a time per [`ScheduleConfig::front_load_remainder`].
pub fn suggest_schedule(
    total: i128,
    start_ledger: u32,
    cfg: &ScheduleConfig,
) -> Result<Schedule, ScheduleError> {
    if total <= 0 {
        return Err(ScheduleError::InvalidAmount);
    }
    let n = cfg.count as usize;
    if n == 0 || n > MAX_INSTALLMENTS || cfg.interval_ledgers == 0 {
        return Err(ScheduleError::InvalidConfig);
    }
    let base = total / n as i128;
    let rem = (total % n as i128) as usize;
    if base < cfg.min_installment || base == 0 {
        return Err(ScheduleError::BelowMinimum);
    }
    let mut items = [Installment { due_ledger: 0, amount: 0 }; MAX_INSTALLMENTS];
    for (i, slot) in items.iter_mut().enumerate().take(n) {
        let extra = if cfg.front_load_remainder { i < rem } else { i >= n - rem };
        let offset = cfg
            .interval_ledgers
            .checked_mul(i as u32)
            .ok_or(ScheduleError::Overflow)?;
        slot.due_ledger = start_ledger.checked_add(offset).ok_or(ScheduleError::Overflow)?;
        slot.amount = base + if extra { 1 } else { 0 };
    }
    Ok(Schedule { items, len: n })
}

/// Like [`suggest_schedule`] but records the outcome in `stats`.
pub fn suggest_tracked(
    total: i128,
    start_ledger: u32,
    cfg: &ScheduleConfig,
    stats: &mut OptimizerStats,
) -> Result<Schedule, ScheduleError> {
    let r = suggest_schedule(total, start_ledger, cfg);
    match r {
        Ok(_) => stats.schedules_built = stats.schedules_built.saturating_add(1),
        Err(_) => stats.rejected = stats.rejected.saturating_add(1),
    }
    r
}

/// Suggests the largest installment count (up to `max_count`) that keeps every
/// installment at or above `min_installment`; returns 0 if none fits.
pub fn suggest_count(total: i128, min_installment: i128, max_count: u32) -> u32 {
    if total <= 0 {
        return 0;
    }
    let cap = core::cmp::min(max_count as usize, MAX_INSTALLMENTS) as u32;
    let mut n = cap;
    while n > 0 {
        if total / n as i128 >= core::cmp::max(min_installment, 1) {
            return n;
        }
        n -= 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(count: u32, front: bool) -> ScheduleConfig {
        ScheduleConfig { count, interval_ledgers: 100, min_installment: 0, front_load_remainder: front }
    }

    #[test]
    fn splits_with_remainder_front_loaded() {
        let s = suggest_schedule(10, 1000, &cfg(3, true)).unwrap();
        let a: [i128; 3] = [s.installments()[0].amount, s.installments()[1].amount, s.installments()[2].amount];
        assert_eq!(a, [4, 3, 3]);
        assert_eq!(s.total(), 10);
        assert_eq!(s.installments()[2].due_ledger, 1200);
    }

    #[test]
    fn remainder_back_loaded() {
        let s = suggest_schedule(10, 0, &cfg(3, false)).unwrap();
        assert_eq!(s.installments()[0].amount, 3);
        assert_eq!(s.installments()[2].amount, 4);
        assert_eq!(s.total(), 10);
    }

    #[test]
    fn rejects_bad_input_and_tracks_stats() {
        let mut st = OptimizerStats::default();
        assert_eq!(suggest_tracked(0, 0, &cfg(3, true), &mut st), Err(ScheduleError::InvalidAmount));
        assert_eq!(suggest_tracked(10, 0, &cfg(0, true), &mut st), Err(ScheduleError::InvalidConfig));
        assert!(suggest_tracked(10, 0, &cfg(2, true), &mut st).is_ok());
        assert_eq!((st.schedules_built, st.rejected), (1, 2));
    }

    #[test]
    fn minimum_and_overflow() {
        let mut c = cfg(5, true);
        c.min_installment = 10;
        assert_eq!(suggest_schedule(20, 0, &c), Err(ScheduleError::BelowMinimum));
        assert_eq!(suggest_schedule(100, u32::MAX, &cfg(2, true)), Err(ScheduleError::Overflow));
    }

    #[test]
    fn suggests_count() {
        assert_eq!(suggest_count(100, 30, 12), 3);
        assert_eq!(suggest_count(10, 30, 12), 0);
        assert_eq!(suggest_count(1000, 1, 50), MAX_INSTALLMENTS as u32);
    }
}
