# Issue #1105: Error aggregation reporting (status)

**Status:** partly covered. On-chain monitoring and alerting exist, but they watch usage metrics, not
errors. Aggregating errors needs the transaction results that only an indexer sees.

## Why errors are not aggregated on chain

A call that fails reverts its writes, so a contract cannot count its own failures. The counters in
`analytics.rs` and `monitoring.rs` are updated by calls that succeed. Error grouping, trends and
reporting to a monitoring system belong in an off-chain service that reads transaction results and uses
the error codes in [`ERROR_CODES_SPLIT.md`](./ERROR_CODES_SPLIT.md) as the grouping key.

## What exists

| Requirement | Where it is in `contracts/split/src/lib.rs` |
|---|---|
| Aggregation system | `analytics.rs` keeps lifetime totals and one bucket per day (see [`adr/0003-observability-from-event-helpers.md`](./adr/0003-observability-from-event-helpers.md)). `get_usage_totals`, `get_usage_day`, `get_usage_report`, `get_usage_insights` and `get_usage_dashboard` read them. |
| Grouping | Usage is grouped by event kind (created, paid, refunded) and by day. Refunds are the closest thing to a failure signal: `MetricKind::DailyRefunds`, `DailyRefundVolume` and `RefundRateBps`. |
| Reporting API | `get_monitoring_dashboard()`, `get_metrics()` and `get_monitoring_stats()` |
| Alerting | `add_alert_rule`, `remove_alert_rule`, `set_alert_rule_enabled`, `set_alert_receiver`, `check_alerts`, `acknowledge_alert`, `get_alert` and `get_recent_alerts`. A rule compares a metric with a threshold and has a severity and a cooldown. |
| Error trends | Not present for errors. The daily buckets give trends for usage and refunds only. |
| Circuit breaker input | `report_circuit_failure(reporter, reason)` lets an operator or keeper that has seen failures feed them into the breaker, which counts them per window. |

The design is in [`ISSUE_1033_MONITORING_ALERTING.md`](./ISSUE_1033_MONITORING_ALERTING.md) and
[`ISSUE_1034_USAGE_ANALYTICS.md`](./ISSUE_1034_USAGE_ANALYTICS.md).

## What this change does

Records the status above. It changes no code. The behaviour was read, not run, and no tests were added.
