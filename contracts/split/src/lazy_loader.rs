//! Lazy loading (issue #1018).
//!
//! A bounded read-through cache that loads values on demand, so only the
//! entries actually touched are held in memory. Heap free and `no_std`
//! friendly; time is expressed in ledger sequence numbers.
//!
//! - **On-demand loading** — [`LazyLoader::get_or_load`] calls the loader only
//!   on a miss.
//! - **Prefetching** — [`LazyLoader::prefetch`] warms keys that are about to be used.
//! - **Cache invalidation** — entries expire after `ttl` ledgers, and can be
//!   dropped with [`LazyLoader::invalidate`] / [`LazyLoader::invalidate_all`].
//! - **Memory bound** — at most `CAP` entries are held; the least recently
//!   used entry is evicted when full.
//! - **Monitoring** — [`LoadMetrics`] counts hits, misses, loads and evictions.

#[derive(Clone, Copy, Debug)]
struct Slot<V: Copy> {
    key: u64,
    value: V,
    loaded_at: u32,
    last_used: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoadMetrics {
    pub hits: u32,
    pub misses: u32,
    pub loads: u32,
    pub prefetched: u32,
    pub evictions: u32,
    pub invalidations: u32,
    pub expirations: u32,
}

impl LoadMetrics {
    /// Share of lookups served from memory, in basis points.
    pub fn hit_rate_bps(&self) -> u32 {
        let total = self.hits + self.misses;
        if total == 0 {
            0
        } else {
            (self.hits as u64 * 10_000 / total as u64) as u32
        }
    }
}

pub struct LazyLoader<V: Copy, const CAP: usize> {
    slots: [Option<Slot<V>>; CAP],
    /// Entries older than this many ledgers are reloaded. `0` disables expiry.
    ttl: u32,
    metrics: LoadMetrics,
}

impl<V: Copy, const CAP: usize> LazyLoader<V, CAP> {
    pub fn new(ttl: u32) -> Self {
        LazyLoader { slots: [None; CAP], ttl, metrics: LoadMetrics::default() }
    }

    pub fn metrics(&self) -> LoadMetrics {
        self.metrics
    }

