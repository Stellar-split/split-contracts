# ADR 0005: Deliver alerts as events plus an optional receiver contract

- **Status:** accepted (issue #1033)
- **Context:** Soroban contracts cannot make HTTP calls, so alerts cannot be sent to a pager or chat
  directly. Alerts are often raised during a payment, which must not fail because of monitoring.

## Decision

- Every alert is stored in a bounded history (newest 50) and emitted as a `(split, alert, rule_id)`
  event that off-chain relayers can forward anywhere.
- When the admin sets a receiver contract, the alert is also passed to its `on_alert(alert)` function
  with `try_invoke_contract`. The result is recorded on the alert (`delivered`), and failures are
  counted in `MonitoringStats.failed_deliveries`.
- Rules have a cooldown so a sustained breach does not raise an alert on every call.
- `check_alerts` is open to anyone, so a keeper can evaluate rules that no event triggers.

## Consequences

- A broken or malicious receiver cannot revert the payment, refund or creation that raised the alert.
- Off-chain delivery is at-least-once via the event stream. Relayers deduplicate by alert id.
- Alerts older than the newest 50 are removed from storage but remain in the event history.
- The receiver call adds cost only when an alert fires.

## Alternatives considered

- **Events only.** Cheapest, but on-chain automation (for example auto-pausing) could not react.
- **Reuse the webhook subsystem.** Its retry ledger is more than alerts need, and the `webhooks`
  module is not currently compiled into the contract.
- **Required receiver call.** Simpler, but lets a failing receiver block payments.
