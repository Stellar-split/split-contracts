//! Storage optimization (issue #1017).
//!
//! Helpers that shrink what a contract keeps in storage. Heap free and
//! `no_std` friendly; callers pass the buffers.
//!
//! - **Analysis** — [`analyze`] summarizes entry counts and sizes.
//! - **Compression** — [`encode_deltas`] / [`decode_deltas`] store a sorted
//!   sequence (for example ledger numbers) as varint-encoded gaps.
//! - **Deduplication** — [`dedupe_latest`] keeps one entry per key.
//! - **Cleanup** — [`cleanup_expired`] drops entries older than a TTL, and
//!   [`optimize`] runs cleanup plus deduplication in one pass.
//! - **Monitoring & metrics** — [`StorageMetrics`] records what was saved.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageError {
    /// The output buffer is too small.
    BufferTooSmall,
    /// The input sequence is not sorted ascending.
    NotSorted,
    /// The encoded input is truncated or malformed.
    InvalidEncoding,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub key: u64,
    pub created_at: u64,
    /// Stored size in bytes.
    pub size: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StorageProfile {
    pub entries: u32,
    pub total_bytes: u64,
    pub largest: u32,
    pub distinct_keys: u32,
}

/// Analyze storage usage patterns.
pub fn analyze(entries: &[Entry]) -> StorageProfile {
    let mut profile = StorageProfile::default();
    for (i, e) in entries.iter().enumerate() {
        profile.entries += 1;
        profile.total_bytes += e.size as u64;
        profile.largest = profile.largest.max(e.size);
        if !entries[..i].iter().any(|p| p.key == e.key) {
            profile.distinct_keys += 1;
        }
    }
    profile
}

fn write_varint(mut value: u32, out: &mut [u8], pos: &mut usize) -> Result<(), StorageError> {
    loop {
        if *pos >= out.len() {
            return Err(StorageError::BufferTooSmall);
        }
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out[*pos] = byte;
            *pos += 1;
            return Ok(());
        }
        out[*pos] = byte | 0x80;
        *pos += 1;
    }
}

fn read_varint(input: &[u8], pos: &mut usize) -> Result<u32, StorageError> {
    let mut value: u32 = 0;
    let mut shift = 0;
    loop {
        let byte = *input.get(*pos).ok_or(StorageError::InvalidEncoding)?;
        *pos += 1;
        if shift >= 32 || (shift == 28 && byte & 0x70 != 0) {
            return Err(StorageError::InvalidEncoding);
        }
        value |= ((byte & 0x7f) as u32) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
    }
}

/// Encode an ascending sequence as `[count][first][gap...]` varints.
/// Returns the number of bytes written.
pub fn encode_deltas(sorted: &[u32], out: &mut [u8]) -> Result<usize, StorageError> {
    let mut pos = 0;
    write_varint(sorted.len() as u32, out, &mut pos)?;
    let mut previous = 0u32;
    for (i, &value) in sorted.iter().enumerate() {
        if i > 0 && value < previous {
            return Err(StorageError::NotSorted);
        }
        write_varint(value - previous, out, &mut pos)?;
        previous = value;
    }
    Ok(pos)
}

/// Decode a sequence produced by [`encode_deltas`]. Returns the number of values written.
pub fn decode_deltas(input: &[u8], out: &mut [u32]) -> Result<usize, StorageError> {
    let mut pos = 0;
    let count = read_varint(input, &mut pos)? as usize;
    if out.len() < count {
        return Err(StorageError::BufferTooSmall);
    }
    let mut previous = 0u32;
    for slot in out.iter_mut().take(count) {
        let gap = read_varint(input, &mut pos)?;
        previous = previous.checked_add(gap).ok_or(StorageError::InvalidEncoding)?;
        *slot = previous;
    }
    if pos != input.len() {
        return Err(StorageError::InvalidEncoding);
    }
    Ok(count)
}

/// Keep only the newest entry per key, preserving order of the survivors.
/// Returns the new length; entries past it are unspecified.
pub fn dedupe_latest(entries: &mut [Entry]) -> usize {
    let mut kept = 0;
    for i in 0..entries.len() {
        let candidate = entries[i];
        let found = entries[..kept].iter().position(|e| e.key == candidate.key);
        match found {
            Some(existing) => {
                if candidate.created_at > entries[existing].created_at {
                    entries[existing] = candidate;
                }
            }
            None => {
                entries[kept] = candidate;
                kept += 1;
            }
        }
    }
    kept
}

/// Drop entries whose age is at least `ttl`. A `ttl` of zero keeps everything.
/// Returns the new length; entries past it are unspecified.
pub fn cleanup_expired(entries: &mut [Entry], now: u64, ttl: u64) -> usize {
    if ttl == 0 {
        return entries.len();
    }
    let mut kept = 0;
    for i in 0..entries.len() {
        let e = entries[i];
        if now.saturating_sub(e.created_at) < ttl {
            entries[kept] = e;
            kept += 1;
        }
    }
    kept
}

/// Cumulative storage savings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StorageMetrics {
    pub runs: u32,
    pub bytes_before: u64,
    pub bytes_after: u64,
    pub expired_removed: u32,
    pub duplicates_removed: u32,
}

impl StorageMetrics {
    /// Share of bytes saved, in basis points.
    pub fn savings_bps(&self) -> u32 {
        if self.bytes_before == 0 || self.bytes_after >= self.bytes_before {
            return 0;
        }
        ((self.bytes_before - self.bytes_after) * 10_000 / self.bytes_before) as u32
    }
}

