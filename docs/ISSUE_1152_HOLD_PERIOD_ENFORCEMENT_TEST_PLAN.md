# Issue #1152: Test Plan — Payment Hold Period Enforcement

Test plan only — no test code. The scenarios below should become `#[test]`
functions in `contracts/split/src/test.rs`. Right now there are **no tests**
for `escrow_hold_period`, `held_until` or `EscrowHoldActive`. Every call site
in tests and e2e passes `escrow_hold_period: None`.

This plan covers the hold that exists in the code: the **escrow hold**. It is
not the hold-and-reserve design in
[ISSUE_1001_PAYMENT_HOLDS_DESIGN.md](ISSUE_1001_PAYMENT_HOLDS_DESIGN.md),
which is design only and not implemented.

## Code under test

| Item | Location |
|---|---|
| `escrow_hold_period: Option<u32>`, `held_until: Option<u32>` on the invoice | `contracts/split/src/types.rs:851-852` |
| Hold starts when the invoice becomes fully funded | `contracts/split/src/lib.rs:8743-8750` |
| Hold blocks auto-release on full funding | `contracts/split/src/lib.rs:8787` |
| Guard in `release_invoice` (and `release`, which calls it) | `contracts/split/src/lib.rs:9565-9569` |
| Guard in `release_tranche` | `contracts/split/src/lib.rs:11135-11139` |
| Guard in the release-readiness check (`simulate_release` path) | `contracts/split/src/lib.rs:16264-16268` |
| `resolve_escrow` clears the hold | `contracts/split/src/lib.rs:5626-5641` |
| `escrow_hold_started` event | `contracts/split/src/events.rs:1447` |

## How the hold actually works

- The unit is **ledger sequence**, not timestamp. Tests must move time with
  `env.ledger().set_sequence_number(...)`. `set_timestamp` has no effect on
  this hold.
- The hold is **per invoice**, not per payment or per recipient.
- When a `pay` brings `funded >= total` and `escrow_hold_period == Some(h)`
  and `held_until` is still `None`, the contract sets
  `held_until = sequence + h` (saturating) and emits `escrow_hold_started`.
  Because of the `is_none()` check, the hold is set **once** and later
  payments do not extend it.
- Release is allowed when `sequence >= held_until`. The comparison is strict
  `<` for "still held", so release **is** allowed at exactly `held_until`.
- `resolve_escrow(creator, id, hash)` sets `held_until = None`. This is the
  only early-release mechanism, and it is available to the creator or any
  co-creator **at any time**. It does not check that the invoice is funded,
  disputed or in any particular status.

## Coverage gap to raise before writing tests

Only `release_invoice` (with `release`) and `release_tranche` check
`held_until`. These release paths do **not** check it:

- `release_to_recipient` — `lib.rs:10322`
- `stage_release` — `lib.rs:11230`
- `partial_release` — `lib.rs:11398`
- `trigger_scheduled_release` — `lib.rs:9701`. It calls `_release` directly,
  not `release_invoice`.

Section 3 tests each of these. If any of them moves funds while a hold is
active, that breaks "No premature release". The test should be written to
expect `EscrowHoldActive`, so it fails until the guard is added. Fix the guard
in the same PR or a linked one; do not weaken the test.

## Test scenarios

Suggested fixture: an invoice with one recipient, `amount = 1_000`,
`escrow_hold_period = Some(100)`, and the ledger sequence set to `1_000`
before the funding payment. The expected `held_until` is `1_100`.

### 1. Hold enforcement

1. **The hold starts on full funding.** After the funding payment,
   `held_until == Some(1_100)` and exactly one `escrow_hold_started(id, 1_100)`
   event is emitted.
2. **Partial funding does not start the hold.** Pay 999 of 1 000 and
   `held_until == None`. Pay the final 1 and `held_until == sequence + 100`,
   using the sequence at the time of the **final** payment.
3. **No auto-release while held.** An invoice that would normally auto-release
   on full funding stays `Pending` (`lib.rs:8787`).
4. **`release` during the hold panics.** At sequence `1_050`, `release(id)`
   panics with `"EscrowHoldActive"`. Recipient balances are unchanged.
