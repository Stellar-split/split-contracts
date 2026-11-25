//! Invoice event filtering and querying (issue #1219), first slice.
//!
//! Pure helpers that filter a bounded in-memory log of normalised invoice
//! events by event type, date range, and arbitrary predicate combinations
//! (complex queries), with `offset`/`limit` pagination and early-exit for
//! query performance. Not wired to contract storage or entrypoints; the
//! on-chain event stream is emitted by [`crate::events`] and would be
//! projected into [`InvoiceEvent`] by an indexer before being queried here.

#![allow(dead_code)]

/// Maximum events retained in one log.
pub const MAX_EVENTS: usize = 64;
/// Maximum results returned from a single query.
pub const MAX_RESULTS: usize = 64;

/// Normalised invoice event type, mirroring the `split` action topics emitted
/// by [`crate::events`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvoiceEventType {
    Created,
    Paid,
    Released,
    Refunded,
    Expired,
    Cancelled,
    Disputed,
    PartiallyReleased,
    Archived,
    Rated,
    Paused,
    Resumed,
    Other,
}

/// A single normalised invoice event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvoiceEvent {
    pub invoice_id: u64,
    pub event_type: InvoiceEventType,
    /// Unix timestamp of the event (ledger timestamp).
    pub timestamp: u64,
    pub ledger: u32,
    /// Per-invoice sequence number for stable ordering.
    pub seq: u64,
}

impl Default for InvoiceEvent {
    fn default() -> Self {
        Self {
            invoice_id: 0,
            event_type: InvoiceEventType::Other,
            timestamp: 0,
            ledger: 0,
            seq: 0,
        }
    }
}

/// Errors returned by the log and query helpers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterError {
    /// Log is at capacity and cannot accept another event.
    LogFull,
    /// Date range is inverted (`from > to`).
    InvalidRange,
}

/// A composite query: every set field is AND-ed together, so combining a
/// type, invoice id and date range yields a complex filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EventFilter {
    pub event_type: Option<InvoiceEventType>,
    pub invoice_id: Option<u64>,
    /// Inclusive lower timestamp bound.
    pub from_ts: Option<u64>,
    /// Inclusive upper timestamp bound.
    pub to_ts: Option<u64>,
    /// Skip the first `offset` matching events (pagination).
    pub offset: u32,
    /// Stop after collecting `limit` matches; `None` means no cap.
    pub limit: Option<u32>,
}

impl EventFilter {
    /// Convenience: filter by a single event type.
    pub fn by_type(t: InvoiceEventType) -> Self {
        Self {
            event_type: Some(t),
            ..Default::default()
        }
    }

    /// Convenience: filter by an inclusive date range. Returns
    /// [`FilterError::InvalidRange`] if `from > to`.
    pub fn by_date_range(from_ts: u64, to_ts: u64) -> Result<Self, FilterError> {
        if from_ts > to_ts {
            return Err(FilterError::InvalidRange);
        }
        Ok(Self {
            from_ts: Some(from_ts),
            to_ts: Some(to_ts),
            ..Default::default()
        })
    }

    /// Merge another filter's set fields into this one (logical AND of the
    /// combined predicates), taking the larger `offset`.
    pub fn and(mut self, other: &EventFilter) -> Self {
        if self.event_type.is_none() {
            self.event_type = other.event_type;
        }
        if self.invoice_id.is_none() {
            self.invoice_id = other.invoice_id;
        }
        if self.from_ts.is_none() {
            self.from_ts = other.from_ts;
        }
        if self.to_ts.is_none() {
            self.to_ts = other.to_ts;
        }
        if self.limit.is_none() {
            self.limit = other.limit;
        }
        self.offset = self.offset.max(other.offset);
        self
    }
}

/// Bounded result set returned from a query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilterResult {
    items: [InvoiceEvent; MAX_RESULTS],
    len: usize,
}

