# Issue #1151: Test Plan — Group Batching Atomicity

Test plan only — no test code. The scenarios below should become `#[test]`
functions in `contracts/split/src/test.rs`.

"Batch" covers four separate mechanisms in this codebase, and they make
different atomicity promises. A test suite for this issue has to cover each one
against its own promise.

| Mechanism | Entry point(s) | Promise |
|---|---|---|
| Batch invoice creation | `create_invoices_batch` (≤10), `create_batch` (≤5) — `lib.rs:7037`, `lib.rs:7128` | All invoices are created or none are |
| Payment batch | `open_payment_batch`, `add_to_payment_batch`, `remove_from_payment_batch`, `cancel_payment_batch`, `execute_payment_batch` — `lib.rs:9201-9424` | One token transfer, and every item is credited or none are |
| Invoice group | `create_invoice_group` — `lib.rs:13867`; release gate `lib.rs:9651-9677` | AllOrNothing: no member releases until every member is funded. Majority: more than 50% funded |
| Off-chain batch processor | `batch_processor::process` — `contracts/split/src/batch_processor.rs` | **Not atomic by design.** It keeps going after an item fails and reports each failure |

## Where atomicity comes from

Soroban reverts **all** storage writes and token transfers of an invocation
when it panics. So any batch entry point that panics partway through is atomic
automatically, whatever order it writes in. The tests therefore need to show
two things:

1. A failing item makes the **whole call panic**, so the batch is never
   skipped-and-continued.
2. After the panic, **no state from earlier items survives**: invoice counter,
   invoice records, balances, stats and events.

Point 2 only shows up if the test inspects state after a `try_*` call. A plain
`#[should_panic]` test checks point 1 but not point 2. Use the `try_` client
methods and assert state afterwards.

## Findings to raise before writing tests

1. **Group rollback is dead code.** `rollback_invoice_group` (`lib.rs:17663`)
   and `create_group_for_invoices` (`lib.rs:3597`) are both
   `#[allow(dead_code)]`. They read and write `group_members_key` /
   `invoice_group_id_key`, a **different** storage layout from the one
   `create_invoice_group` writes (`group_key` / `invoice_group_key`). So:
   - nothing ever triggers a rollback when a group member expires;
   - `get_group_members_list(group_id)` returns `[]` for every group created
     through the public API.
   The "Rollbacks work" and "Recovery smooth" criteria for groups cannot pass
   until this is wired up. Write those tests as
   `#[ignore = "blocked: group rollback not wired (#434)"]`.
2. **`create_invoice_group` has no auth or validation.** Anyone can call it.
   It does not check that the invoices exist, that the caller owns them, that
   IDs are not duplicated, or that an invoice is not already in another group.
   Regrouping silently overwrites `invoice_group_key`. Section 3.6 covers
   this.
3. **`create_batch` does not pre-validate.** Unlike `create_invoices_batch`, it
   creates invoices one at a time, so a bad item 3 panics after items 1 and 2
   were written. Host rollback still makes it atomic, but the tests must
   confirm that the invoice counter and rate-limit counters are rolled back
   too.

## Test scenarios

### 1. Batch invoice creation (`create_invoices_batch`, `create_batch`)

1. **All-or-nothing.** In a 10-item batch, item 7 is invalid (mismatched
   recipients and amounts, or a deadline in the past). The call errors.
   Afterwards the next invoice ID is unchanged, `get_invoice(id)` fails for
   every ID the batch would have used, and no `InvoiceCreated` or
   `batch_invoice_created` events are present.
2. **First item invalid** and **last item invalid** give the same result as
   1.1. Covering both ends of the batch catches off-by-one errors in the
   pre-validation loop.
3. **`create_batch` with item 3 invalid** (5-item batch): the same assertions
   as 1.1, plus the creator's rate-limit counter is unchanged.
4. **Creation fees.** With a creation fee configured, a failed batch charges
   **no** fee. A successful batch charges `n × fee`. This extends the existing
   `test_creation_fee_charged_per_invoice_in_batch`.
5. **Limits.** 10 items pass and 11 panic with `"BatchLimitExceeded"` for
   `create_invoices_batch`. 5 pass and 6 panic with `"batch limit exceeded"`
   for `create_batch`. No state is written on rejection.
6. **Empty batch** returns `[]` and writes no state. Decide whether that is
   intended; if not, add a guard.
7. **IDs are consecutive and in order** for a successful batch.

### 2. Payment batch

Fixture: invoices A, B and C, each needing 100. The payer has 1 000 tokens.
Open a batch with all three items.

