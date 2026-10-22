//! Resource pooling (issue #1020).
//!
//! A bounded pool of reusable execution slots, e.g. for batch operations that
//! need a limited number of concurrent workers. The pool is fixed-size, heap
//! free and `no_std` friendly, and time is expressed in ledger sequence numbers.
//!
//! - **Initialization** — [`Pool::new`] validates the [`PoolConfig`] and
//!   creates `capacity` primary slots followed by `standby` failover slots.
//! - **Recycling** — idle slots older than `max_age` ledgers, or used
//!   `max_uses` times, are reset by [`Pool::recycle`] (also run on acquire).
//! - **Health checks** — a slot that fails `failure_threshold` times in a row
//!   becomes unhealthy; [`Pool::health_check`] reports and repairs idle ones.
//! - **Failover** — [`Pool::acquire`] falls back to standby slots when every
//!   healthy primary slot is busy or unhealthy.
//! - **Monitoring** — [`Pool::stats`] exposes cumulative counters.

/// Hard upper bound on the number of slots (primary + standby).
pub const MAX_POOL_SLOTS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoolError {
    /// `capacity` is zero or `capacity + standby` exceeds [`MAX_POOL_SLOTS`].
    InvalidConfig,
    /// Every usable slot is busy or unhealthy.
    Exhausted,
    /// The slot index is out of range.
    InvalidSlot,
    /// The slot is not currently in use.
    NotInUse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolConfig {
    /// Number of primary slots.
    pub capacity: usize,
    /// Number of standby slots used only for failover.
    pub standby: usize,
    /// Idle slots older than this many ledgers are recycled. `0` disables.
    pub max_age: u32,
    /// Slots used this many times are recycled. `0` disables.
    pub max_uses: u32,
    /// Consecutive failures before a slot is marked unhealthy. `0` disables.
    pub failure_threshold: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotState {
    Idle,
    InUse,
    Unhealthy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub state: SlotState,
    pub created_at: u32,
    pub uses: u32,
    pub failures: u32,
    pub standby: bool,
}

impl Slot {
    fn fresh(now: u32, standby: bool) -> Self {
        Slot { state: SlotState::Idle, created_at: now, uses: 0, failures: 0, standby }
    }
}

/// Cumulative pool counters for monitoring.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PoolStats {
    pub acquired: u32,
    pub released: u32,
    pub recycled: u32,
    pub failovers: u32,
    pub exhausted: u32,
    pub marked_unhealthy: u32,
}

/// Point-in-time view of slot states.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HealthReport {
    pub idle: u32,
    pub in_use: u32,
    pub unhealthy: u32,
    /// `true` when at least one healthy primary slot exists.
    pub healthy: bool,
}

pub struct Pool {
    config: PoolConfig,
    slots: [Slot; MAX_POOL_SLOTS],
    len: usize,
    stats: PoolStats,
}

impl Pool {
    pub fn new(config: PoolConfig, now: u32) -> Result<Self, PoolError> {
        let len = config.capacity + config.standby;
        if config.capacity == 0 || len > MAX_POOL_SLOTS {
            return Err(PoolError::InvalidConfig);
        }
        let mut slots = [Slot::fresh(now, false); MAX_POOL_SLOTS];
        for (i, slot) in slots.iter_mut().enumerate().take(len) {
            *slot = Slot::fresh(now, i >= config.capacity);
        }
        Ok(Pool { config, slots, len, stats: PoolStats::default() })
    }

    pub fn stats(&self) -> PoolStats {
        self.stats
    }

    pub fn slot(&self, index: usize) -> Option<Slot> {
        if index < self.len {
            Some(self.slots[index])
        } else {
            None
        }
    }

    fn needs_recycle(&self, slot: &Slot, now: u32) -> bool {
        let aged = self.config.max_age > 0 && now.saturating_sub(slot.created_at) >= self.config.max_age;
        let worn = self.config.max_uses > 0 && slot.uses >= self.config.max_uses;
        aged || worn
    }

    /// Reset idle slots that are too old or too used. Returns how many were recycled.
    pub fn recycle(&mut self, now: u32) -> u32 {
        let mut recycled = 0;
        for i in 0..self.len {
            let slot = self.slots[i];
            if slot.state == SlotState::Idle && self.needs_recycle(&slot, now) {
                self.slots[i] = Slot::fresh(now, slot.standby);
                recycled += 1;
            }
        }
        self.stats.recycled += recycled;
        recycled
    }

    /// Acquire a slot, preferring healthy primary slots and failing over to standby.
    pub fn acquire(&mut self, now: u32) -> Result<usize, PoolError> {
        self.recycle(now);

        for pass_standby in [false, true] {
            for i in 0..self.len {
                let slot = &mut self.slots[i];
                if slot.standby == pass_standby && slot.state == SlotState::Idle {
                    slot.state = SlotState::InUse;
                    self.stats.acquired += 1;
                    if pass_standby {
                        self.stats.failovers += 1;
                    }
                    return Ok(i);
                }
            }
        }

        self.stats.exhausted += 1;
        Err(PoolError::Exhausted)
    }

