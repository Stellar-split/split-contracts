# Issue #973: Circuit breaker pattern (status)

**Status:** the design is in [`ISSUE_297_CIRCUIT_BREAKER.md`](./ISSUE_297_CIRCUIT_BREAKER.md), and
[`ISSUE_923_CIRCUIT_BREAKER_STATE_MACHINE.md`](./ISSUE_923_CIRCUIT_BREAKER_STATE_MACHINE.md) describes
the state machine. This note records what is known. It is not a full review.

## What the design covers

- A contract-wide breaker an admin can trip to halt every state-changing operation at once.
- A state machine for the breaker, so the transitions are explicit and recoverable.

## What this change does

Adds this status note. It changes no code and adds no tests.

## What still needs checking

- Whether every state-mutating entry point checks the breaker before it writes. A missed check is the
  main risk of this design.
- Whether reads stay open while the breaker is tripped, as the design intends.
- Whether the reset path is admin-only and emits an event.

## Not done

No review of the implementation, no new tests.