    /// Number of entries currently held in memory.
    pub fn len(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn is_expired(&self, slot: &Slot<V>, now: u32) -> bool {
        self.ttl > 0 && now.saturating_sub(slot.loaded_at) >= self.ttl
    }

    fn find(&self, key: u64) -> Option<usize> {
        self.slots.iter().position(|s| matches!(s, Some(slot) if slot.key == key))
    }

    /// Index to store a new entry in: a free slot, else the least recently used.
    fn victim(&mut self) -> usize {
        if let Some(free) = self.slots.iter().position(|s| s.is_none()) {
            return free;
        }
        let mut oldest = 0;
        let mut oldest_used = u32::MAX;
        for (i, s) in self.slots.iter().enumerate() {
            if let Some(slot) = s {
                if slot.last_used < oldest_used {
                    oldest_used = slot.last_used;
                    oldest = i;
                }
            }
        }
        self.metrics.evictions += 1;
        oldest
    }

    fn store(&mut self, key: u64, value: V, now: u32) {
        let index = match self.find(key) {
            Some(i) => i,
            None => self.victim(),
        };
        self.slots[index] = Some(Slot { key, value, loaded_at: now, last_used: now });
    }

    /// Return the cached value, loading it with `loader` on a miss or after expiry.
    /// `None` from the loader means the value does not exist and nothing is cached.
    pub fn get_or_load<F>(&mut self, key: u64, now: u32, loader: F) -> Option<V>
    where
        F: FnOnce(u64) -> Option<V>,
    {
        if let Some(i) = self.find(key) {
            let slot = self.slots[i].expect("slot found above");
            if self.is_expired(&slot, now) {
                self.slots[i] = None;
                self.metrics.expirations += 1;
            } else {
                self.metrics.hits += 1;
                self.slots[i] = Some(Slot { last_used: now, ..slot });
                return Some(slot.value);
            }
        }

        self.metrics.misses += 1;
        let value = loader(key)?;
        self.metrics.loads += 1;
        self.store(key, value, now);
        Some(value)
    }

    /// Load every key in `keys` that is not already cached. Returns how many were loaded.
    pub fn prefetch<F>(&mut self, keys: &[u64], now: u32, mut loader: F) -> u32
    where
        F: FnMut(u64) -> Option<V>,
    {
        let mut loaded = 0;
        for &key in keys {
            let fresh = match self.find(key) {
                Some(i) => !self.is_expired(&self.slots[i].expect("slot found above"), now),
                None => false,
            };
            if fresh {
                continue;
            }
            if let Some(value) = loader(key) {
                self.store(key, value, now);
                self.metrics.loads += 1;
                self.metrics.prefetched += 1;
                loaded += 1;
            }
        }
        loaded
    }

    /// Drop one entry. Returns `true` if it was cached.
    pub fn invalidate(&mut self, key: u64) -> bool {
        match self.find(key) {
            Some(i) => {
                self.slots[i] = None;
                self.metrics.invalidations += 1;
                true
            }
            None => false,
        }
    }

    /// Drop every entry. Returns how many were cached.
    pub fn invalidate_all(&mut self) -> usize {
        let held = self.len();
        self.slots = [None; CAP];
        self.metrics.invalidations += held as u32;
        held
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(key: u64) -> Option<u64> {
        Some(key * key)
    }

    #[test]
    fn loads_on_demand_and_serves_hits_from_memory() {
        let mut cache: LazyLoader<u64, 4> = LazyLoader::new(0);
        let mut calls = 0;
        assert_eq!(cache.get_or_load(3, 1, |k| { calls += 1; square(k) }), Some(9));
        assert_eq!(cache.get_or_load(3, 2, |k| { calls += 1; square(k) }), Some(9));
        assert_eq!(calls, 1);
        assert_eq!(cache.len(), 1);
        let m = cache.metrics();
        assert_eq!((m.hits, m.misses, m.loads), (1, 1, 1));
        assert_eq!(m.hit_rate_bps(), 5_000);
    }

    #[test]
    fn missing_values_are_not_cached() {
        let mut cache: LazyLoader<u64, 2> = LazyLoader::new(0);
        assert_eq!(cache.get_or_load(1, 0, |_| None), None);
        assert!(cache.is_empty());
        assert_eq!(cache.metrics().loads, 0);
        assert_eq!(cache.metrics().misses, 1);
    }

    #[test]
    fn memory_is_bounded_and_evicts_least_recently_used() {
        let mut cache: LazyLoader<u64, 2> = LazyLoader::new(0);
        cache.get_or_load(1, 1, square);
        cache.get_or_load(2, 2, square);
        // Touch key 1 so key 2 becomes the least recently used.
        cache.get_or_load(1, 3, square);
        cache.get_or_load(3, 4, square);

        assert_eq!(cache.len(), 2);
        assert_eq!(cache.metrics().evictions, 1);
        let mut reloaded = false;
        cache.get_or_load(2, 5, |k| { reloaded = true; square(k) });
        assert!(reloaded, "key 2 should have been evicted");
    }

    #[test]
    fn entries_expire_after_ttl() {
        let mut cache: LazyLoader<u64, 2> = LazyLoader::new(10);
        cache.get_or_load(5, 100, square);
        let mut calls = 0;
        cache.get_or_load(5, 109, |k| { calls += 1; square(k) });
        assert_eq!(calls, 0);
        cache.get_or_load(5, 110, |k| { calls += 1; square(k) });
        assert_eq!(calls, 1);
        assert_eq!(cache.metrics().expirations, 1);
    }

    #[test]
    fn prefetch_warms_only_uncached_keys() {
        let mut cache: LazyLoader<u64, 4> = LazyLoader::new(0);
        cache.get_or_load(1, 0, square);
        let mut loaded_keys = 0;
        let n = cache.prefetch(&[1, 2, 3], 1, |k| { loaded_keys += 1; square(k) });
        assert_eq!(n, 2);
        assert_eq!(loaded_keys, 2);
        assert_eq!(cache.metrics().prefetched, 2);

        // Prefetched keys are now hits.
        assert_eq!(cache.get_or_load(2, 2, |_| None), Some(4));
        assert_eq!(cache.get_or_load(3, 2, |_| None), Some(9));
        assert_eq!(cache.metrics().hits, 2);
    }

    #[test]
    fn invalidation_forces_a_reload() {
        let mut cache: LazyLoader<u64, 4> = LazyLoader::new(0);
        cache.get_or_load(1, 0, square);
        cache.get_or_load(2, 0, square);
        assert!(cache.invalidate(1));
        assert!(!cache.invalidate(1));
        assert_eq!(cache.len(), 1);

        assert_eq!(cache.invalidate_all(), 1);
        assert!(cache.is_empty());
        assert_eq!(cache.metrics().invalidations, 2);

        let mut calls = 0;
        cache.get_or_load(2, 1, |k| { calls += 1; square(k) });
        assert_eq!(calls, 1);
    }

    #[test]
    fn hit_rate_is_zero_without_lookups() {
        assert_eq!(LoadMetrics::default().hit_rate_bps(), 0);
    }
}