    /// Return a slot to the pool. `ok == false` records a failure against it.
    pub fn release(&mut self, index: usize, ok: bool) -> Result<(), PoolError> {
        if index >= self.len {
            return Err(PoolError::InvalidSlot);
        }
        let threshold = self.config.failure_threshold;
        let slot = &mut self.slots[index];
        if slot.state != SlotState::InUse {
            return Err(PoolError::NotInUse);
        }

        slot.uses += 1;
        if ok {
            slot.failures = 0;
            slot.state = SlotState::Idle;
        } else {
            slot.failures += 1;
            if threshold > 0 && slot.failures >= threshold {
                slot.state = SlotState::Unhealthy;
                self.stats.marked_unhealthy += 1;
            } else {
                slot.state = SlotState::Idle;
            }
        }
        self.stats.released += 1;
        Ok(())
    }

    /// Report slot health and repair unhealthy slots by replacing them.
    pub fn health_check(&mut self, now: u32) -> HealthReport {
        let mut report = HealthReport::default();
        for i in 0..self.len {
            if self.slots[i].state == SlotState::Unhealthy {
                let standby = self.slots[i].standby;
                self.slots[i] = Slot::fresh(now, standby);
                self.stats.recycled += 1;
            }
        }
        for i in 0..self.len {
            match self.slots[i].state {
                SlotState::Idle => report.idle += 1,
                SlotState::InUse => report.in_use += 1,
                SlotState::Unhealthy => report.unhealthy += 1,
            }
            if !self.slots[i].standby && self.slots[i].state != SlotState::Unhealthy {
                report.healthy = true;
            }
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> PoolConfig {
        PoolConfig { capacity: 2, standby: 1, max_age: 100, max_uses: 3, failure_threshold: 2 }
    }

    #[test]
    fn rejects_invalid_config() {
        let mut c = config();
        c.capacity = 0;
        assert_eq!(Pool::new(c, 0).err(), Some(PoolError::InvalidConfig));
        let mut c = config();
        c.capacity = MAX_POOL_SLOTS;
        c.standby = 1;
        assert_eq!(Pool::new(c, 0).err(), Some(PoolError::InvalidConfig));
    }

    #[test]
    fn acquires_primary_then_fails_over_then_exhausts() {
        let mut p = Pool::new(config(), 0).unwrap();
        assert_eq!(p.acquire(0), Ok(0));
        assert_eq!(p.acquire(0), Ok(1));
        assert_eq!(p.stats().failovers, 0);
        assert_eq!(p.acquire(0), Ok(2));
        assert_eq!(p.stats().failovers, 1);
        assert_eq!(p.acquire(0), Err(PoolError::Exhausted));
        assert_eq!(p.stats().exhausted, 1);
    }

    #[test]
    fn released_slot_is_reused() {
        let mut p = Pool::new(config(), 0).unwrap();
        let a = p.acquire(0).unwrap();
        p.release(a, true).unwrap();
        assert_eq!(p.acquire(0), Ok(a));
        assert_eq!(p.release(9, true), Err(PoolError::InvalidSlot));
        let b = p.acquire(0).unwrap();
        p.release(b, true).unwrap();
        assert_eq!(p.release(b, true), Err(PoolError::NotInUse));
    }

    #[test]
    fn recycles_aged_and_worn_slots() {
        let mut p = Pool::new(config(), 0).unwrap();
        // Wear slot 0 out: three uses reaches max_uses.
        for _ in 0..3 {
            let s = p.acquire(0).unwrap();
            assert_eq!(s, 0);
            p.release(s, true).unwrap();
        }
        assert_eq!(p.slot(0).unwrap().uses, 3);
        assert_eq!(p.recycle(1), 1);
        assert_eq!(p.slot(0).unwrap().uses, 0);

        // Age every idle slot out.
        assert_eq!(p.recycle(1_000), 3);
        assert_eq!(p.slot(1).unwrap().created_at, 1_000);
        assert_eq!(p.stats().recycled, 4);
    }

    #[test]
    fn failing_slot_is_marked_unhealthy_and_failed_over() {
        let mut p = Pool::new(config(), 0).unwrap();
        for _ in 0..2 {
            let s = p.acquire(0).unwrap();
            assert_eq!(s, 0);
            p.release(s, false).unwrap();
        }
        assert_eq!(p.slot(0).unwrap().state, SlotState::Unhealthy);
        assert_eq!(p.stats().marked_unhealthy, 1);

        // Slot 1 is next healthy primary, then standby takes over.
        assert_eq!(p.acquire(0), Ok(1));
        assert_eq!(p.acquire(0), Ok(2));
        assert_eq!(p.acquire(0), Err(PoolError::Exhausted));
    }

    #[test]
    fn health_check_repairs_unhealthy_slots() {
        let mut p = Pool::new(config(), 0).unwrap();
        for _ in 0..2 {
            let s = p.acquire(0).unwrap();
            p.release(s, false).unwrap();
        }
        let held = p.acquire(0).unwrap();
        assert_eq!(held, 1);

        let report = p.health_check(10);
        assert_eq!(report, HealthReport { idle: 2, in_use: 1, unhealthy: 0, healthy: true });
        assert_eq!(p.slot(0).unwrap().state, SlotState::Idle);
        assert_eq!(p.slot(0).unwrap().created_at, 10);
    }

    #[test]
    fn success_resets_failure_streak() {
        let mut p = Pool::new(config(), 0).unwrap();
        let s = p.acquire(0).unwrap();
        p.release(s, false).unwrap();
        let s = p.acquire(0).unwrap();
        p.release(s, true).unwrap();
        let s = p.acquire(0).unwrap();
        p.release(s, false).unwrap();
        assert_eq!(p.slot(0).unwrap().state, SlotState::Idle);
    }
}
