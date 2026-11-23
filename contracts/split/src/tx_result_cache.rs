//! Issue #1172: transaction result cache (first slice).
//!
//! A fixed-capacity, heap-free cache keyed by a 32-byte transaction id with
//! TTL expiry, explicit invalidation, oldest-entry eviction and hit/miss
//! statistics. Time is a caller-supplied `u64` (e.g. ledger timestamp).
//! It is a standalone helper and is not yet wired into any contract entry point.

/// Number of cache slots.
pub const CACHE_CAPACITY: usize = 16;

/// Cache entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheEntry {
    /// Transaction id.
    pub key: [u8; 32],
    /// Cached value (e.g. an amount or status code).
    pub value: i128,
    /// Insert time.
    pub inserted_at: u64,
    /// Time-to-live in the same unit as `now`.
    pub ttl: u64,
}

impl CacheEntry {
    fn is_expired(&self, now: u64) -> bool {
        now >= self.inserted_at.saturating_add(self.ttl)
    }
}

/// Cache statistics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// Lookups that returned a live entry.
    pub hits: u32,
    /// Lookups that found nothing or an expired entry.
    pub misses: u32,
    /// Entries removed by `invalidate` or evicted for space.
    pub evictions: u32,
}

impl CacheStats {
    /// Hit rate in basis points (0..=10_000); 0 when there were no lookups.
    pub fn hit_rate_bps(&self) -> u32 {
        let total = self.hits as u64 + self.misses as u64;
        if total == 0 {
            return 0;
        }
        ((self.hits as u64 * 10_000) / total) as u32
    }
}

/// Fixed-capacity TTL cache.
#[derive(Clone, Copy, Debug)]
pub struct TxResultCache {
    slots: [Option<CacheEntry>; CACHE_CAPACITY],
    stats: CacheStats,
}

impl Default for TxResultCache {
    fn default() -> Self {
        Self::new()
    }
}

impl TxResultCache {
    /// Creates an empty cache.
    pub fn new() -> Self {
        TxResultCache { slots: [None; CACHE_CAPACITY], stats: CacheStats::default() }
    }

    /// Inserts or replaces an entry. If full, expired entries are reused
    /// first, otherwise the oldest entry is evicted.
    pub fn put(&mut self, key: [u8; 32], value: i128, now: u64, ttl: u64) {
        let entry = CacheEntry { key, value, inserted_at: now, ttl };
        let mut target: Option<usize> = self
            .slots
            .iter()
            .position(|s| matches!(s, Some(e) if e.key == key));
        if target.is_none() {
            target = self.slots.iter().position(|s| s.is_none());
        }
        if target.is_none() {
            target = self
                .slots
                .iter()
                .position(|s| matches!(s, Some(e) if e.is_expired(now)));
        }
        let idx = match target {
            Some(i) => i,
            None => {
                let mut oldest = 0usize;
                let mut oldest_t = u64::MAX;
                for (i, s) in self.slots.iter().enumerate() {
                    if let Some(e) = s {
                        if e.inserted_at < oldest_t {
                            oldest_t = e.inserted_at;
                            oldest = i;
                        }
                    }
                }
                self.stats.evictions = self.stats.evictions.saturating_add(1);
                oldest
            }
        };
        self.slots[idx] = Some(entry);
    }

    /// Looks up a live entry, updating hit/miss stats. Expired entries are
    /// dropped and count as misses.
    pub fn get(&mut self, key: &[u8; 32], now: u64) -> Option<i128> {
        let idx = self
            .slots
            .iter()
            .position(|s| matches!(s, Some(e) if e.key == *key));
        if let Some(i) = idx {
            if let Some(e) = self.slots[i] {
                if !e.is_expired(now) {
                    self.stats.hits = self.stats.hits.saturating_add(1);
                    return Some(e.value);
                }
            }
            self.slots[i] = None;
        }
        self.stats.misses = self.stats.misses.saturating_add(1);
        None
    }

    /// Removes an entry; returns whether one was removed.
    pub fn invalidate(&mut self, key: &[u8; 32]) -> bool {
        let idx = self
            .slots
            .iter()
            .position(|s| matches!(s, Some(e) if e.key == *key));
        match idx {
            Some(i) => {
                self.slots[i] = None;
                self.stats.evictions = self.stats.evictions.saturating_add(1);
                true
            }
            None => false,
        }
    }

    /// Returns a copy of the current statistics.
    pub fn stats(&self) -> CacheStats {
        self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_ttl_and_invalidate() {
        let mut c = TxResultCache::new();
        let k = [7u8; 32];
        c.put(k, 42, 100, 10);
        assert_eq!(c.get(&k, 105), Some(42));
        assert_eq!(c.get(&k, 110), None);
        c.put(k, 1, 200, 50);
        assert!(c.invalidate(&k));
        assert!(!c.invalidate(&k));
        assert_eq!(c.get(&k, 201), None);
        let s = c.stats();
        assert_eq!((s.hits, s.misses, s.evictions), (1, 2, 1));
        assert_eq!(s.hit_rate_bps(), 3333);
    }

    #[test]
    fn evicts_oldest_when_full() {
        let mut c = TxResultCache::new();
        for i in 0..CACHE_CAPACITY {
            c.put([i as u8; 32], i as i128, i as u64, 1_000);
        }
        c.put([200u8; 32], 5, 50, 1_000);
        assert_eq!(c.stats().evictions, 1);
        assert_eq!(c.get(&[0u8; 32], 51), None);
        assert_eq!(c.get(&[200u8; 32], 51), Some(5));
    }
}
