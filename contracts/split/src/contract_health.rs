//! Issue #1187: contract health assessment over monitoring stats (first slice).
//!
//! Builds on the alerting system from issue #1033 (`monitoring`). [`delta`]
//! counts the alerts and failed deliveries raised between two
//! [`MonitoringStats`] snapshots, [`assess`] turns that window plus the current
//! unacknowledged backlog into a [`HealthStatus`] using configurable
//! [`HealthThresholds`], and [`HealthLog`] tracks the outcome of each check.

#![allow(dead_code)]

use crate::monitoring::MonitoringStats;

/// Overall contract health, ordered from best to worst.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HealthStatus {
    /// No threshold exceeded.
    Healthy,
    /// A warning-level threshold exceeded.
    Degraded,
    /// A critical-level threshold exceeded.
    Critical,
}

/// Configurable limits for one health check window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HealthThresholds {
    /// Unacknowledged alerts tolerated before the contract is degraded.
    pub max_unacknowledged: u64,
    /// New warning alerts tolerated in the window.
    pub max_new_warnings: u64,
    /// New failed deliveries tolerated in the window.
    pub max_failed_deliveries: u64,
    /// New critical alerts tolerated in the window.
    pub max_new_critical: u64,
}

impl Default for HealthThresholds {
    fn default() -> Self {
        HealthThresholds {
            max_unacknowledged: 10,
            max_new_warnings: 5,
            max_failed_deliveries: 0,
            max_new_critical: 0,
        }
    }
}

/// Alerts and delivery failures raised between two snapshots.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HealthDelta {
    /// New alerts of any severity.
    pub new_alerts: u64,
    /// New warning alerts.
    pub new_warnings: u64,
    /// New critical alerts.
    pub new_critical: u64,
    /// New failed deliveries.
    pub new_failed_deliveries: u64,
}

/// Result of one health check.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HealthReport {
    /// Overall status.
    pub status: HealthStatus,
    /// Activity in the checked window.
    pub delta: HealthDelta,
    /// Unacknowledged alerts at the end of the window.
    pub unacknowledged: u64,
}

/// Counts activity between `prev` and `cur` (saturating at zero if counters were reset).
pub fn delta(prev: &MonitoringStats, cur: &MonitoringStats) -> HealthDelta {
    HealthDelta {
        new_alerts: cur.total_alerts.saturating_sub(prev.total_alerts),
        new_warnings: cur.warning_alerts.saturating_sub(prev.warning_alerts),
        new_critical: cur.critical_alerts.saturating_sub(prev.critical_alerts),
        new_failed_deliveries: cur.failed_deliveries.saturating_sub(prev.failed_deliveries),
    }
}

/// Assesses health for the window from `prev` to `cur`.
pub fn assess(
    prev: &MonitoringStats,
    cur: &MonitoringStats,
    thresholds: &HealthThresholds,
) -> HealthReport {
    let d = delta(prev, cur);
    let status = if d.new_critical > thresholds.max_new_critical {
        HealthStatus::Critical
    } else if cur.unacknowledged > thresholds.max_unacknowledged
        || d.new_warnings > thresholds.max_new_warnings
        || d.new_failed_deliveries > thresholds.max_failed_deliveries
    {
        HealthStatus::Degraded
    } else {
        HealthStatus::Healthy
    };
    HealthReport { status, delta: d, unacknowledged: cur.unacknowledged }
}

/// Running record of health checks.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HealthLog {
    /// Checks recorded.
    pub checks: u64,
    /// Checks that ended degraded.
    pub degraded: u64,
    /// Checks that ended critical.
    pub critical: u64,
    /// Status of the most recent check.
    pub last_status: Option<HealthStatus>,
}

impl HealthLog {
    /// Records a report; returns true when the status changed from the previous check.
    pub fn record(&mut self, report: &HealthReport) -> bool {
        self.checks = self.checks.saturating_add(1);
        match report.status {
            HealthStatus::Degraded => self.degraded = self.degraded.saturating_add(1),
            HealthStatus::Critical => self.critical = self.critical.saturating_add(1),
            HealthStatus::Healthy => {}
        }
        let changed = self.last_status != Some(report.status);
        self.last_status = Some(report.status);
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(warn: u64, crit: u64, failed: u64, unacked: u64) -> MonitoringStats {
        MonitoringStats {
            total_alerts: warn + crit,
            warning_alerts: warn,
            critical_alerts: crit,
            failed_deliveries: failed,
            unacknowledged: unacked,
            ..Default::default()
        }
    }

    #[test]
    fn assess_uses_thresholds() {
        let t = HealthThresholds::default();
        let base = stats(2, 1, 0, 3);

        let quiet = assess(&base, &stats(3, 1, 0, 4), &t);
        assert_eq!(quiet.status, HealthStatus::Healthy);
        assert_eq!(quiet.delta.new_alerts, 1);

        assert_eq!(assess(&base, &stats(2, 2, 0, 4), &t).status, HealthStatus::Critical);
        assert_eq!(assess(&base, &stats(2, 1, 1, 3), &t).status, HealthStatus::Degraded);
        assert_eq!(assess(&base, &stats(2, 1, 0, 11), &t).status, HealthStatus::Degraded);
        assert_eq!(assess(&base, &stats(8, 1, 0, 3), &t).status, HealthStatus::Degraded);

        let lenient = HealthThresholds { max_new_critical: 1, ..t };
        assert_eq!(assess(&base, &stats(2, 2, 0, 4), &lenient).status, HealthStatus::Healthy);
    }

    #[test]
    fn delta_saturates_after_reset() {
        assert_eq!(delta(&stats(5, 5, 5, 0), &MonitoringStats::default()), HealthDelta::default());
    }

    #[test]
    fn log_tracks_checks_and_changes() {
        let t = HealthThresholds::default();
        let base = stats(0, 0, 0, 0);
        let mut log = HealthLog::default();
        assert!(log.record(&assess(&base, &base, &t)));
        assert!(!log.record(&assess(&base, &base, &t)));
        assert!(log.record(&assess(&base, &stats(0, 1, 0, 1), &t)));
        assert_eq!(log.checks, 3);
        assert_eq!(log.critical, 1);
        assert_eq!(log.degraded, 0);
        assert_eq!(log.last_status, Some(HealthStatus::Critical));
    }
}
