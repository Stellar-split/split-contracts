//! Batch verification helpers (issue #1176).
//!
//! Verifies many items against a shared verifier in one pass with early
//! exit, optional fail-fast, and bounded batch size. The verifier is a
//! caller-supplied closure so this module is independent of any particular
//! proof system. Heap free: callers pass the result buffer.
#![allow(dead_code)]

/// Errors returned by batch verification setup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifyError {
    /// The batch is empty.
    EmptyBatch,
    /// The batch is larger than the allowed maximum.
    BatchTooLarge,
    /// The result buffer is smaller than the batch.
    BufferTooSmall,
}

/// Summary of a batch verification run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerifyReport {
    /// Items actually checked (less than the batch size when fail-fast hit).
    pub checked: u32,
    /// Items that verified successfully.
    pub passed: u32,
    /// Items that failed verification.
    pub failed: u32,
}

impl VerifyReport {
    /// True when every item in a batch of `total` items was checked and passed.
    pub fn all_valid(&self, total: usize) -> bool {
        self.failed == 0 && self.passed as usize == total
    }
}

/// Verify `items` with `verify`, writing per-item results to `results`.
///
/// When `fail_fast` is true, processing stops at the first failure and the
/// remaining `results` entries are left untouched.
pub fn verify_batch<T, F>(
    items: &[T],
    max_batch: usize,
    fail_fast: bool,
    results: &mut [bool],
    mut verify: F,
) -> Result<VerifyReport, VerifyError>
where
    F: FnMut(&T) -> bool,
{
    if items.is_empty() {
        return Err(VerifyError::EmptyBatch);
    }
    if items.len() > max_batch {
        return Err(VerifyError::BatchTooLarge);
    }
    if results.len() < items.len() {
        return Err(VerifyError::BufferTooSmall);
    }
    let mut report = VerifyReport { checked: 0, passed: 0, failed: 0 };
    for (i, item) in items.iter().enumerate() {
        let ok = verify(item);
        results[i] = ok;
        report.checked += 1;
        if ok {
            report.passed += 1;
        } else {
            report.failed += 1;
            if fail_fast {
                break;
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_all_items() {
        let items = [2u32, 4, 6];
        let mut res = [false; 3];
        let r = verify_batch(&items, 10, false, &mut res, |x| x % 2 == 0).unwrap();
        assert!(r.all_valid(3));
        assert_eq!(res, [true, true, true]);
    }

    #[test]
    fn fail_fast_stops_early() {
        let items = [2u32, 3, 4];
        let mut res = [false; 3];
        let r = verify_batch(&items, 10, true, &mut res, |x| x % 2 == 0).unwrap();
        assert_eq!(r.checked, 2);
        assert_eq!(r.failed, 1);
        assert!(!r.all_valid(3));
    }

    #[test]
    fn continues_without_fail_fast() {
        let items = [2u32, 3, 4];
        let mut res = [false; 3];
        let r = verify_batch(&items, 10, false, &mut res, |x| x % 2 == 0).unwrap();
        assert_eq!((r.checked, r.passed, r.failed), (3, 2, 1));
    }

    #[test]
    fn rejects_bad_shapes() {
        let mut res = [false; 1];
        assert_eq!(
            verify_batch::<u32, _>(&[], 5, false, &mut res, |_| true),
            Err(VerifyError::EmptyBatch)
        );
        assert_eq!(
            verify_batch(&[1u32, 2], 1, false, &mut res, |_| true),
            Err(VerifyError::BatchTooLarge)
        );
        assert_eq!(
            verify_batch(&[1u32, 2], 5, false, &mut res, |_| true),
            Err(VerifyError::BufferTooSmall)
        );
    }
}
