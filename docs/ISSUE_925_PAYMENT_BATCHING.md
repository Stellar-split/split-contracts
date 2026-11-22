# Issue #925: Payment Batching

A payer can collect payments to several invoices into a batch, then settle them all with
one token transfer. This builds on `pool_pay` (issue #3), whose accounting now lives in a
shared `_pool_pay` helper so both paths validate and record payments the same way.

## Data model

```rust
pub enum PaymentBatchStatus { Open, Executed, Cancelled }

pub struct PaymentBatch {
    pub id: u64,
    pub payer: Address,
    pub token: Address,
    pub items: Vec<InvoicePayment>, // one entry per invoice
    pub total: i128,
    pub status: PaymentBatchStatus,
    pub created_at: u64,
    pub executed_at: u64,
}
```

## Collection

| Function | Notes |
|----------|-------|
| `open_payment_batch(payer, token) -> u64` | Creates an empty `Open` batch |
| `add_to_payment_batch(payer, batch_id, invoice_id, amount)` | Validates the item and appends it. Adding to an invoice already in the batch merges the amounts |
| `remove_from_payment_batch(payer, batch_id, invoice_id)` | Removes the item and updates `total` |
| `cancel_payment_batch(payer, batch_id)` | Marks the batch `Cancelled`. No funds have moved |

Only the batch owner can change a batch (`"not batch owner"`), and only while it is
`Open` (`"batch is not open"`).

## Validation

Each item is checked when it is added and again at execution:

- The invoice is `Pending`, not disputed, and before its deadline.
- The invoice is funded in the batch token.
- `amount > 0`.
- The batched amount for the invoice (merged additions included) fits within the invoice's remaining balance.

## Batch size limits

- The default limit is 25 invoices per batch. `set_payment_batch_limit(admin, limit)` (SuperAdmin) can change it within `1..=50`.
- Adding an invoice beyond the limit panics with `BatchLimitExceeded`. Merging into an existing item does not count toward the limit.

## Atomicity

`execute_payment_batch(payer, batch_id)`:

1. Re-validates every item before any funds move.
2. Makes a single `token.transfer(payer → contract)` for the batch total.
3. Credits each invoice through the same sharded payment, audit (`batch_pay`) and auto-release path as `pool_pay`.

If any step panics, the whole transaction reverts. No funds move, no invoice is credited, and
the batch stays `Open`. Execution is blocked by the pause and circuit breaker guards under
the `pay` guard name.

## Monitoring

`execute_payment_batch` returns a `BatchExecutionReport`:

| Field | Meaning |
|-------|---------|
| `item_count`, `total_amount` | Batch size and value |
| `token_transfers`, `transfers_saved` | 1 transfer, compared with one transfer per item |
| `batched_cpu_insns` | `batch_pay` cost estimate (issue #924 model) |
| `unbatched_cpu_insns` | Sum of `pay` estimates if each invoice were paid individually |
| `savings_bps` | `(unbatched - batched) / unbatched` |

With the default models, a 2-invoice batch saves about 36% of estimated instructions and a
5-invoice batch about 63%, above the 20% target.

`get_batching_stats()` gives cumulative figures: `batches_executed`, `items_processed`,
`total_volume`, `transfers_saved`, `estimated_insns_saved` and `last_savings_bps`.

Events: `pb_open`, `pb_exec` `(batch_id, item_count, total_amount, savings_bps)`, `pb_cncl`.
