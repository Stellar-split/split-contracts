# Issue #1154: Test Plan — Creator Reputation Calculation

Test plan only — no test code. The scenarios below should become `#[test]`
functions in `contracts/split/src/test.rs`, next to the existing reputation
tests (`test_reputation_zero_for_new_address`,
`test_reputation_increments_across_invoices`, `test_reputation_is_per_address`
around `test.rs:1625`).

## Code under test

| Item | Location |
|---|---|
| `RepScore { paid_on_time, late_pays, invoices_released, invoices_refunded }` (all `u32`) | `contracts/split/src/types.rs:400-409` |
| `rep_key`, `get_rep_internal`, `update_rep_internal` | `contracts/split/src/lib.rs:791-822` |
| Payer update on `pay` (on-time vs late) | `contracts/split/src/lib.rs:8677-8686` |
| Creator update on full release | `contracts/split/src/lib.rs:12208` (inside `_release_full`) |
| Creator update on refund | `contracts/split/src/lib.rs:12626` (inside `refund`) |
| `min_payer_rep` gate | `contracts/split/src/lib.rs:8280-8284` |
| `get_rep`, `get_reputation` | `contracts/split/src/lib.rs:14272-14283` |
| `rep_updated` event (carries `computed_score`) | `contracts/split/src/events.rs:1387-1414` |

## How scoring actually works

Each address has one `RepScore` record, and the same record is used whether
the address acts as a payer or a creator. A creator's reputation is the
`invoices_released` / `invoices_refunded` half of that record.

| Trigger | Address updated | Counter |
|---|---|---|
| `pay`, `timestamp <= penalty_deadline` or `penalty_deadline == 0` | payer | `paid_on_time += 1` |
| `pay`, `penalty_deadline > 0 && timestamp > penalty_deadline` | payer | `late_pays += 1` |
| `_release_full` completes | `invoice.creator` | `invoices_released += 1` |
| `refund` completes | `invoice.creator` | `invoices_refunded += 1` |

Every counter uses `saturating_add`. After each update, `update_rep_internal`
computes a derived score and **emits it in the `rep_updated` event only**. It
is not stored, and no getter returns it:

```
base        = paid_on_time * 10 + invoices_released * 5      (saturating u32)
deductions  = late_pays * 5 + invoices_refunded * 2          (saturating u32)
computed    = base.saturating_sub(deductions)                (floor 0)
```

The three read paths each mean something different:

- `get_rep(addr)` returns the raw `RepScore`.
- `get_reputation(addr)` returns `paid_on_time + late_pays` as a `u64`. Late
  payments count the same as on-time ones here.
- The `min_payer_rep` gate compares against `paid_on_time` only.

Tests should treat these as three separate contracts and assert each one.

## Test scenarios

Read `computed_score` from the last `rep_updated` event in `env.events().all()`.
A small `last_computed_score(&env, &addr)` test helper keeps this short.

### 1. Score calculation

1. **Fresh address.** `get_rep` returns `RepScore::default()` (all zeros),
   `get_reputation` returns `0`, and no `rep_updated` event exists.
2. **One on-time payment.** `paid_on_time == 1` and the event's
   `computed_score == 10`.
3. **One late payment.** Set `penalty_deadline`, move the ledger timestamp past
   it, then pay. Expect `late_pays == 1`, `paid_on_time == 0`, and
   `computed_score == 0` (`0 - 5` saturates to 0).
4. **Creator release.** Fully fund and release one invoice. The creator's
   `invoices_released == 1` and `computed_score == 5`.
5. **Creator refund.** Refund one invoice. The creator's `invoices_refunded == 1`
   and `computed_score == 0`.
6. **Mixed history, exact formula.** Get one address to `paid_on_time = 4`,
   `late_pays = 1`, `invoices_released = 2`, `invoices_refunded = 1`. Expect
   `computed = 40 + 10 - 5 - 2 = 43`. Use a table-driven test over several
   hand-computed tuples so the formula is pinned and not re-derived inside the
   test.

### 2. Updates are tracked

1. **Every update emits exactly one `rep_updated` event.** Its topic is
   `(split, rep_upd, address)` and its data is `(RepScore, computed)`. The
   `RepScore` in the event must equal `get_rep` read right after.
2. **Updates touch only the target address.** When A pays an invoice created by
   B, only A's record changes during the pay. B changes only on release or
   refund.
3. **A payer who is also the creator** (self-pay) gets both halves on one
   record: `paid_on_time` from the pay and `invoices_released` from the
   release.
