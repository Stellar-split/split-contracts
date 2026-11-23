# Issue #1099: Error recovery strategies (status)

**Status:** partly covered. The contract has recovery paths for the transient failures it can observe,
but a contract cannot retry its own failed transaction, so "automatic recovery" is a keeper or client job.

## What a contract can and cannot recover

A Soroban transaction that fails reverts all its writes. The contract therefore cannot record a failure
of the call that failed, or retry it. It can only record a failure it caught itself, for example a token
transfer made with `try_invoke_contract`, and then offer a later call that retries the step.

## Requirements against the code

| Requirement | Where it is | Notes |
|---|---|---|
| Recovery logic | `retry_failed_payout(invoice_id, recipient)` re-checks that the recipient account now exists, then pays and removes it from the failed list. `claim_pending_payout`, `claim_delayed_payout` and `claim_fallback` let a recipient collect a payout that was not sent at release. | Present. |
| Retry mechanism | Release records a payout it could not send with `record_failed_payout` (reason `"TransferFailed"`) and carries on with the other recipients. `retry_webhook_delivery(delivery_id)` retries a delivery, and is callable by anyone once its backoff has passed. | Present. |
| Backoff strategy | `webhooks::backoff_ledgers(attempts)` doubles from `RETRY_BASE_LEDGERS` (12) up to `RETRY_MAX_BACKOFF_LEDGERS` (720), for at most `MAX_DELIVERY_ATTEMPTS` (5). A failed payout has no backoff: it can be retried any time. | Webhooks only. |
| Recovery hooks | `emergency_pause.rs` has a recovery plan with steps and subscriber notification, and `begin_circuit_recovery` and `complete_circuit_recovery` run the circuit breaker's half-open phase. Events `payout_failed` and `circuit_breaker_failure_reported` let a watcher react. | The `emergency_pause` module is not called by `lib.rs`. The circuit breaker is. |
| Recovery stats | `get_circuit_breaker_history`, webhook delivery `attempts`, and the archival and batching counters | No single recovery statistics view. |

## Related

- The recovery text for each error code is in [`ERROR_CODES_SPLIT.md`](./ERROR_CODES_SPLIT.md).
- The circuit breaker state machine is in
  [`ISSUE_923_CIRCUIT_BREAKER_STATE_MACHINE.md`](./ISSUE_923_CIRCUIT_BREAKER_STATE_MACHINE.md).

## What this change does

Records the status above. It changes no code. The behaviour was read, not run, and no tests were added.
