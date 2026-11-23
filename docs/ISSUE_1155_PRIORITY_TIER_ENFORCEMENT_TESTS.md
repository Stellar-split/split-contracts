# Issue #1155: Priority Tier Enforcement Tests

## Overview

Tests for per-recipient priority tiers: assigning them, enforcing the payment order in `partial_release`, and keeping that order deterministic.

## Current Model

- `Invoice.priorities: Vec<u32>` runs parallel to `recipients`. An empty vector means no ordering.
- A lower number means a higher priority (paid first).
- Priorities are set at creation through the `priorities` option of `create_invoice` / `create_invoice_with_recipients`.
- Validation: if the vector is not empty, `priorities.len() == recipients.len()`, otherwise the call panics with `"priorities length must match recipients"`.

## Functions Under Test

#### `partial_release(invoice_id, creator, amount)`
- Creator-only, `Pending`, not frozen, not disputed, `0 < amount <= funded`
- **With priorities**: recipients are sorted ascending by priority (selection sort) and each is paid their full amount while `remaining >= amount`. A recipient who cannot be paid in full is **skipped**, and the loop continues to later recipients.
- **Without priorities**: proportional distribution, with the last recipient taking the remainder
- Invoice status stays `Pending`

#### Queries
- No public getter. `get_invoice` returns `InvoiceCore`, which does not include `priorities`. Tests read the full invoice inside `env.as_contract(...)` with `load_invoice`. A `get_priorities(invoice_id)` view may be worth adding so "queries efficient" can be tested from outside the contract.

## Test Cases

Add to `contracts/split/src/test.rs`.

### Tier assignment
- `test_priorities_stored_on_create`: the stored vector (read via `load_invoice`) equals the input
- `test_priorities_empty_means_unordered`: falls back to proportional release
- `test_priorities_length_mismatch_panics`: `"priorities length must match recipients"`

### Processing order
- `test_partial_release_pays_lowest_priority_first`: priorities `[3, 1, 2]`; with funds for one recipient only, the recipient with priority 1 is paid
- `test_partial_release_full_amount_pays_all_in_order`: token balances match after a release that covers everyone
- `test_partial_release_skips_unaffordable_then_pays_smaller`: amounts `[100, 500, 50]`, priorities `[1, 2, 3]`, release 160; recipients 1 and 3 are paid and recipient 2 is skipped (pins the skip behaviour)
- `test_partial_release_amount_below_all_shares_pays_nobody`: no transfers, and the amount is not lost

### Determinism
- `test_partial_release_equal_priorities_tie_break_by_index`: priorities `[1, 1, 1]`; the lower index is paid first (strict `<` in selection)
- `test_partial_release_order_independent_of_recipient_order`: the same priority mapping with recipients shuffled pays the same set
- `test_partial_release_repeatable`: an identical setup in two environments gives identical events and balances

### Tier updates
- `test_priorities_unchanged_by_adjust_split`: amounts change and priorities do not
- `test_priorities_after_add_recipient`: see Known Gaps
- `test_priorities_after_rebalance_recipients`: see Known Gaps

### Edge cases
- `test_priority_u32_max`: a recipient with `u32::MAX` is still selected (it ties with the sentinel `best_pri = u32::MAX` and still resolves, because `best_pos` defaults to 0)
- `test_priority_zero`: highest priority
- `test_single_recipient_with_priority`
- `test_partial_release_non_creator_panics`
- `test_partial_release_frozen_or_disputed_panics`
- `test_partial_release_amount_exceeds_funded_panics`

## Known Gaps

1. **No update path.** No function changes `priorities` after creation, so "tier updates" can only be tested through other operations that change recipients.
2. **Desync on add.** `add_recipient` does not push a priority. Afterwards `priorities.len() < recipients.len()` and `partial_release` panics on `priorities.get(idx).unwrap()`.
3. **Desync on remove.** `rebalance_recipients` does not remove the matching priority, so the later priorities shift onto the wrong recipients.
4. **All-`u32::MAX` selection.** With every priority at `u32::MAX`, no candidate is strictly less than the sentinel, so `best_pos` stays 0. The order is still by index, but only by accident. A test should pin this.
5. **Accounting.** The priority branch of `partial_release` should be checked to confirm it updates `funded` and `claimed` the same way the proportional branch does.

Recommended fix for gaps 2 and 3: keep `priorities` in step inside `add_recipient` (take the priority as an argument or default to `u32::MAX`) and inside `rebalance_recipients`. This is shared with #1158.

## Acceptance Criteria

- [ ] Tiers assigned
- [ ] Order respected
- [ ] Updates work (or the gap is documented and fixed)
- [ ] Queries efficient
- [ ] All tests pass (`cargo test -p split`)
- [ ] Deterministic order (ties broken by index)