4. **Failed operations do not change reputation.** A `pay` that panics after
   validation (for example on a frozen invoice or a bad nonce) leaves `get_rep`
   unchanged. This relies on Soroban reverting state when an invocation
   panics. Assert it explicitly anyway.
5. **Persistence.** Call `get_rep` again after moving the ledger forward. The
   value is unchanged because the record lives in persistent storage.

### 3. Boundary values

1. **Late-payment cutoff is strict `>`.** With `timestamp == penalty_deadline`
   the payment counts as **on time**. With `penalty_deadline + 1` it counts as
   **late**.
2. **`penalty_deadline == 0` means never late,** even at a very large
   timestamp.
3. **`min_payer_rep` gate.** Pass in three cases: `min_payer_rep = Some(0)`
   with a fresh payer, `Some(n)` with `paid_on_time == n`, and `None` with a
   fresh payer. Panic with `"insufficient payer reputation"` for `Some(n)` with
   `paid_on_time == n - 1`.
4. **Counter saturation.** Driving a counter to `u32::MAX` through real calls
   is not practical. Seed storage directly instead: in
   `env.as_contract(&contract_id, ...)`, write a `RepScore` under the `rep` key
   with `paid_on_time = u32::MAX`. Then:
   - one more on-time pay leaves `paid_on_time == u32::MAX`, with no panic or
     wrap;
   - `get_reputation` with `paid_on_time = late_pays = u32::MAX` returns
     `2 * u32::MAX as u64`, with no overflow because the sum is a `u64`.
5. **`computed_score` saturation.** Seed `paid_on_time = u32::MAX / 10 + 1`.
   The `* 10` saturates, so `computed == u32::MAX`, not a wrapped value.
6. **Deductions larger than base** (`late_pays = 1000`, everything else 0).
   `computed == 0`, never a panic or underflow.

### 4. Edge cases

1. **Multiple payments to the same invoice** each add 1. The counter tracks
   payment calls, not distinct invoices. Assert the current behaviour so a
   future change shows up as a deliberate test change.
2. **Partial or tranche releases.** Only `_release_full` credits
   `invoices_released`. Check what `release_tranche`, `partial_release` and
   `stage_release` do to the creator's score once an invoice reaches
   `Released` through them, and record the result. If the creator is never
   credited, file it as a bug rather than baking it into the test.
3. **Withdrawals and early withdrawals** do not decrement `RepScore`, unlike
   the separate credit score at `lib.rs:14288`. Assert that
   `paid_on_time` is unchanged after `withdraw`.
4. **Archived or restored invoices** do not reset or double-count the
   creator's counters.
5. **Payment to a deleted or frozen invoice** panics and leaves reputation
   unchanged (see 2.4).

### 5. Fairness

These tests document the current behaviour and flag decisions for the
maintainers. Where the behaviour looks wrong, mark the test `#[ignore]` with a
link to a follow-up issue rather than asserting the questionable behaviour.

1. **Two payers with the same history get the same score**, whatever their
   address or the order of calls.
2. **Same history built in a different order gives the same final score.** For
   example, on-time then late versus late then on-time. Because the formula
   uses saturating subtraction only at the end, the result is order-independent.
   Assert it.
3. **`get_reputation` treats late payments as good.** A payer with 10 late pays
   and a payer with 10 on-time pays both return `10`. Assert this, and note in
   the test comment that `get_reputation` is a payment count, not a quality
   score.
4. **Score farming.** A creator who makes a minimum-amount invoice and pays it
   themselves gains both `paid_on_time` and `invoices_released`. Write the test
   to show how cheap that is: the cost is one creation fee. Raise it as a
   follow-up.
5. **Creator penalised for refunds the creator did not cause.** A refund
   triggered by the deadline passing still costs the creator 2. Document it.

## Acceptance-criteria mapping

| Criterion | Scenarios |
|---|---|
| Calculations correct | 1.1–1.6 |
| Updates tracked | 2.1–2.5 |
| Boundaries handled | 3.1–3.6 |
| Edge cases safe | 4.1–4.5 |
| Fair scoring | 5.1–5.5 |
| All tests pass | `cargo test -p split` green. Behaviour flagged in 5.x and 4.2 is tracked as follow-ups, not left failing. |

## Notes for whoever implements this

- There is no getter for `computed_score`. If the tests need it often, add a
  read-only `get_computed_rep(address) -> u32` that reuses the formula, in a
  separate PR.
- Keep the formula constants (10, 5, 5, 2) in one place in the test module so
  a future weighting change updates one table.