1. **All-or-nothing on execute.** Make B invalid after it was added, for
   example by having the admin freeze it, or by having another payer fund it
   so it is already `Released`. `try_execute_payment_batch` errors. Then
   assert:
   - the payer's balance is unchanged, because there was no single transfer;
   - A, B and C all have unchanged `funded`;
   - the batch status is still `Open`, not `Executed`. The status is written
     *before* `_pool_pay` runs (`lib.rs:9361-9365`), so this checks that the
     revert covers it;
   - `get_batching_stats()` is unchanged;
   - no `payment_batch_executed` event.
2. **Insufficient payer balance.** With a balance of 250 against a 300 total,
   execute errors and nothing changes. `_pool_pay` validates everything
   before the single transfer, so this should fail before any credit.
3. **Payer reputation and nonce.** On failure, no `rep_updated` event, and
   `get_rep(payer)` and `get_nonce` are unchanged for **all** items, including
   ones earlier in the list than the failing one.
4. **Success.** Exactly one token transfer from the payer, each invoice
   credited its exact amount, status `Executed`, stats incremented by one
   batch and three items, and `transfers_saved == 2`.
5. **Double execute.** A second `execute_payment_batch` on an `Executed` batch
   panics, and no funds move.
6. **Cancel then execute** panics. **Remove then execute** credits only the
   remaining items.
7. **Wrong payer.** Payer B cannot add to, remove from, cancel or execute
   payer A's batch.
8. **Limit.** `set_payment_batch_limit(n)`: adding item `n+1` panics, and the
   batch keeps its `n` items.
9. **Mixed tokens.** Adding an invoice priced in a different token from the
   batch's token panics at add time.

### 3. Invoice groups

1. **AllOrNothing blocks partial release.** This is already covered by
   `test_group_partial_fund_blocks_release`. Extend it so that after the
   panic, member 1 is still `Pending` and the recipient balance is unchanged.
2. **AllOrNothing releases each member once all are funded.** Covered by
   `test_group_all_funded_releases_both`. Keep it.
3. **Majority, boundaries.** For groups of 2, 3 and 4 members, check the
   release gate at each funded count. The condition is strict
   `funded_count * 2 > total`, so:
   - 2 members: 1 funded is blocked, 2 funded releases;
   - 3 members: 1 is blocked, 2 releases;
   - 4 members: 2 is blocked (exactly half), 3 releases.
4. **Consistency of the gate across release paths.** The group gate only
   lives in `release_invoice`. Run `release_tranche`, `release_to_recipient`,
   `stage_release`, `partial_release` and `trigger_scheduled_release` against
   a partially funded AllOrNothing group. Each should panic. Any path that
   releases breaks "No partial batches". Expect failures and file them.
5. **Group state after a member is refunded.** Member A is refunded and B is
   funded. B's AllOrNothing release stays blocked forever, because
   `group_all_funded` compares `funded < total` and a refunded A never
   satisfies it. Record this, since it is the case rollback is supposed to
   handle (finding 1).
6. **Group creation validation** (finding 2). Each case below should be
   rejected. Write them to assert the rejection, then `#[ignore]` them until
   guards are added:
   - a caller who is not the creator of every member;
   - a non-existent invoice ID;
   - a duplicate ID in the list;
   - an invoice that is already in another group (today it is silently moved
     to the new group).
7. **Group rollback** (blocked, finding 1). When one member expires, every
   `Pending` member becomes `Refunded`, each payer is refunded
   `amount + tip`, and one `group_rollback_triggered(group_id, n)` event is
   emitted. Members already `Released` are left alone. Then check recovery:
   payers can pay into new invoices, and the old group ID no longer gates
   anything.

### 4. Off-chain `batch_processor` (contrast case)

These are already partly covered by unit tests in `batch_processor.rs`. Add or
keep:

1. **Validation is all-or-nothing.** `validate` rejects an empty batch, too
   many items, an amount `<= 0`, a duplicate ID, a sum that overflows `i128`,
   or a total above `max_total`. No handler runs on a rejected batch.
2. **Processing is deliberately not atomic.** In a valid batch where the
   handler fails item 3, items 1, 2, 4 and 5 still run, the report lists
   exactly item 3 as failed, and `BatchStats` counts the failure. This test
   exists so nobody mistakes `process` for an atomic path.

## Acceptance-criteria mapping

| Criterion | Scenarios |
|---|---|
| Atomicity enforced | 1.1–1.3, 2.1–2.3, 3.1 |
| Rollbacks work | 1.1, 2.1 (host revert); 3.7 (blocked) |
| Recovery smooth | 2.6, 3.7 (blocked) |
| Consistency maintained | 1.4, 1.7, 2.4, 3.3, 3.4 |
| No partial batches | 1.x, 2.1–2.3, 3.4 |
| All tests pass | Sections 1, 2 and 4 green. 3.4 and 3.6 need guards. 3.7 is `#[ignore]`d until rollback is wired |
