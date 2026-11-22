# Issue #1150: Invoice state machine edge case tests (test plan)

**Status:** test plan only. This change adds no tests. The existing suites it builds on are listed
below.

## What already exists

| Piece | Where | What it covers |
|---|---|---|
| Transition rules | `valid_transition` / `transition_status` in `contracts/split/src/lib.rs` (#519) | Which `InvoiceStatus` moves are allowed. Both are marked `#[allow(dead_code)]`, so not every lifecycle function goes through them. |
| State machine suite | `contracts/split/src/state_machine_tests.rs` (#1024) | The four `Pending` transitions, six guarded moves out of `Released` / `Cancelled` / `Refunded`, and that those three states are absorbing. |
| Concurrency suite | `contracts/split/src/concurrency_tests.rs` (#1026) | Interleaved payers, final payment racing cancel, double refund, funds conservation. |
| Error scenarios | `contracts/split/src/error_scenario_tests.rs` (#1023) | One invalid call, then a check that `funded` and `status` are unchanged, then a successful recovery payment. |
| Undo | `reversible_update_deadline`, `reversible_update_amounts`, `reversible_cancel_invoice`, `undo_operation` (#929) | Reversing an operation inside the rollback window. |

### The allowed transitions (from `valid_transition`)

| From | Allowed to |
|---|---|
| `Pending` | `PartiallyReleased`, `Released`, `Disputed`, `Cancelled`, `Expired`, `Refunded` |
| `PartiallyReleased` | `Released`, `Pending` |
| `Disputed` | `Pending`, `Refunded` |
| `PayoutInProgress` | `Released`, `Refunded` |
| `Released`, `Refunded`, `Expired`, `Cancelled`, `Finalised`, `Deleted` | nothing (terminal) |

A move to the same state is always allowed.

## Gaps this issue should close

The current suite only reaches `Pending`, `Released`, `Cancelled` and `Refunded`. It does not test
`Expired`, `Disputed`, `PartiallyReleased`, `PayoutInProgress`, `Finalised` or `Deleted`. It also does
not check that the public functions agree with `valid_transition`.

## Test plan

Put the new tests in `state_machine_tests.rs`, named `test_1150_*`, using the helpers in
`testing_support.rs`. After every rejected call, assert that `status`, `funded`, `claimed` and the
contract's token balance are unchanged.

### 1. Invalid transitions

- A table test over every `(from, to)` pair of `InvoiceStatus`, checking `valid_transition` against
  the table above. This is a pure function test; it needs no contract calls.
- For each terminal state not yet covered (`Expired`, `Deleted`): pay, cancel, refund and
  `add_recipient` must all be rejected.
- `Disputed`: cancel, `add_recipient`, `adjust_split` and release must be rejected; resolving the
  dispute must return the invoice to `Pending` or move it to `Refunded`.
- Paying exactly 0, or 1 stroop more than the remaining amount, must be rejected in every state.
- Every lifecycle function that writes `invoice.status` directly instead of calling
  `transition_status` must still produce only moves that `valid_transition` allows. List these
  functions while writing the tests; any that break the rules are bugs to report.

### 2. Concurrent changes

Soroban runs transactions one at a time, so "concurrent" here means two actors' calls interleaved in
the same ledger, as in `concurrency_tests.rs`.

- The final payment and `cancel_invoice` in both orders: exactly one succeeds, and the end state
  matches the one that ran first.
- The final payment and `refund` at the deadline timestamp, in both orders.
- `add_recipient` against a first payment: once `funded > 0`, `add_recipient` must fail.
- `raise_dispute` against the final payment, in both orders.
- Two `undo_operation` calls on the same op id: the second must fail.

### 3. State rollback

- A call that fails part way through (for example a token transfer that fails during release) must
  leave no partial writes. Check `status`, `funded`, `claimed` and the audit log length.
- `reversible_cancel_invoice` followed by `undo_operation` inside the window restores `Pending` and
  the same snapshot hash.
- `undo_operation` after the window closes is rejected and the state stays `Cancelled`.
- `undo_operation` after a later operation has changed the invoice is rejected (the snapshot no
  longer matches).

### 4. Recovery

- After any rejected call, a valid full payment still releases the invoice (the pattern from #1023,
  applied to each new state).
- `PayoutInProgress` with a failed payout: `retry_failed_payout` or `claim_pending_payout` completes
  the move to `Released`.
- A disputed invoice that is resolved can be paid and released normally.
- An emergency pause, then unpause: the invoice continues from the state it was in.

### 5. Cleanup

- `delete_invoice` leaves a `Deleted` tombstone, and no further call can change it.
- `archive_invoice` and `purge_archived_invoices` only accept terminal invoices; a `Pending` invoice is
  refused.
- After purge, the recipient index (`get_recipient_invoice_ids`) and the metadata store
  (`get_invoice_metadata_keys`) hold no entries that point at the purged invoice.

## Acceptance criteria

| Criterion | Checked by |
|---|---|
| Invalid transitions rejected | Section 1 |
| Concurrency safe | Section 2 |
| Rollback works | Section 3 |
| Recovery smooth | Section 4 |
| No inconsistencies | The state, funds and balance assertions after every rejected call; Section 5 |
| All tests pass | `cargo test -p split test_1150` |

## Open questions

- `valid_transition` allows `PartiallyReleased → Pending`, but the comment calls this a dispute
  resolution path. Confirm which function performs it before writing that test.
- `Finalised` is described as an alias for `Released`. Decide whether tests should treat it as its own
  state.
