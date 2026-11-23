//! Issue #1190: performance profiling report helpers (first slice).
//!
//! Builds on the per-operation profiles recorded by `record_compute_usage`
//! (issue #924). [`metrics`] derives averages, spread, estimate drift and
//! accuracy from an [`OperationProfile`], [`insight`] classifies them into an
//! actionable [`Insight`], and [`PerfTracker`] aggregates a whole report
//! (operations seen, samples, flagged operations and the hottest operation).

#![allow(dead_code)]

use crate::types::OperationProfile;

/// Samples needed before an operation's metrics are trusted.
pub const MIN_SAMPLES: u32 = 5;
/// Spread (max - min) above this share of the average, in bps, is volatile.
pub const VOLATILE_SPREAD_BPS: u64 = 5_000;
/// Share of samples estimated within tolerance below which accuracy is low, in bps.
pub const LOW_ACCURACY_BPS: u64 = 8_000;

/// Derived metrics for one profiled operation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProfileMetrics {
    /// Recorded samples.
    pub samples: u32,
    /// Average measured instructions per call.
    pub avg_actual_insns: u64,
    /// Average raw (uncalibrated) estimate per call.
    pub avg_raw_estimated_insns: u64,
    /// (max - min) as a share of the average, in bps.
    pub spread_bps: u64,
    /// |avg actual - avg raw estimate| as a share of avg actual, in bps.
    pub drift_bps: u64,
    /// Share of samples estimated within tolerance, in bps.
    pub accuracy_bps: u64,
}

/// Actionable classification of an operation's metrics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Insight {
    /// Fewer than [`MIN_SAMPLES`] samples; record more usage.
    NeedsMoreSamples,
    /// Cost varies widely between calls; inspect input-size dependence.
    Volatile,
    /// Estimates often miss; review the operation's cost model.
    LowAccuracy,
    /// Nothing to act on.
    Healthy,
}

fn ratio_bps(part: u64, whole: u64) -> u64 {
    if whole == 0 {
        return 0;
    }
    ((part as u128) * 10_000 / (whole as u128)).min(u64::MAX as u128) as u64
}

/// Derives [`ProfileMetrics`] from a recorded profile.
pub fn metrics(profile: &OperationProfile) -> ProfileMetrics {
    if profile.samples == 0 {
        return ProfileMetrics::default();
    }
    let n = profile.samples as u64;
    let avg_actual = profile.total_actual_insns / n;
    let avg_raw = profile.total_raw_estimated_insns / n;
    let spread = profile.max_actual_insns.saturating_sub(profile.min_actual_insns);
    ProfileMetrics {
        samples: profile.samples,
        avg_actual_insns: avg_actual,
        avg_raw_estimated_insns: avg_raw,
        spread_bps: ratio_bps(spread, avg_actual),
        drift_bps: ratio_bps(avg_actual.abs_diff(avg_raw), avg_actual),
        accuracy_bps: ratio_bps(profile.within_tolerance as u64, n),
    }
}

/// Classifies metrics; sample count is checked first, then volatility, then accuracy.
pub fn insight(m: &ProfileMetrics) -> Insight {
    if m.samples < MIN_SAMPLES {
        Insight::NeedsMoreSamples
    } else if m.spread_bps > VOLATILE_SPREAD_BPS {
        Insight::Volatile
    } else if m.accuracy_bps < LOW_ACCURACY_BPS {
        Insight::LowAccuracy
    } else {
        Insight::Healthy
    }
}

/// Report-wide counters across profiled operations.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PerfTracker {
    /// Operations added to the report.
    pub operations: u32,
    /// Total samples across operations (saturating).
    pub total_samples: u64,
    /// Operations whose insight is not [`Insight::Healthy`].
    pub flagged: u32,
    /// Position (in add order) of the operation with the highest average cost.
    pub hottest: Option<u32>,
    /// Highest average cost seen.
    pub hottest_avg_insns: u64,
}

impl PerfTracker {
    /// Adds one operation's metrics to the report and returns its insight.
    pub fn add(&mut self, m: &ProfileMetrics) -> Insight {
        let found = insight(m);
        if self.hottest.is_none() || m.avg_actual_insns > self.hottest_avg_insns {
            self.hottest = Some(self.operations);
            self.hottest_avg_insns = m.avg_actual_insns;
        }
        self.operations = self.operations.saturating_add(1);
        self.total_samples = self.total_samples.saturating_add(m.samples as u64);
        if found != Insight::Healthy {
            self.flagged = self.flagged.saturating_add(1);
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::symbol_short;

    fn profile(
        samples: u32,
        actual: u64,
        raw: u64,
        min: u64,
        max: u64,
        ok: u32,
    ) -> OperationProfile {
        OperationProfile {
            operation: symbol_short!("pay"),
            samples,
            total_raw_estimated_insns: raw,
            total_actual_insns: actual,
            min_actual_insns: min,
            max_actual_insns: max,
            last_actual_insns: max,
            last_error_bps: 0,
            within_tolerance: ok,
        }
    }

    #[test]
    fn metrics_and_insights() {
        let m = metrics(&profile(10, 10_000, 9_000, 900, 1_100, 9));
        assert_eq!(m.avg_actual_insns, 1_000);
        assert_eq!(m.avg_raw_estimated_insns, 900);
        assert_eq!(m.spread_bps, 2_000);
        assert_eq!(m.drift_bps, 1_000);
        assert_eq!(m.accuracy_bps, 9_000);
        assert_eq!(insight(&m), Insight::Healthy);

        let few = metrics(&profile(2, 2_000, 2_000, 1_000, 1_000, 2));
        assert_eq!(insight(&few), Insight::NeedsMoreSamples);
        let volatile = metrics(&profile(5, 5_000, 5_000, 200, 2_000, 5));
        assert_eq!(insight(&volatile), Insight::Volatile);
        let inaccurate = metrics(&profile(5, 5_000, 5_000, 900, 1_100, 1));
        assert_eq!(insight(&inaccurate), Insight::LowAccuracy);
        let empty = metrics(&profile(0, 0, 0, u64::MAX, 0, 0));
        assert_eq!(empty, ProfileMetrics::default());
    }

    #[test]
    fn tracker_reports_hottest_and_flagged() {
        let mut t = PerfTracker::default();
        t.add(&metrics(&profile(10, 10_000, 10_000, 1_000, 1_000, 10)));
        t.add(&metrics(&profile(2, 8_000, 8_000, 4_000, 4_000, 2)));
        assert_eq!(t.operations, 2);
        assert_eq!(t.total_samples, 12);
        assert_eq!(t.flagged, 1);
        assert_eq!(t.hottest, Some(1));
        assert_eq!(t.hottest_avg_insns, 4_000);
    }
}
