# Issue #1158: Dynamic Recipient Management Tests

## Overview

Test plan for adding and removing recipients on an existing invoice, with checks that the invoice's parallel vectors and indexes stay consistent after every operation.

## Functions Under Test

All in `contracts/split/src/lib.rs`.

#### `add_recipient(caller, invoice_id, recipient, amount)`
- Creator-only, `Pending` invoices only, rejected when `disputed` or `funded > 0`
- `amount` must be `> 0`
- Pushes onto `recipients`, `amounts`, `tokens` (copies `tokens[0]`) and `claimed` (`0`)
- Adds `invoice_id` to `recipient_invoice_ids_key(recipient)`
- Writes audit entry `add_rec` and emits `recipient_added`

#### `rebalance_recipients(creator, invoice_id, remove_address)`
- Creator-only, `Pending` invoices only, rejected when `disputed` or `funded > 0`
- At least 3 recipients before removal (`InsufficientRecipients`), so at least 2 remain
- Gives the removed amount to the remaining recipients in proportion to their shares; the rounding remainder goes to the first remaining recipient
- Writes audit entry `rebal` and emits `recipients_rebalanced`

#### `adjust_split(caller, invoice_id, new_amounts)`
- Used in consistency tests to confirm that later edits still work after add/remove

## Invariants Checked After Every Operation

| Invariant | Check |
|---|---|
| Parallel vector lengths | `recipients.len() == amounts.len() == tokens.len() == claimed.len()` |
| Total preserved on remove | `sum(amounts)` before == `sum(amounts)` after `rebalance_recipients` |
| Total grows on add | `sum(amounts)` after == before + `amount` |
| Order preserved | Remaining recipients keep their original relative order |
| Claimed state | `claimed` entries move together with their recipient |
| Audit trail | One new audit entry per successful operation |
| No state change on failure | Invoice is identical after a rejected call |

## Test Cases

Add to `contracts/split/src/test.rs`.

### Add operations
- `test_add_recipient_appends_to_all_vectors`: lengths grow by 1, the new entry is last, `claimed == 0`, token equals `tokens[0]`
- `test_add_recipient_updates_recipient_index`: `get_recipient_invoice_ids(recipient)` contains the invoice
- `test_add_recipient_emits_event_and_audit`: `recipient_added` event and `add_rec` audit entry
- `test_add_multiple_recipients_sequentially`: totals and ordering are correct after N adds

### Remove operations
- `test_rebalance_removes_recipient`: address is gone and lengths shrink by 1
- `test_rebalance_proportional_redistribution`: e.g. `[100, 200, 300]` with the 300 removed gives `[200, 400]`
- `test_rebalance_remainder_to_first`: amounts that do not divide evenly; the first recipient absorbs the remainder and the total is unchanged
- `test_rebalance_removes_first_and_last`: index edge positions

### Validation
- `test_add_recipient_non_creator_panics`: `"only creator can add recipients"`
- `test_add_recipient_zero_or_negative_amount_panics`: `"amount must be positive"`
- `test_add_recipient_after_funding_panics`: `"cannot add recipient after payment received"`
- `test_add_recipient_non_pending_panics`: released or cancelled invoice
- `test_add_recipient_disputed_panics`
- `test_add_recipient_when_paused_panics`
- `test_rebalance_non_creator_panics`
- `test_rebalance_unknown_address_panics`: `"recipient not in invoice"`
- `test_rebalance_with_two_recipients_panics`: `InsufficientRecipients`
- `test_rebalance_after_funding_panics`: `"payments already received"`

### Consistency
- `test_add_then_remove_roundtrip`: add X, then remove X; the remaining amounts are redistributed (not restored) and the total equals the original plus X's amount
- `test_interleaved_add_remove_keeps_vectors_aligned`: random sequence of operations with the invariants checked after each step
- `test_adjust_split_after_add_and_remove`: length check uses the updated recipient count
- `test_release_after_dynamic_changes`: fund and release; each recipient receives exactly their final amount

### Edge cases
- `test_add_duplicate_recipient`: documents current behaviour (see Known Gaps)
- `test_add_recipient_with_max_amount`: an `i128` amount close to overflow of `sum(amounts)`
- `test_rebalance_with_one_dust_recipient`: share of 1 stroop; rounding still preserves the total
- `test_add_recipient_on_prioritized_invoice`: see Known Gaps
- `test_rebalance_on_prioritized_invoice`: see Known Gaps

## Known Gaps Found While Writing This Plan

These should be fixed, or pinned down by a test, before the issue is closed:

1. **Priorities not kept in sync.** `add_recipient` and `rebalance_recipients` do not update `invoice.priorities`. On an invoice created with priorities:
   - after an add, `priorities.len() < recipients.len()` and `partial_release` panics on `priorities.get(idx).unwrap()`
   - after a remove, the priorities shift onto the wrong recipients
2. **No duplicate check in `add_recipient`.** Creation uses `assert_unique_recipients`, but `add_recipient` does not, so a duplicate address can be added. `rebalance_recipients` then removes only the first match.
3. **Index not cleaned up on remove.** `rebalance_recipients` does not remove `invoice_id` from `recipient_invoice_ids_key(remove_address)`.

## Acceptance Criteria

- [ ] Add/remove works
- [ ] Validation enforced
- [ ] Consistency maintained (invariant helper run after every mutation)
- [ ] Edge cases handled
- [ ] All tests pass (`cargo test -p split`)
- [ ] No corruption (parallel vectors stay aligned, totals are preserved)
