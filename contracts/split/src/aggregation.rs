//! Request aggregation (issue #1022).
//!
//! Collects amount-bearing requests into a bounded batch and coalesces
//! requests that target the same destination so a single downstream
//! operation (e.g. one token transfer) can serve many of them.
//!
//! - **Batching** — [`Aggregator::add`] queues up to [`MAX_BATCH`] requests.
//! - **Timeouts** — a batch is due once it is full or `window_ledgers` have
//!   passed since the first request ([`Aggregator::is_due`]); adding to a
//!   timed-out batch is refused until it is flushed.
//! - **Aggregation logic** — [`Aggregator::flush`] groups by target and sums
//!   amounts with overflow checks.
//! - **Result distribution** — [`Flush::group_of`] maps each request back to
//!   the group whose outcome applies to it.
//! - **Metrics** — [`AggregationMetrics`] reports how many operations the
//!   batching saved.

/// Maximum number of requests per batch.
pub const MAX_BATCH: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AggregationError {
    /// `window_ledgers` or `max_size` is zero, or `max_size` exceeds [`MAX_BATCH`].
    InvalidConfig,
    /// Amount is zero or negative.
    InvalidAmount,
    /// The batch is full or has timed out and must be flushed first.
    BatchClosed,
    /// Summing amounts overflowed `i128`.
    Overflow,
    /// There is nothing to flush.
    Empty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub id: u32,
    pub target: u32,
    pub amount: i128,
}

impl Request {
    const EMPTY: Request = Request { id: 0, target: 0, amount: 0 };
}

/// Combined amount for one target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Group {
    pub target: u32,
    pub total: i128,
    pub requests: u32,
}

impl Group {
    const EMPTY: Group = Group { target: 0, total: 0, requests: 0 };
}

/// Result of flushing a batch.
#[derive(Clone, Copy, Debug)]
pub struct Flush {
    groups: [Group; MAX_BATCH],
    group_count: usize,
    request_group: [u8; MAX_BATCH],
    request_count: usize,
    /// `true` when the flush was triggered by the window elapsing.
    pub timed_out: bool,
}

impl Flush {
    pub fn groups(&self) -> &[Group] {
        &self.groups[..self.group_count]
    }

    /// Index into [`Flush::groups`] whose outcome applies to request `index`
    /// (in the order requests were added).
    pub fn group_of(&self, index: usize) -> Option<usize> {
        if index < self.request_count {
            Some(self.request_group[index] as usize)
        } else {
            None
        }
    }

