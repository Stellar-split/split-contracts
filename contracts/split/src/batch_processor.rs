//! Batch transaction processing (issue #1016).
//!
//! Groups many payment-like items into validated batches that are processed
//! in fixed-size chunks, so per-call overhead is paid once per chunk instead of
//! once per item. Heap free and `no_std` friendly.
//!
//! - **Design** — [`BatchConfig`] bounds the batch, chunk and total amount.
//! - **Validation** — [`validate`] rejects empty or oversized batches, bad
//!   amounts, duplicate ids and totals that overflow or exceed the cap before
//!   anything is processed.
//! - **Error handling** — [`process`] keeps going after an item fails and
//!   records each failure in the [`BatchReport`] instead of aborting.
//! - **Monitoring & analytics** — [`BatchStats`] accumulates counters across
//!   batches and derives success rate, average batch size and calls saved.

/// Hard upper bound on items in one batch.
pub const MAX_BATCH_ITEMS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchError {
    /// `max_items`, `chunk_size` is zero or exceeds [`MAX_BATCH_ITEMS`].
    InvalidConfig,
    EmptyBatch,
    TooManyItems,
    /// An item amount is zero or negative.
    InvalidAmount,
    DuplicateId,
    /// Summing amounts overflowed `i128`.
    Overflow,
    /// Total amount is above `max_total`.
    TotalTooLarge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchConfig {
    pub max_items: usize,
    /// Items handled per chunk (one downstream call per chunk).
    pub chunk_size: usize,
    /// Maximum combined amount of a batch. `0` disables the cap.
    pub max_total: i128,
}

impl BatchConfig {
    fn check(&self) -> Result<(), BatchError> {
        if self.max_items == 0
            || self.max_items > MAX_BATCH_ITEMS
            || self.chunk_size == 0
            || self.chunk_size > MAX_BATCH_ITEMS
        {
            return Err(BatchError::InvalidConfig);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchItem {
    pub id: u32,
    pub amount: i128,
}

/// Validate a batch and return its total amount.
pub fn validate(config: &BatchConfig, items: &[BatchItem]) -> Result<i128, BatchError> {
    config.check()?;
    if items.is_empty() {
        return Err(BatchError::EmptyBatch);
    }
    if items.len() > config.max_items {
        return Err(BatchError::TooManyItems);
    }

    let mut total: i128 = 0;
    for (i, item) in items.iter().enumerate() {
        if item.amount <= 0 {
            return Err(BatchError::InvalidAmount);
        }
        if items[..i].iter().any(|other| other.id == item.id) {
            return Err(BatchError::DuplicateId);
        }
        total = total.checked_add(item.amount).ok_or(BatchError::Overflow)?;
    }
    if config.max_total > 0 && total > config.max_total {
        return Err(BatchError::TotalTooLarge);
    }
    Ok(total)
}

/// Number of chunks (downstream calls) needed for `items` items.
pub fn chunk_count(items: usize, chunk_size: usize) -> usize {
    if chunk_size == 0 {
        0
    } else {
        items.div_ceil(chunk_size)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemFailure {
    pub id: u32,
    /// Handler-defined error code.
    pub code: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct BatchReport {
    pub total_amount: i128,
    pub succeeded: u32,
    pub failed: u32,
    pub chunks: u32,
    failures: [ItemFailure; MAX_BATCH_ITEMS],
    failure_count: usize,
}

impl BatchReport {
    pub fn failures(&self) -> &[ItemFailure] {
        &self.failures[..self.failure_count]
    }
}

/// Cumulative counters across batches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BatchStats {
    pub batches: u32,
    pub rejected_batches: u32,
    pub items: u32,
    pub succeeded: u32,
    pub failed: u32,
    pub chunks: u32,
}

impl BatchStats {
    /// Share of processed items that succeeded, in basis points.
    pub fn success_rate_bps(&self) -> u32 {
        if self.items == 0 {
            return 0;
        }
        (self.succeeded as u64 * 10_000 / self.items as u64) as u32
    }

    pub fn average_batch_size(&self) -> u32 {
        if self.batches == 0 {
            0
        } else {
            self.items / self.batches
        }
    }

    /// Downstream calls avoided versus processing every item individually.
    pub fn calls_saved(&self) -> u32 {
        self.items.saturating_sub(self.chunks)
    }
}

/// Validate and process `items`, calling `handler` for every item.
///
/// `handler` returns `Err(code)` to fail one item without aborting the batch.
pub fn process<F>(
    config: &BatchConfig,
    items: &[BatchItem],
    stats: &mut BatchStats,
    mut handler: F,
) -> Result<BatchReport, BatchError>
where
    F: FnMut(&BatchItem) -> Result<(), u32>,
{
    let total_amount = match validate(config, items) {
        Ok(total) => total,
        Err(e) => {
            stats.rejected_batches += 1;
            return Err(e);
        }
    };

    let mut report = BatchReport {
        total_amount,
        succeeded: 0,
        failed: 0,
        chunks: chunk_count(items.len(), config.chunk_size) as u32,
        failures: [ItemFailure { id: 0, code: 0 }; MAX_BATCH_ITEMS],
        failure_count: 0,
    };

    for chunk in items.chunks(config.chunk_size) {
        for item in chunk {
            match handler(item) {
                Ok(()) => report.succeeded += 1,
                Err(code) => {
                    report.failed += 1;
                    report.failures[report.failure_count] = ItemFailure { id: item.id, code };
                    report.failure_count += 1;
                }
            }
        }
    }

    stats.batches += 1;
    stats.items += items.len() as u32;
    stats.succeeded += report.succeeded;
    stats.failed += report.failed;
    stats.chunks += report.chunks;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> BatchConfig {
        BatchConfig { max_items: 10, chunk_size: 4, max_total: 1_000 }
    }

    fn item(id: u32, amount: i128) -> BatchItem {
        BatchItem { id, amount }
    }

    #[test]
    fn validation_rejects_bad_batches() {
        let c = config();
        assert_eq!(validate(&c, &[]), Err(BatchError::EmptyBatch));
        assert_eq!(validate(&c, &[item(1, 0)]), Err(BatchError::InvalidAmount));
        assert_eq!(validate(&c, &[item(1, -4)]), Err(BatchError::InvalidAmount));
        assert_eq!(validate(&c, &[item(1, 5), item(1, 6)]), Err(BatchError::DuplicateId));
        assert_eq!(validate(&c, &[item(1, 600), item(2, 401)]), Err(BatchError::TotalTooLarge));

        let many = [item(0, 1); 11];
        assert_eq!(validate(&c, &many), Err(BatchError::TooManyItems));

        let uncapped = BatchConfig { max_total: 0, ..c };
        assert_eq!(
            validate(&uncapped, &[item(1, i128::MAX), item(2, 1)]),
            Err(BatchError::Overflow)
        );
    }

    #[test]
    fn validation_rejects_invalid_config() {
        let zero_chunk = BatchConfig { chunk_size: 0, ..config() };
        assert_eq!(validate(&zero_chunk, &[item(1, 1)]), Err(BatchError::InvalidConfig));
        let huge = BatchConfig { max_items: MAX_BATCH_ITEMS + 1, ..config() };
        assert_eq!(validate(&huge, &[item(1, 1)]), Err(BatchError::InvalidConfig));
    }

    #[test]
    fn valid_batch_returns_total() {
        assert_eq!(validate(&config(), &[item(1, 10), item(2, 20), item(3, 30)]), Ok(60));
    }

    #[test]
    fn chunk_count_rounds_up() {
        assert_eq!(chunk_count(0, 4), 0);
        assert_eq!(chunk_count(1, 4), 1);
        assert_eq!(chunk_count(4, 4), 1);
        assert_eq!(chunk_count(5, 4), 2);
        assert_eq!(chunk_count(9, 0), 0);
    }

    #[test]
    fn failures_are_recorded_without_aborting() {
        let mut stats = BatchStats::default();
        let items = [item(1, 10), item(2, 20), item(3, 30), item(4, 40), item(5, 50)];
        let mut seen = 0;
        let report = process(&config(), &items, &mut stats, |it| {
            seen += 1;
            if it.id % 2 == 0 {
                Err(it.id * 100)
            } else {
                Ok(())
            }
        })
        .unwrap();

        assert_eq!(seen, 5);
        assert_eq!(report.total_amount, 150);
        assert_eq!(report.succeeded, 3);
        assert_eq!(report.failed, 2);
        assert_eq!(report.chunks, 2);
        assert_eq!(
            report.failures(),
            &[ItemFailure { id: 2, code: 200 }, ItemFailure { id: 4, code: 400 }]
        );
    }

    #[test]
    fn rejected_batch_runs_no_handler_and_is_counted() {
        let mut stats = BatchStats::default();
        let mut called = false;
        let err = process(&config(), &[item(1, 0)], &mut stats, |_| {
            called = true;
            Ok(())
        })
        .err();
        assert_eq!(err, Some(BatchError::InvalidAmount));
        assert!(!called);
        assert_eq!(stats.rejected_batches, 1);
        assert_eq!(stats.batches, 0);
    }

    #[test]
    fn stats_derive_analytics() {
        let mut stats = BatchStats::default();
        assert_eq!(stats.success_rate_bps(), 0);
        assert_eq!(stats.average_batch_size(), 0);

        let first = [item(1, 1), item(2, 1), item(3, 1), item(4, 1), item(5, 1), item(6, 1)];
        process(&config(), &first, &mut stats, |_| Ok(())).unwrap();
        let second = [item(1, 1), item(2, 1)];
        process(&config(), &second, &mut stats, |it| if it.id == 2 { Err(1) } else { Ok(()) }).unwrap();

        assert_eq!(stats.batches, 2);
        assert_eq!(stats.items, 8);
        assert_eq!(stats.succeeded, 7);
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.chunks, 3);
        assert_eq!(stats.success_rate_bps(), 8_750);
        assert_eq!(stats.average_batch_size(), 4);
        assert_eq!(stats.calls_saved(), 5);
    }
}