/// Run cleanup then deduplication, record the result and return the new length.
pub fn optimize(entries: &mut [Entry], now: u64, ttl: u64, metrics: &mut StorageMetrics) -> usize {
    let before = analyze(entries);
    let live = cleanup_expired(entries, now, ttl);
    let unique = dedupe_latest(&mut entries[..live]);
    let after = analyze(&entries[..unique]);

    metrics.runs += 1;
    metrics.bytes_before += before.total_bytes;
    metrics.bytes_after += after.total_bytes;
    metrics.expired_removed += (entries.len() - live) as u32;
    metrics.duplicates_removed += (live - unique) as u32;
    unique
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: u64, created_at: u64, size: u32) -> Entry {
        Entry { key, created_at, size }
    }

    #[test]
    fn analyze_summarizes_usage() {
        let entries = [entry(1, 0, 10), entry(2, 0, 30), entry(1, 5, 20)];
        assert_eq!(
            analyze(&entries),
            StorageProfile { entries: 3, total_bytes: 60, largest: 30, distinct_keys: 2 }
        );
        assert_eq!(analyze(&[]), StorageProfile::default());
    }

    #[test]
    fn delta_encoding_roundtrips_and_is_smaller() {
        let ledgers: [u32; 8] = [1_000_000, 1_000_005, 1_000_006, 1_000_020, 1_000_021, 1_000_100, 1_000_101, 1_000_300];
        let mut buf = [0u8; 64];
        let n = encode_deltas(&ledgers, &mut buf).unwrap();
        assert!(n < ledgers.len() * 4, "encoded {n} bytes");

        let mut out = [0u32; 8];
        assert_eq!(decode_deltas(&buf[..n], &mut out), Ok(8));
        assert_eq!(out, ledgers);
    }

    #[test]
    fn delta_encoding_handles_empty_and_extremes() {
        let mut buf = [0u8; 16];
        let n = encode_deltas(&[], &mut buf).unwrap();
        let mut out = [0u32; 0];
        assert_eq!(decode_deltas(&buf[..n], &mut out), Ok(0));

        let extremes = [0u32, u32::MAX];
        let n = encode_deltas(&extremes, &mut buf).unwrap();
        let mut out = [0u32; 2];
        assert_eq!(decode_deltas(&buf[..n], &mut out), Ok(2));
        assert_eq!(out, extremes);
    }

    #[test]
    fn delta_encoding_reports_errors() {
        let mut buf = [0u8; 16];
        assert_eq!(encode_deltas(&[5, 3], &mut buf), Err(StorageError::NotSorted));
        let mut tiny = [0u8; 1];
        assert_eq!(encode_deltas(&[1, 2], &mut tiny), Err(StorageError::BufferTooSmall));

        let n = encode_deltas(&[1, 2, 3], &mut buf).unwrap();
        let mut small_out = [0u32; 2];
        assert_eq!(decode_deltas(&buf[..n], &mut small_out), Err(StorageError::BufferTooSmall));
        let mut out = [0u32; 3];
        assert_eq!(decode_deltas(&buf[..n - 1], &mut out), Err(StorageError::InvalidEncoding));
        assert_eq!(decode_deltas(&[], &mut out), Err(StorageError::InvalidEncoding));
        // Trailing garbage is rejected.
        let mut padded = [0u8; 8];
        padded[..n].copy_from_slice(&buf[..n]);
        assert_eq!(decode_deltas(&padded[..n + 1], &mut out), Err(StorageError::InvalidEncoding));
    }

    #[test]
    fn dedupe_keeps_newest_per_key() {
        let mut entries = [entry(1, 10, 5), entry(2, 10, 5), entry(1, 20, 7), entry(3, 5, 5), entry(2, 1, 9)];
        let n = dedupe_latest(&mut entries);
        assert_eq!(n, 3);
        assert_eq!(&entries[..n], &[entry(1, 20, 7), entry(2, 10, 5), entry(3, 5, 5)]);
    }

    #[test]
    fn cleanup_removes_only_expired_entries() {
        let mut entries = [entry(1, 0, 5), entry(2, 90, 5), entry(3, 40, 5)];
        let n = cleanup_expired(&mut entries, 100, 50);
        assert_eq!(n, 1);
        assert_eq!(entries[0], entry(2, 90, 5));

        let mut keep_all = [entry(1, 0, 5)];
        assert_eq!(cleanup_expired(&mut keep_all, 1_000, 0), 1);
    }

    #[test]
    fn optimize_runs_cleanup_and_dedup_and_records_metrics() {
        let mut entries = [
            entry(1, 0, 100),   // expired
            entry(2, 90, 100),
            entry(2, 95, 100),  // duplicate of key 2
            entry(3, 99, 100),
        ];
        let mut metrics = StorageMetrics::default();
        let n = optimize(&mut entries, 100, 50, &mut metrics);

        assert_eq!(n, 2);
        assert_eq!(metrics.runs, 1);
        assert_eq!(metrics.expired_removed, 1);
        assert_eq!(metrics.duplicates_removed, 1);
        assert_eq!(metrics.bytes_before, 400);
        assert_eq!(metrics.bytes_after, 200);
        assert_eq!(metrics.savings_bps(), 5_000);
        assert_eq!(StorageMetrics::default().savings_bps(), 0);
    }
}
