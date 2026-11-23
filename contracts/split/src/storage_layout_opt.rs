//! Storage layout indexing and query helpers (issue #1175).
//!
//! Complements `storage_optimizer` with a sorted key index that supports
//! point lookups and range queries in O(log n), plus lightweight access
//! counters for tracking performance. Heap free: callers pass buffers.
//! This module is a helper library and is not yet wired into contract entry
//! points.
#![allow(dead_code)]

/// Errors returned by index operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexError {
    /// The output buffer cannot hold every entry.
    BufferTooSmall,
    /// Two entries share the same key.
    DuplicateKey,
}

/// A key to storage-slot mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndexEntry {
    pub key: u64,
    pub slot: u32,
}

/// Build a key-sorted index from `entries` into `out`, returning its length.
pub fn build_index(entries: &[IndexEntry], out: &mut [IndexEntry]) -> Result<usize, IndexError> {
    if out.len() < entries.len() {
        return Err(IndexError::BufferTooSmall);
    }
    let mut n = 0usize;
    for e in entries {
        // Insertion sort: find position, shift larger entries right.
        let mut pos = n;
        while pos > 0 && out[pos - 1].key > e.key {
            out[pos] = out[pos - 1];
            pos -= 1;
        }
        if pos > 0 && out[pos - 1].key == e.key {
            return Err(IndexError::DuplicateKey);
        }
        // A duplicate may also sit right after the insert position.
        if pos < n && out[pos].key == e.key {
            return Err(IndexError::DuplicateKey);
        }
        out[pos] = *e;
        n += 1;
    }
    Ok(n)
}

/// Look up the slot for `key` in a sorted index.
pub fn lookup(index: &[IndexEntry], key: u64) -> Option<u32> {
    match index.binary_search_by(|e| e.key.cmp(&key)) {
        Ok(i) => Some(index[i].slot),
        Err(_) => None,
    }
}

/// Return the sub-slice of a sorted index with `lo <= key <= hi`.
pub fn range_query(index: &[IndexEntry], lo: u64, hi: u64) -> &[IndexEntry] {
    if lo > hi {
        return &index[0..0];
    }
    let start = index.partition_point(|e| e.key < lo);
    let end = index.partition_point(|e| e.key <= hi);
    &index[start..end]
}

/// Counters for tracking storage access performance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AccessStats {
    pub reads: u32,
    pub hits: u32,
    pub writes: u32,
}

impl AccessStats {
    /// Record a read and whether it found an entry.
    pub fn record_read(&mut self, hit: bool) {
        self.reads = self.reads.saturating_add(1);
        if hit {
            self.hits = self.hits.saturating_add(1);
        }
    }

    /// Record a write.
    pub fn record_write(&mut self) {
        self.writes = self.writes.saturating_add(1);
    }

    /// Hit rate in basis points (0 when no reads were recorded).
    pub fn hit_rate_bps(&self) -> u32 {
        if self.reads == 0 {
            return 0;
        }
        ((self.hits as u64 * 10_000) / self.reads as u64) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(key: u64, slot: u32) -> IndexEntry {
        IndexEntry { key, slot }
    }

    #[test]
    fn builds_sorted_index_and_looks_up() {
        let input = [e(30, 3), e(10, 1), e(20, 2)];
        let mut out = [e(0, 0); 3];
        assert_eq!(build_index(&input, &mut out), Ok(3));
        assert_eq!(out[0].key, 10);
        assert_eq!(lookup(&out, 20), Some(2));
        assert_eq!(lookup(&out, 25), None);
    }

    #[test]
    fn rejects_duplicates_and_small_buffers() {
        let mut out = [e(0, 0); 2];
        assert_eq!(
            build_index(&[e(1, 1), e(1, 2)], &mut out),
            Err(IndexError::DuplicateKey)
        );
        assert_eq!(
            build_index(&[e(1, 1), e(2, 2), e(3, 3)], &mut out),
            Err(IndexError::BufferTooSmall)
        );
    }

    #[test]
    fn range_query_is_inclusive() {
        let input = [e(1, 1), e(5, 5), e(9, 9), e(7, 7)];
        let mut out = [e(0, 0); 4];
        let n = build_index(&input, &mut out).unwrap();
        let r = range_query(&out[..n], 5, 8);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].key, 5);
        assert_eq!(r[1].key, 7);
        assert!(range_query(&out[..n], 8, 5).is_empty());
    }

    #[test]
    fn stats_hit_rate() {
        let mut s = AccessStats::default();
        assert_eq!(s.hit_rate_bps(), 0);
        s.record_read(true);
        s.record_read(false);
        s.record_write();
        assert_eq!(s.hit_rate_bps(), 5_000);
        assert_eq!(s.writes, 1);
    }
}
