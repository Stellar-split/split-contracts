# Issue #1033: Contract monitoring and alerting

**Status:** implemented in `contracts/split/src/monitoring.rs`. Builds on the usage analytics from
issue #1034.

## Design

| Requirement | How it is met |
|---|---|
| Monitoring system | Alert rules are checked against metrics read from the analytics aggregates. |
| Metric collection | `MetricKind`: invoices, payments, payment volume, refunds and refund volume for today, the lifetime refund rate (bps), and the amount of the payment being processed. `get_metrics()` returns the current value of each. |
| Alerting rules | Up to 20 `AlertRule`s: metric, `Above`/`Below`, threshold, severity (`Info`/`Warning`/`Critical`), cooldown and an enabled flag. |
| Dashboards | `get_monitoring_dashboard()` returns stats, rules, metric values, the 10 most recent alerts and the receiver. |
| Alert delivery | Each alert is stored, emitted as a `(split, alert, rule_id)` event for off-chain relayers, and sent to the optional receiver contract's `on_alert(alert)` through `try_invoke_contract`. |
| Monitoring analytics | `MonitoringStats`: evaluations, alerts by severity, unacknowledged alerts, failed deliveries and the time of the last alert. |

## Evaluation

- **On usage**: after each recorded usage event, only the enabled rules whose metric that event can
  change are evaluated. With no rules registered this costs one instance read.
- **On demand**: `check_alerts()` evaluates every rule and anyone can call it. A keeper uses it for
  conditions that no event triggers, such as "fewer than 1 payment today". `PaymentAmount` rules are
  skipped here because they only have a value while a payment is processed.
- A rule fires again only after `cooldown_secs` have passed since its last alert.

## Delivery guarantees

- A failing or missing receiver never reverts the payment, refund or creation that raised the alert.
  The alert records `delivered = Some(false)` and `failed_deliveries` is incremented.
- History keeps the 50 most recent alerts. Older alerts are removed as new ones arrive.
- If analytics recording is switched off, metrics stop changing and on-usage evaluation stops with it.

## Entry points

| Function | Access |
|---|---|
| `add_alert_rule(rule)`, `remove_alert_rule(id)`, `set_alert_rule_enabled(id, enabled)` | admin |
| `set_alert_receiver(receiver)` | admin |
| `acknowledge_alert(id)` | admin |
| `check_alerts()` | anyone |
| `get_alert_rules()`, `get_alert(id)`, `get_recent_alerts(limit)` | anyone |
| `get_metrics()`, `get_monitoring_stats()`, `get_monitoring_dashboard()` | anyone |

## Events

| Topics | Data |
|---|---|
| `(split, alert, rule_id)` | `(alert_id, metric, severity, value, threshold)` |
| `(split, alert_ack)` | `alert_id` |
| `(split, mon_add)` | `rule_id` |
| `(split, mon_rm)` | `rule_id` |

## Tests

`test_1033_*` in `contracts/split/src/test.rs`.