impl FilterResult {
    pub fn empty() -> Self {
        Self {
            items: [InvoiceEvent::default(); MAX_RESULTS],
            len: 0,
        }
    }
    /// The matched events, in log order.
    pub fn matched(&self) -> &[InvoiceEvent] {
        &self.items[..self.len]
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// Bounded in-memory log of normalised invoice events.
pub struct InvoiceEventLog {
    items: [InvoiceEvent; MAX_EVENTS],
    len: usize,
}

impl Default for InvoiceEventLog {
    fn default() -> Self {
        Self {
            items: [InvoiceEvent::default(); MAX_EVENTS],
            len: 0,
        }
    }
}

impl InvoiceEventLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn all(&self) -> &[InvoiceEvent] {
        &self.items[..self.len]
    }

    /// Append an event. Returns [`FilterError::LogFull`] at capacity.
    pub fn push(&mut self, e: InvoiceEvent) -> Result<(), FilterError> {
        if self.len >= MAX_EVENTS {
            return Err(FilterError::LogFull);
        }
        self.items[self.len] = e;
        self.len += 1;
        Ok(())
    }

    /// Does `e` satisfy every set field of `f`?
    pub fn matches(e: &InvoiceEvent, f: &EventFilter) -> bool {
        if let Some(t) = f.event_type {
            if e.event_type != t {
                return false;
            }
        }
        if let Some(id) = f.invoice_id {
            if e.invoice_id != id {
                return false;
            }
        }
        if let Some(from) = f.from_ts {
            if e.timestamp < from {
                return false;
            }
        }
        if let Some(to) = f.to_ts {
            if e.timestamp > to {
                return false;
            }
        }
        true
    }

    /// Count matching events without materialising them.
    pub fn count(&self, f: &EventFilter) -> u32 {
        let mut c = 0u32;
        for e in self.all() {
            if Self::matches(e, f) {
                c += 1;
            }
        }
        c
    }

    /// Collect matching events, applying `offset`/`limit` with early exit
    /// once the limit is reached, so a small `limit` over a large log does not
    /// scan the whole buffer.
    pub fn query(&self, f: &EventFilter) -> FilterResult {
        let mut res = FilterResult::empty();
        let cap = f.limit.map(|l| l as usize).unwrap_or(MAX_RESULTS);
        let cap = core::cmp::min(cap, MAX_RESULTS);
        let mut skipped = 0u32;
        for e in self.all() {
            if Self::matches(e, f) {
                if skipped < f.offset {
                    skipped += 1;
                    continue;
                }
                if res.len >= cap {
                    break;
                }
                res.items[res.len] = *e;
                res.len += 1;
            }
        }
        res
    }

    /// Shorthand: events of a single type.
    pub fn filter_by_type(&self, t: InvoiceEventType) -> FilterResult {
        self.query(&EventFilter::by_type(t))
    }

    /// Shorthand: events within an inclusive date range.
    pub fn filter_by_date_range(
        &self,
        from_ts: u64,
        to_ts: u64,
    ) -> Result<FilterResult, FilterError> {
        Ok(self.query(&EventFilter::by_date_range(from_ts, to_ts)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(id: u64, t: InvoiceEventType, ts: u64) -> InvoiceEvent {
        InvoiceEvent {
            invoice_id: id,
            event_type: t,
            timestamp: ts,
            ledger: (ts / 5) as u32,
            seq: ts,
        }
    }

    #[test]
    fn push_and_iterate() {
        let mut log = InvoiceEventLog::new();
        assert!(log.is_empty());
        assert!(log.push(ev(1, InvoiceEventType::Created, 100)).is_ok());
        assert!(log.push(ev(1, InvoiceEventType::Paid, 200)).is_ok());
        assert_eq!(log.len(), 2);
        assert_eq!(log.all().len(), 2);
        assert_eq!(log.all()[0].event_type, InvoiceEventType::Created);
    }

    #[test]
    fn filter_by_type_works() {
        let mut log = InvoiceEventLog::new();
        log.push(ev(1, InvoiceEventType::Created, 100)).unwrap();
        log.push(ev(1, InvoiceEventType::Paid, 110)).unwrap();
        log.push(ev(2, InvoiceEventType::Paid, 120)).unwrap();
        log.push(ev(1, InvoiceEventType::Released, 130)).unwrap();
        let paid = log.filter_by_type(InvoiceEventType::Paid);
        assert_eq!(paid.len(), 2);
        assert!(paid
            .matched()
            .iter()
            .all(|e| e.event_type == InvoiceEventType::Paid));
    }

    #[test]
    fn filter_by_date_range_works() {
        let mut log = InvoiceEventLog::new();
        log.push(ev(1, InvoiceEventType::Paid, 100)).unwrap();
        log.push(ev(1, InvoiceEventType::Paid, 150)).unwrap();
        log.push(ev(1, InvoiceEventType::Paid, 200)).unwrap();
        log.push(ev(1, InvoiceEventType::Paid, 250)).unwrap();
        let mid = log.filter_by_date_range(150, 200).unwrap();
        assert_eq!(mid.len(), 2);
        assert_eq!(mid.matched()[0].timestamp, 150);
        assert_eq!(mid.matched()[1].timestamp, 200);
        // Inverted range is rejected.
        assert_eq!(
            log.filter_by_date_range(200, 100),
            Err(FilterError::InvalidRange)
        );
    }

    #[test]
    fn complex_query_combines_predicates() {
        let mut log = InvoiceEventLog::new();
        log.push(ev(1, InvoiceEventType::Paid, 100)).unwrap();
        log.push(ev(2, InvoiceEventType::Paid, 110)).unwrap(); // other invoice
        log.push(ev(1, InvoiceEventType::Paid, 90)).unwrap(); // before range
        log.push(ev(1, InvoiceEventType::Refunded, 120)).unwrap(); // other type
        let f = EventFilter {
            event_type: Some(InvoiceEventType::Paid),
            invoice_id: Some(1),
            from_ts: Some(95),
            to_ts: Some(200),
            ..Default::default()
        };
        let r = log.query(&f);
        assert_eq!(r.len(), 1);
        assert_eq!(r.matched()[0].invoice_id, 1);
        assert_eq!(r.matched()[0].event_type, InvoiceEventType::Paid);
        assert_eq!(r.matched()[0].timestamp, 100);
        // count-only agrees with the materialised query.
        assert_eq!(log.count(&f), 1);
    }

    #[test]
    fn offset_and_limit_paginate() {
        let mut log = InvoiceEventLog::new();
        for i in 0..10u64 {
            log.push(ev(1, InvoiceEventType::Paid, 100 + i)).unwrap();
        }
        let page = log.query(&EventFilter {
            event_type: Some(InvoiceEventType::Paid),
            offset: 2,
            limit: Some(3),
            ..Default::default()
        });
        assert_eq!(page.len(), 3);
        assert_eq!(page.matched()[0].timestamp, 102);
        assert_eq!(page.matched()[2].timestamp, 104);
        assert_eq!(log.count(&EventFilter::by_type(InvoiceEventType::Paid)), 10);
    }

    #[test]
    fn limit_stops_early() {
        let mut log = InvoiceEventLog::new();
        for _ in 0..MAX_EVENTS {
            log.push(ev(1, InvoiceEventType::Paid, 100)).unwrap();
        }
        let r = log.query(&EventFilter {
            event_type: Some(InvoiceEventType::Paid),
            limit: Some(3),
            ..Default::default()
        });
        assert_eq!(r.len(), 3);
    }

    #[test]
    fn log_full_is_rejected() {
        let mut log = InvoiceEventLog::new();
        for _ in 0..MAX_EVENTS {
            log.push(ev(1, InvoiceEventType::Paid, 100)).unwrap();
        }
        assert_eq!(
            log.push(ev(1, InvoiceEventType::Paid, 100)),
            Err(FilterError::LogFull)
        );
    }
}
