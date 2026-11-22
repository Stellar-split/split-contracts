# Issue #1034: Contract usage analytics

**Status:** implemented in `contracts/split/src/analytics.rs`.

## Design

Analytics are kept on-chain as aggregates, so a dashboard can read current usage with one call instead
of replaying every event.

| Requirement | How it is met |
|---|---|
| Event tracking | `analytics::record` is called from the lifecycle event helpers in `events.rs` (`invoice_created`, `payment_received`, `invoice_released`, `invoice_refunded`, `invoice_refunded_with_reason`). Every code path that emits one of these events is counted, with no per-call-site changes. |
| Aggregation | Lifetime `UsageTotals` in instance storage, plus one `DailyUsage` bucket per UTC day in persistent storage. Unique creators and payers are counted with one marker entry per address. |
| Reporting | `get_usage_report(from_day, to_day)` returns one bucket per day, empty days included, for up to 90 days. |
| Dashboards | `get_usage_dashboard()` returns totals, today's bucket and insights together. |
| Insights | `get_usage_insights()` derives average payment and invoice size, payments per invoice, release and refund rates, the week-over-week payment trend and the busiest recent day. |

## Rules

- Recording never publishes events, so the existing event stream is unchanged.
- Counters saturate instead of failing, so analytics can never revert the operation being recorded.
- The admin can switch recording off with `set_analytics_enabled(false)`. Reads keep working on the data
  already collected.
- Day index = `ledger timestamp / 86_400` (UTC days).

## Entry points

| Function | Access |
|---|---|
| `set_analytics_enabled(enabled)` | admin |
| `is_analytics_enabled()` | anyone |
| `get_usage_totals()` | anyone |
| `get_usage_day(day)` | anyone |
| `get_usage_report(from_day, to_day)` | anyone |
| `get_usage_insights()` | anyone |
| `get_usage_dashboard()` | anyone |

## Cost

Each tracked event adds one instance write, one persistent write for the day bucket, and on the first
activity from an address one more persistent write for its marker.

## Tests

`test_1034_*` in `contracts/split/src/test.rs`.
