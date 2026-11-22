# Issue #1104: Circuit breaker pattern (status)

**Status:** covered. A contract-wide circuit breaker with a recovery phase, configuration, exemptions
and history exists (issues #297 and #923), next to the plain pause. The older
[`ISSUE_973_CIRCUIT_BREAKER_STATUS.md`](./ISSUE_973_CIRCUIT_BREAKER_STATUS.md) only lists what was left to
check. This note answers those checks from the code.

## Requirements against the code

| Requirement | Where it is in `contracts/split/src/lib.rs` |
|---|---|
| Circuit breaker design | A three-state machine, `Closed`, `Open` and `HalfOpen`, in [`ISSUE_923_CIRCUIT_BREAKER_STATE_MACHINE.md`](./ISSUE_923_CIRCUIT_BREAKER_STATE_MACHINE.md) |
| Pause logic | `activate_circuit_breaker(admin, reason)` opens it (SuperAdmin). `report_circuit_failure(reporter, reason)` (Operator or higher) opens it automatically once `failure_threshold` failures are reported within `failure_window_ledgers`, and a single failure in `HalfOpen` re-opens it. The older switches are `pause` and `unpause` for the whole contract, `pause_function` and `unpause_function` for one function, and `set_pause_exempt`. |
| Recovery mechanism | `begin_circuit_recovery(admin)` moves `Open` to `HalfOpen`. `complete_circuit_recovery(admin)` moves `HalfOpen` to `Closed` after `recovery_ledgers` have passed. `deactivate_circuit_breaker(admin)` closes it at once, skipping probation. |
| Monitoring | `get_circuit_breaker_status()` returns the state, reason, ledgers opened and recovery started, and the failure count in the current window. It is read-only and is not blocked by the breaker. Events: `circuit_breaker_state_changed` on every transition, plus `circuit_breaker_activated`, `circuit_breaker_deactivated` and `circuit_breaker_failure_reported`. |
| Breaker stats | `get_circuit_breaker_history()` returns the last 50 transitions, oldest first. `get_circuit_breaker_config()` returns the thresholds. |
| Operations that stay open | `set_circuit_breaker_exemption(admin, function, exempt)` and `get_circuit_breaker_exemptions()`, for example `refund`, so payers can still get money back while the breaker is open. |

## The open checks from issue #973

- **Does every state-changing entry point check the breaker?** Not verified by this note. The guard is
  by function name, and checking each of about 200 entry points was not done.
- **Do reads stay open?** Yes for the status and configuration reads, which are documented as not
  affected by the breaker.
- **Is the reset path admin-only and does it emit an event?** Yes. Activate, deactivate, begin and
  complete recovery all need SuperAdmin. Every transition goes through `transition_circuit_breaker`,
  which records it in the history and emits `circuit_breaker_state_changed` with the old and new state,
  the actor and the reason.

## What this change does

Records the status above. It changes no code. The behaviour was read, not run, and no tests were added.