    pub fn request_count(&self) -> usize {
        self.request_count
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AggregationMetrics {
    pub requests: u32,
    pub batches: u32,
    pub groups: u32,
    pub timeouts: u32,
    pub rejected: u32,
}

impl AggregationMetrics {
    /// Downstream operations avoided by aggregating (`requests - groups`).
    pub fn operations_saved(&self) -> u32 {
        self.requests.saturating_sub(self.groups)
    }
}

pub struct Aggregator {
    window_ledgers: u32,
    max_size: usize,
    opened_at: u32,
    requests: [Request; MAX_BATCH],
    len: usize,
    metrics: AggregationMetrics,
}

impl Aggregator {
    pub fn new(window_ledgers: u32, max_size: usize) -> Result<Self, AggregationError> {
        if window_ledgers == 0 || max_size == 0 || max_size > MAX_BATCH {
            return Err(AggregationError::InvalidConfig);
        }
        Ok(Aggregator {
            window_ledgers,
            max_size,
            opened_at: 0,
            requests: [Request::EMPTY; MAX_BATCH],
            len: 0,
            metrics: AggregationMetrics::default(),
        })
    }

    pub fn metrics(&self) -> AggregationMetrics {
        self.metrics
    }

    pub fn pending(&self) -> usize {
        self.len
    }

    /// `true` when the batch is full or its window has elapsed.
    pub fn is_due(&self, now: u32) -> bool {
        self.len > 0 && (self.len >= self.max_size || self.window_elapsed(now))
    }

    fn window_elapsed(&self, now: u32) -> bool {
        self.len > 0 && now.saturating_sub(self.opened_at) >= self.window_ledgers
    }

    /// Queue a request. Returns the number of pending requests.
    pub fn add(&mut self, request: Request, now: u32) -> Result<usize, AggregationError> {
        if request.amount <= 0 {
            self.metrics.rejected += 1;
            return Err(AggregationError::InvalidAmount);
        }
        if self.len >= self.max_size || self.window_elapsed(now) {
            self.metrics.rejected += 1;
            return Err(AggregationError::BatchClosed);
        }
        if self.len == 0 {
            self.opened_at = now;
        }
        self.requests[self.len] = request;
        self.len += 1;
        self.metrics.requests += 1;
        Ok(self.len)
    }

    /// Coalesce pending requests by target and reset the batch.
    pub fn flush(&mut self, now: u32) -> Result<Flush, AggregationError> {
        if self.len == 0 {
            return Err(AggregationError::Empty);
        }
        let timed_out = self.window_elapsed(now) && self.len < self.max_size;

        let mut groups = [Group::EMPTY; MAX_BATCH];
        let mut group_count = 0usize;
        let mut request_group = [0u8; MAX_BATCH];

        for i in 0..self.len {
            let req = self.requests[i];
            let mut found = None;
            for (g, group) in groups.iter().enumerate().take(group_count) {
                if group.target == req.target {
                    found = Some(g);
                    break;
                }
            }
            let g = match found {
                Some(g) => g,
                None => {
                    groups[group_count] = Group { target: req.target, total: 0, requests: 0 };
                    group_count += 1;
                    group_count - 1
                }
            };
            groups[g].total = groups[g]
                .total
                .checked_add(req.amount)
                .ok_or(AggregationError::Overflow)?;
            groups[g].requests += 1;
            request_group[i] = g as u8;
        }

        let request_count = self.len;
        self.len = 0;
        self.metrics.batches += 1;
        self.metrics.groups += group_count as u32;
        if timed_out {
            self.metrics.timeouts += 1;
        }

        Ok(Flush { groups, group_count, request_group, request_count, timed_out })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(id: u32, target: u32, amount: i128) -> Request {
        Request { id, target, amount }
    }

    #[test]
    fn rejects_invalid_config_and_amounts() {
        assert_eq!(Aggregator::new(0, 4).err(), Some(AggregationError::InvalidConfig));
        assert_eq!(Aggregator::new(10, 0).err(), Some(AggregationError::InvalidConfig));
        assert_eq!(Aggregator::new(10, MAX_BATCH + 1).err(), Some(AggregationError::InvalidConfig));

        let mut a = Aggregator::new(10, 4).unwrap();
        assert_eq!(a.add(req(1, 1, 0), 0), Err(AggregationError::InvalidAmount));
        assert_eq!(a.add(req(1, 1, -5), 0), Err(AggregationError::InvalidAmount));
        assert_eq!(a.metrics().rejected, 2);
    }

    #[test]
    fn coalesces_same_target_and_distributes_results() {
        let mut a = Aggregator::new(10, 8).unwrap();
        a.add(req(1, 100, 10), 0).unwrap();
        a.add(req(2, 200, 5), 1).unwrap();
        a.add(req(3, 100, 7), 2).unwrap();
        assert_eq!(a.pending(), 3);

        let flush = a.flush(3).unwrap();
        assert_eq!(flush.groups().len(), 2);
        assert_eq!(flush.groups()[0], Group { target: 100, total: 17, requests: 2 });
        assert_eq!(flush.groups()[1], Group { target: 200, total: 5, requests: 1 });
        assert_eq!(flush.group_of(0), Some(0));
        assert_eq!(flush.group_of(1), Some(1));
        assert_eq!(flush.group_of(2), Some(0));
        assert_eq!(flush.group_of(3), None);
        assert!(!flush.timed_out);
        assert_eq!(a.pending(), 0);
        assert_eq!(a.metrics().operations_saved(), 1);
    }

    #[test]
    fn full_batch_is_due_and_refuses_more() {
        let mut a = Aggregator::new(100, 2).unwrap();
        a.add(req(1, 1, 1), 0).unwrap();
        assert!(!a.is_due(0));
        a.add(req(2, 2, 1), 0).unwrap();
        assert!(a.is_due(0));
        assert_eq!(a.add(req(3, 3, 1), 0), Err(AggregationError::BatchClosed));
        a.flush(0).unwrap();
        assert_eq!(a.add(req(3, 3, 1), 0), Ok(1));
    }

    #[test]
    fn window_timeout_is_respected() {
        let mut a = Aggregator::new(5, 8).unwrap();
        a.add(req(1, 1, 1), 10).unwrap();
        assert!(!a.is_due(14));
        assert!(a.is_due(15));
        assert_eq!(a.add(req(2, 1, 1), 15), Err(AggregationError::BatchClosed));

        let flush = a.flush(15).unwrap();
        assert!(flush.timed_out);
        assert_eq!(a.metrics().timeouts, 1);

        // A new batch opens a fresh window.
        a.add(req(3, 1, 1), 20).unwrap();
        assert!(!a.is_due(24));
    }

    #[test]
    fn empty_flush_is_an_error() {
        let mut a = Aggregator::new(5, 8).unwrap();
        assert_eq!(a.flush(0).err(), Some(AggregationError::Empty));
        assert!(!a.is_due(1_000));
    }

    #[test]
    fn overflow_is_reported_without_wrapping() {
        let mut a = Aggregator::new(5, 8).unwrap();
        a.add(req(1, 1, i128::MAX), 0).unwrap();
        a.add(req(2, 1, 1), 0).unwrap();
        assert_eq!(a.flush(0).err(), Some(AggregationError::Overflow));
    }

    #[test]
    fn metrics_accumulate_across_batches() {
        let mut a = Aggregator::new(5, 8).unwrap();
        for batch in 0..3u32 {
            for i in 0..4 {
                a.add(req(i, i % 2, 1), batch * 10).unwrap();
            }
            a.flush(batch * 10).unwrap();
        }
        let m = a.metrics();
        assert_eq!(m.requests, 12);
        assert_eq!(m.batches, 3);
        assert_eq!(m.groups, 6);
        assert_eq!(m.operations_saved(), 6);
    }
}
