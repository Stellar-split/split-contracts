//! Issue #1179: Sharded contract architecture, first slice.
//!
//! Pure, storage-free helpers for deterministic shard routing and per-shard
//! statistics. Not yet wired into any contract entry point, no persistent
//! storage and no shard deployment/coordination is implemented here.

/// Maximum number of shards supported by this routing scheme.
pub const MAX_SHARDS: u32 = 256;

/// Errors from shard routing helpers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShardError {
    /// Shard count was zero or above [`MAX_SHARDS`].
    InvalidShardCount,
    /// Shard index is outside the configured shard count.
    ShardOutOfRange,
    /// A counter would overflow.
    Overflow,
}

/// Per-shard usage statistics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShardStats {
    /// Number of splits routed to the shard.
    pub split_count: u64,
    /// Total amount routed through the shard.
    pub total_amount: i128,
}

/// Mixes a 64-bit key so that sequential ids spread evenly (splitmix64 finalizer).
fn mix(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// Routes `key` (e.g. a split id) to a shard index in `0..shard_count`.
///
/// Deterministic: the same key and shard count always give the same shard.
pub fn shard_for_key(key: u64, shard_count: u32) -> Result<u32, ShardError> {
    if shard_count == 0 || shard_count > MAX_SHARDS {
        return Err(ShardError::InvalidShardCount);
    }
    Ok((mix(key) % shard_count as u64) as u32)
}

/// Records one routed split of `amount` against `shard` in `stats`.
///
/// `stats` must have one entry per shard; `shard` must be a valid index.
pub fn record_routed(stats: &mut [ShardStats], shard: u32, amount: i128) -> Result<(), ShardError> {
    let s = stats.get_mut(shard as usize).ok_or(ShardError::ShardOutOfRange)?;
    let count = s.split_count.checked_add(1).ok_or(ShardError::Overflow)?;
    let total = s.total_amount.checked_add(amount).ok_or(ShardError::Overflow)?;
    s.split_count = count;
    s.total_amount = total;
    Ok(())
}

/// Returns the index of the shard with the fewest routed splits, if any.
pub fn least_loaded_shard(stats: &[ShardStats]) -> Option<u32> {
    let mut best: Option<(usize, u64)> = None;
    for (i, s) in stats.iter().enumerate() {
        match best {
            Some((_, c)) if c <= s.split_count => {}
            _ => best = Some((i, s.split_count)),
        }
    }
    best.map(|(i, _)| i as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_is_deterministic_and_in_range() {
        for k in 0..1000u64 {
            let a = shard_for_key(k, 8).unwrap();
            assert_eq!(a, shard_for_key(k, 8).unwrap());
            assert!(a < 8);
        }
    }

    #[test]
    fn invalid_shard_count_rejected() {
        assert_eq!(shard_for_key(1, 0), Err(ShardError::InvalidShardCount));
        assert_eq!(shard_for_key(1, MAX_SHARDS + 1), Err(ShardError::InvalidShardCount));
    }

    #[test]
    fn stats_are_tracked_and_validated() {
        let mut stats = [ShardStats::default(); 3];
        record_routed(&mut stats, 1, 50).unwrap();
        record_routed(&mut stats, 1, 25).unwrap();
        assert_eq!(stats[1], ShardStats { split_count: 2, total_amount: 75 });
        assert_eq!(record_routed(&mut stats, 3, 1), Err(ShardError::ShardOutOfRange));
        assert_eq!(least_loaded_shard(&stats), Some(0));
        assert_eq!(least_loaded_shard(&[]), None);
    }
}
