//! Contract state pruning (issue #1177).
//!
//! Retention-policy driven selection of prunable records, with a tombstone
//! summary (digest + metadata) kept so a pruned record can be recognised and
//! restored from an off-chain archive, plus counters to track pruning.
//! Heap free; callers pass buffers. Helper library only: it does not delete
//! contract storage itself.
#![allow(dead_code)]

/// Errors returned by pruning helpers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PruneError {
    /// The output buffer is too small.
    BufferTooSmall,
    /// The retention policy is invalid.
    InvalidPolicy,
    /// A restored record does not match its tombstone digest.
    DigestMismatch,
}

/// A record eligible for pruning decisions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub id: u64,
    /// Ledger timestamp of last update.
    pub updated_at: u64,
    /// True when the record is still active and must never be pruned.
    pub active: bool,
    /// Caller-computed digest of the record contents.
    pub digest: u64,
}

/// Retention policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetentionPolicy {
    /// Inactive records older than this many seconds may be pruned.
    pub max_age: u64,
    /// Newest inactive records to always keep, regardless of age.
    pub keep_latest: u32,
}

/// Summary left behind for a pruned record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tombstone {
    pub id: u64,
    pub digest: u64,
    pub pruned_at: u64,
}

/// Counters tracking pruning activity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PruneStats {
    pub runs: u32,
    pub pruned: u32,
    pub retained: u32,
}

/// Select prunable records and write their tombstones to `out`.
///
/// Active records are never pruned. Of the inactive records, the
/// `keep_latest` most recently updated are kept; others are pruned when
/// older than `max_age`. Returns the number of tombstones written.
pub fn select_prunable(
    records: &[Record],
    policy: &RetentionPolicy,
    now: u64,
    out: &mut [Tombstone],
    stats: &mut PruneStats,
) -> Result<usize, PruneError> {
    if policy.max_age == 0 {
        return Err(PruneError::InvalidPolicy);
    }
    let mut written = 0usize;
    let mut retained = 0u32;
    for r in records {
        let mut keep = r.active || now.saturating_sub(r.updated_at) <= policy.max_age;
        if !keep {
            // Rank among inactive records: count newer inactive ones.
            let newer = records
                .iter()
                .filter(|o| !o.active && (o.updated_at, o.id) > (r.updated_at, r.id))
                .count();
            keep = newer < policy.keep_latest as usize;
        }
        if keep {
            retained = retained.saturating_add(1);
            continue;
        }
        if written >= out.len() {
            return Err(PruneError::BufferTooSmall);
        }
        out[written] = Tombstone { id: r.id, digest: r.digest, pruned_at: now };
        written += 1;
    }
    stats.runs = stats.runs.saturating_add(1);
    stats.pruned = stats.pruned.saturating_add(written as u32);
    stats.retained = stats.retained.saturating_add(retained);
    Ok(written)
}

/// Verify an archived record restored for `tombstone` matches its digest.
pub fn verify_restore(tombstone: &Tombstone, restored: &Record) -> Result<(), PruneError> {
    if restored.id == tombstone.id && restored.digest == tombstone.digest {
        Ok(())
    } else {
        Err(PruneError::DigestMismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: u64, updated_at: u64, active: bool) -> Record {
        Record { id, updated_at, active, digest: id * 7 }
    }

    const POLICY: RetentionPolicy = RetentionPolicy { max_age: 100, keep_latest: 1 };

    #[test]
    fn prunes_old_inactive_but_keeps_active_and_latest() {
        let records = [rec(1, 0, false), rec(2, 10, false), rec(3, 20, true), rec(4, 900, false)];
        let mut out = [Tombstone { id: 0, digest: 0, pruned_at: 0 }; 4];
        let mut stats = PruneStats::default();
        let n = select_prunable(&records, &POLICY, 1000, &mut out, &mut stats).unwrap();
        // 4 is recent, 3 is active; 1 and 2 are old and not the latest inactive.
        assert_eq!(n, 2);
        assert_eq!(out[0].id, 1);
        assert_eq!(out[1].id, 2);
        assert_eq!((stats.runs, stats.pruned, stats.retained), (1, 2, 2));
    }

    #[test]
    fn keep_latest_protects_newest_inactive() {
        let records = [rec(1, 0, false), rec(2, 10, false)];
        let mut out = [Tombstone { id: 0, digest: 0, pruned_at: 0 }; 2];
        let mut stats = PruneStats::default();
        let n = select_prunable(&records, &POLICY, 1000, &mut out, &mut stats).unwrap();
        assert_eq!(n, 1);
        assert_eq!(out[0].id, 1);
    }

    #[test]
    fn rejects_invalid_policy_and_small_buffer() {
        let records = [rec(1, 0, false), rec(2, 1, false), rec(3, 2, false)];
        let mut out = [Tombstone { id: 0, digest: 0, pruned_at: 0 }; 1];
        let mut stats = PruneStats::default();
        let bad = RetentionPolicy { max_age: 0, keep_latest: 0 };
        assert_eq!(
            select_prunable(&records, &bad, 1000, &mut out, &mut stats),
            Err(PruneError::InvalidPolicy)
        );
        assert_eq!(
            select_prunable(&records, &POLICY, 1000, &mut out, &mut stats),
            Err(PruneError::BufferTooSmall)
        );
    }

    #[test]
    fn restore_checks_digest() {
        let t = Tombstone { id: 5, digest: 35, pruned_at: 1 };
        assert_eq!(verify_restore(&t, &rec(5, 0, false)), Ok(()));
        assert_eq!(verify_restore(&t, &rec(6, 0, false)), Err(PruneError::DigestMismatch));
    }
}
