# Issue #1148: Dynamic recipient management (design)

**Status:** design only. Adding, removing and rotating recipients already exist in part. This doc
records what exists and what has to be added to support the full invoice lifecycle.

## What already exists (`contracts/split/src/lib.rs`)

| Function | What it does | Limits |
|---|---|---|
| `add_recipient(caller, invoice_id, recipient, amount)` | Adds a recipient with a fixed amount at the end of the list. Updates the recipient → invoice index. Emits `recipient_added`. | Creator only; `Pending`; not disputed; `funded == 0`; new total ≤ `MAX_INVOICE_TOTAL`. Uses the invoice's first token. Does **not** check for duplicate recipients. |
| `rebalance_recipients(creator, invoice_id, remove_address)` | Removes a recipient and shares their amount out to the others in proportion. Any rounding remainder goes to the first recipient. Emits `recipients_rebalanced`. | Creator only; `Pending`; `funded == 0`; needs at least 3 recipients. Does **not** remove the invoice from the removed address's index, and does not clear the invoice-ext cache. |
| `adjust_split(caller, invoice_id, new_amounts)` | Replaces all amounts. | Same list length; `funded == 0`. |
| `rotate_recipient_address(invoice_id, old, new)` | Swaps a recipient's address; signed by the old address. | `Pending`. Does not check whether `new` is already a recipient, and does not update the index. |
| `get_recipients_list`, `get_invoice_recipients`, `get_recipient_invoice_ids(_page)` | Queries. | |
| `get_audit_log(invoice_id)` | Contains `add_rec` and `rebal` entries. | No entry for rotation. |

Every change is blocked once any payment has arrived. That is the main gap against "during the invoice
lifecycle".

## Design

### 1. A plain remove, separate from rebalance

```rust
pub fn remove_recipient(env, caller, invoice_id, recipient) -> i128
```

Removes the recipient and their amount, so the invoice total goes down. Returns the amount removed.
`rebalance_recipients` stays for the case where the total must stay the same.

### 2. Changes after funding has started

Allowed only where no recipient can lose money they are owed:

| Operation | Rule once `funded > 0` |
|---|---|
| Add | Allowed. The invoice total goes up, so the invoice stays `Pending` until the new total is funded. |
| Remove | Allowed only if that recipient's `claimed == 0`, and only if the new total is still ≥ `funded`. Otherwise refused. |
| Change amount | Allowed only if the new amount is ≥ that recipient's `claimed`, and the new total is ≥ `funded`. |

If an invoice has a creator cosigner, both must sign any change after funding, the same way
`adjust_split` already works.

### 3. Validation, shared by every path

One helper, `validate_recipient_change`, used by add, remove, rebalance, adjust and rotate:

- The address passes `validation::validate_recipient_address` (#932).
- The address is not the contract itself, and not already a recipient (no duplicates).
- If the invoice has a recipient whitelist, the address is on it.
- The new count stays between `get_min_recipients()` and the contract's maximum recipient count.
- The total stays ≤ `MAX_INVOICE_TOTAL` and doesn't overflow (#933).
- The invoice is `Pending`, not disputed, and the contract is not paused.

### 4. Index consistency

Every add, remove and rotate must keep `recipient_invoice_ids_key` correct: add the invoice id for a
new address, remove it for an address that leaves. Every change also calls
`invalidate_invoice_ext_cache`. This fixes the two gaps noted in the table above.

### 5. Change history

Each change is appended to a new list stored per invoice, key `(rcp_hist, invoice_id)`:

```rust
#[contracttype]
pub struct RecipientChange {
    pub kind: RecipientChangeKind,   // Added | Removed | AmountChanged | Rotated | Rebalanced
    pub recipient: Address,
    pub other: Option<Address>,      // new address for Rotated
    pub old_amount: i128,
    pub new_amount: i128,
    pub by: Address,
    pub at: u64,
}
```

The list is capped at 100 entries; once full, the oldest entry is dropped. Each change also adds an
audit log entry (`add_rec`, `rm_rec`, `amt_rec`, `rot_rec`, `rebal`) and emits an event.

### Queries

| Function | Cost |
|---|---|
| `get_recipient_history(invoice_id)` | 1 read |
| `get_recipient_history_page(invoice_id, start, limit)` | 1 read |
| `is_invoice_recipient(invoice_id, address)` | 1 read |
| `get_recipient_count(invoice_id)` | 1 read |

## Test plan (`test_1148_*`)

- Add and remove before funding: amounts, tokens, `claimed` and the index all stay in step.
- Adding a duplicate address is refused (this is a fix to `add_recipient`).
- Removing down past `min_recipients` is refused.
- After a partial payment: add works; removing a recipient with `claimed > 0` is refused; a change
  that would make the total less than `funded` is refused.
- `rotate_recipient_address` to an existing recipient is refused, and the index moves to the new
  address.
- After `rebalance_recipients`, `get_recipient_invoice_ids(removed)` no longer lists the invoice.
- The history lists every change in order, and drops the oldest entry once it holds 100.
- A paused contract, a disputed invoice or a non-`Pending` invoice refuses every change.

## Acceptance criteria

| Criterion | Met by |
|---|---|
| Add/remove works | `add_recipient`, new `remove_recipient`, existing `rebalance_recipients`; the rules after funding |
| Validation enforced | `validate_recipient_change` on every path |
| Queries efficient | Every query reads a fixed number of entries |
| Changes tracked | `RecipientChange` history, audit entries, events |
| No inconsistencies | The index and cache fixes; total ≥ `funded`; amount ≥ `claimed` |
| All tests pass | `cargo test -p split test_1148` |

## Open questions

- If a recipient is added after funding, should the invoice's own token be used, or should the caller
  be allowed to choose one? This design keeps the current behaviour (the first token).
- Should a recipient be able to remove themselves? This design says no: only the creator can.
