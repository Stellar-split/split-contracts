# Issue #1140: Invoice state machine validation

**Status:** core transition logic implemented; validation coverage is partial.

---

## Goal

Harden the invoice lifecycle state machine so that every entry point that
changes invoice status checks the transition against a single authoritative
guard before writing, unreachable states are tested exhaustively, and the
validation rules are documented in one place.

---

## Background: existing implementation

`lib.rs` already contains two relevant functions:

```rust
pub(crate) fn valid_transition(from: InvoiceStatus, to: InvoiceStatus) -> bool
pub(crate) fn transition_status(env, invoice_id, invoice, to, actor)
```

`valid_transition` encodes the allowed moves:

| From | Allowed moves to |
|---|---|
| `Pending` | `PartiallyReleased`, `Released`, `Disputed`, `Cancelled`, `Expired`, `Refunded` |
| `PartiallyReleased` | `Released`, `Pending` (via dispute resolution) |
| `Disputed` | `Pending`, `Refunded` |
| `PayoutInProgress` | `Released`, `Refunded` |
| `Released` | — (terminal) |
| `Refunded` | — (terminal) |
| `Cancelled` | — (terminal) |
| `Expired` | — (terminal) |
| `Finalised` | — (terminal) |
| `Deleted` | — (terminal) |

Same-state transitions are always allowed (idempotent).

`transition_status` calls `valid_transition` and panics with
`ContractError::InvalidStateTransition` if the guard fails. It also emits the
`invoice_state_changed` event on every accepted move.

`state_machine_tests.rs` already tests four transitions and six guards against
terminal states, with a coverage counter, but it only exercises the `Pending`,
`Released`, `Cancelled`, and `Refunded` states.

---

## What is missing

### 1. Inconsistent guard usage

Several entry points that change invoice status call `transition_status`, but
some — particularly `refund`, the auto-release path inside `_pay`, and the
dispute resolution path — set `invoice.status` directly without going through
`transition_status`. This means those paths bypass the guard and bypass the
`invoice_state_changed` event.

**Required change:** every write to `invoice.status` in `lib.rs` must route
through `transition_status`. Direct assignments (`invoice.status = X`) should
only appear inside `transition_status` itself.

### 2. Missing state coverage in tests

`state_machine_tests.rs` does not test:

- `Pending → Disputed` (via `raise_dispute`)
- `Disputed → Pending` (via `resolve_dispute`)
- `Disputed → Refunded` (via `resolve_dispute`)
- `Pending → PartiallyReleased` (via staged release)
- `PartiallyReleased → Released` (final stage released)
- `PayoutInProgress → Released`
- `Pending → Expired` (deadline passed)
- `Expired → Refunded` (refund after expiry)

The `TRANSITIONS` and `GUARDED` arrays in `state_machine_tests.rs` need to be
extended to cover these paths. Each new transition must follow the existing
pattern: drive the invoice to the precondition state using `drive_to`, perform
the action, and assert the resulting state.

### 3. Invalid transitions not covered by guards tests

The guard table should include at least:

- `Released → Cancel` — already present
- `Cancelled → Pay` — already present
- `Disputed → Cancel` — missing
- `Disputed → Pay` — missing
- `PartiallyReleased → Cancel` — missing
- `PartiallyReleased → Refund` (without resolving dispute) — missing

### 4. No validation for `PayoutInProgress`

`PayoutInProgress` is set during `release_funds` and cleared to `Released` or
`Refunded`. Nothing currently prevents a concurrent call from attempting to
transition an invoice that is already `PayoutInProgress`. The reentrancy guard
handles the concurrent case, but `valid_transition` should nonetheless reject
any move out of `PayoutInProgress` to anything other than `Released` or
`Refunded`, and a test should verify this.

---

## Proposed changes

### `lib.rs`

Replace every direct `invoice.status = X` assignment outside
`transition_status` with a call to `transition_status`. Where the assignment
is intentional and needs to bypass the guard (e.g. the initial `create_invoice`
sets status to `Pending` from no prior status), document the exception
explicitly.

### `state_machine_tests.rs`

Extend `TRANSITIONS` and `GUARDED` to cover the states listed in §2 and §3
above. The coverage assertion `cov.transition_coverage_pct() == 100` should
remain at the end of `every_transition_reaches_expected_state`.

Add a new test `payout_in_progress_is_guarded` that:
1. Gets an invoice into `PayoutInProgress` state.
2. Attempts `cancel_invoice`, `refund`, and `pay` on it.
3. Asserts all three are rejected.

### New test file: `tests/state_machine_integration.rs`

An integration-level test (top-level `tests/` directory, one contract
instance per test) that exercises the full sequence
`Pending → PartiallyReleased → Released` end-to-end using the public client,
confirming that the event sequence numbers increment monotonically and that
`get_invoice` reflects the correct status at each step.

---

## Validation rules (normative)

These rules are the authoritative specification for `valid_transition`.
Implementations must not add or remove edges without updating this table.

```
Pending           → PartiallyReleased  ALLOWED  (staged/partial release)
Pending           → Released           ALLOWED  (full release)
Pending           → Disputed           ALLOWED  (raise_dispute)
Pending           → Cancelled          ALLOWED  (cancel_invoice)
Pending           → Expired            ALLOWED  (deadline passed)
Pending           → Refunded           ALLOWED  (refund while Pending)

PartiallyReleased → Released           ALLOWED  (all stages released)
PartiallyReleased → Pending            ALLOWED  (dispute resolution reversal)

Disputed          → Pending            ALLOWED  (resolve_dispute: resume)
Disputed          → Refunded           ALLOWED  (resolve_dispute: refund)

PayoutInProgress  → Released           ALLOWED  (release_funds success)
PayoutInProgress  → Refunded           ALLOWED  (release_funds failure path)

Expired           → Refunded           ALLOWED  (refund after deadline)

Released          → (any)              DENIED   (terminal)
Refunded          → (any)              DENIED   (terminal)
Cancelled         → (any)              DENIED   (terminal)
Finalised         → (any)              DENIED   (terminal)
Deleted           → (any)              DENIED   (terminal)
```

Any same-state transition is allowed (idempotent).

---

## Storage

No new storage keys. The state transition is stored as the `status` field
inside the existing `InvoiceCore` / `InvoiceHot` entries. `transition_status`
already emits `invoice_state_changed`; no new event is needed.

---

## Open questions

- Whether `Expired` should be a separate terminal state or simply an alias for
  `Refunded` after the deadline. The current code treats `Expired` as terminal
  for `valid_transition` but allows `refund()` to move an expired invoice to
  `Refunded` directly (bypassing the guard). This inconsistency should be
  resolved: either `Expired → Refunded` is added to `valid_transition`, or
  `refund()` is documented as the one entry point that may call
  `transition_status` with that move.
- Whether `Deleted` (soft-delete tombstone) should ever appear as a `from`
  state in `valid_transition`. Currently it is terminal, but a "restore" admin
  action might need `Deleted → Pending`.

---

## Not done

The direct-assignment audit across `lib.rs`, the extended test cases, and the
integration test file are all to be implemented.