5. **`release_tranche` during the hold panics** with `"EscrowHoldActive"`.
6. **`simulate_release` or the readiness check** reports "not ready" during the
   hold and "ready" after it.
7. **No hold configured.** With `escrow_hold_period = None`, `held_until` stays
   `None` and release goes ahead immediately (regression guard).

### 2. Time-based release

1. **One ledger before the end.** At sequence `1_099`, `release` panics.
2. **Exactly at the end.** At sequence `1_100`, `release` succeeds, status is
   `Released`, and the recipient receives the payout minus fees.
3. **Well after the end.** At sequence `5_000`, `release` succeeds.
4. **`hold = 0`.** `held_until == sequence` and release succeeds in the same
   ledger.
5. **Overflow saturation.** With `hold = u32::MAX` and a sequence near the top,
   `held_until == u32::MAX` with no panic. Release stays blocked until
   sequence `u32::MAX`.
6. **Timestamp does not matter.** Move the timestamp forward by years while
   keeping the sequence inside the hold. `release` still panics.

### 3. Early release is blocked

1. **Each unguarded path, during the hold**, should panic with
   `"EscrowHoldActive"` and move no tokens:
   - `release_to_recipient(id, r)`
   - `stage_release(id, creator)`
   - `partial_release(id, creator, 500)`
   - `trigger_scheduled_release(id)`, with `scheduled_release_at` already
     passed

   See the coverage gap above. These are expected to fail on current `main`.
2. **Admin or arbitrary callers cannot bypass the hold.** `release` called with
   a mocked admin or a random address, during the hold, still panics.
3. **`resolve_escrow` authorisation.** A non-creator, non-co-creator panics
   with `"only creator or co-creator can resolve escrow"`. Missing auth from
   the creator also fails.
4. **`resolve_escrow` by the creator** sets `held_until = None`, after which
   `release` succeeds before the original end. Assert the `escrow_resolved`
   event. This is the intended early release, so it should be documented
   rather than blocked. But see 5.3.

### 4. Hold expiration

1. **Expiry does not release anything by itself.** After `held_until` passes,
   status is still `Pending` and balances are unchanged until someone calls
   `release`.
2. **Expiry combined with a dispute.** The hold has expired but a dispute is
   active, so `release` panics with the dispute error, not `EscrowHoldActive`.
   This checks that guards combine correctly.
3. **Deadline or refund during the hold.** If the invoice deadline passes while
   the hold is active, decide whether `refund` and `claim_refund` should be
   allowed. Assert the current behaviour and note the decision in the test.
4. **Cloned invoice.** `clone_invoice` copies `escrow_hold_period` but resets
   `held_until = None` (`lib.rs:7627-7628`). The clone starts its own hold
   when it is funded.

### 5. Concurrent holds

1. **Independent invoices.** Invoices A and B with different periods (100 and
   300), funded in the same ledger. At sequence +150, A releases and B panics.
   At +300, B releases.
2. **Extra payments after full funding do not extend the hold.** An
   overfunding or tip payment after `held_until` is set leaves it unchanged
   (the `is_none()` check).
3. **`resolve_escrow` on A does not affect B.**
4. **A hold combined with an invoice group** (all-or-nothing). Both members are
   funded, and A is held while B is not. B's release goes ahead if the group
   gate only checks funding. A's release still panics. Record the behaviour:
   group gating and the hold are independent.
5. **Hold combined with `release_delay_ledgers` or `scheduled_release_at`.**
   Release requires **both** to have passed. Test each order: the hold ending
   first, and the delay ending first.

## Acceptance-criteria mapping

| Criterion | Scenarios |
|---|---|
| Holds enforced | 1.1–1.7 |
| Time-based release | 2.1–2.6 |
| Early release blocked | 3.1–3.4 |
| Expiration handled | 4.1–4.4 |
| No premature release | 1.4, 1.5, 2.1, 3.1, 3.2 |
| All tests pass | Requires the hold guard on the paths listed in 3.1 (see the coverage gap) |
