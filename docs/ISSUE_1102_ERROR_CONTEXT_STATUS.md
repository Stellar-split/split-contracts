# Issue #1102: Error context enrichment (status)

**Status:** partly covered. Context reaches a debugger through events and typed codes. A contract
cannot attach a stack trace to an error, and a failed call cannot store context at all.

## Why

A Soroban transaction that fails reverts its writes and its events. The error that comes back is a
single `Error(Contract, #code)` value or a panic message. Storage "context" written during a failing
call is discarded, so on-chain context queries and stack traces cannot be done. The debugging data has
to come from the error value, from events emitted by calls that succeed, and from off-chain simulation.

## Requirements against the code

| Requirement | Where it is |
|---|---|
| Context attached | Typed errors with a documented cause for each of the 82 codes, see [`ERROR_CODES_SPLIT.md`](./ERROR_CODES_SPLIT.md). Some failures carry a message, for example `RecipientMissingTrustline` names the address in the panic message. |
| Context on blocked operations | `events::guard_failure(env, invoice_id, guard, detail)` publishes `(split, grd_fail, invoice_id)` with `(guard, detail)` when a release guard blocks an operation without aborting the call (for example a missing co-signer approval, with the approval count as `detail`). |
| Context for state changes | `events::invoice_state_changed` and `events::invoice_transition` (topics `(split, inv_trans, invoice_id)`, data `(trigger, from, to, actor, amount)`) |
| Context queries | `get_audit_log(invoice_id)` for one invoice, `get_security_events()` for the bounded security log (`MAX_SECURITY_EVENTS` = 100) |
| Stack traces | Not possible on chain. Use the simulation diagnostics from the Soroban RPC, and the `release-with-logs` profile in the workspace `Cargo.toml` (release with debug assertions) when building for debugging. |

## Gaps

- Many failures are plain `panic!` or `assert!` messages with no code and no values.
- Failing `guard_failure` calls are only visible when the surrounding call succeeds.

## What this change does

Records the status above. It changes no code. The behaviour was read, not run, and no tests were added.
