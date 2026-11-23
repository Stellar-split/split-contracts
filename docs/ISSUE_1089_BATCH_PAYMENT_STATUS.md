# Issue #1089: Batch payment processing optimization (status)

**Status:** covered. The batch payment flow exists, bounds its size, reports the compute saved, and
keeps statistics. The design is in [`ISSUE_925_PAYMENT_BATCHING.md`](./ISSUE_925_PAYMENT_BATCHING.md).

## Requirements against the code

| Requirement | Where it is |
|---|---|
| Batch system | `open_payment_batch`, `add_to_payment_batch`, `remove_from_payment_batch`, `cancel_payment_batch`, `execute_payment_batch(payer, batch_id)` in `contracts/split/src/lib.rs` |
| Batch processing | `execute_payment_batch` pays every item through one `_pool_pay` call, so the payer's token is transferred once for the whole batch |
| Batch validation | A batch must be non-empty and within `get_payment_batch_limit()` (default 25, hard cap `MAX_PAYMENT_BATCH_LIMIT` = 50, set with `set_payment_batch_limit`, SuperAdmin). Only the payer who opened a batch can use it, and only while it is open. |
| Partial batches | A batch is all-or-nothing. A failing item aborts the whole call and nothing is written. An item can be removed or the batch cancelled before execution. |
| Batch status | `PaymentBatch` carries a `PaymentBatchStatus` (open, executed, cancelled), read with `get_payment_batch(batch_id)` |
| Cost reduced | `execute_payment_batch` returns a `BatchExecutionReport` with `token_transfers`, `transfers_saved`, `batched_cpu_insns`, `unbatched_cpu_insns` and `savings_bps`, using the cost model in `compute_cost_estimate` |
| Statistics | `get_batching_stats()` returns batches executed, items processed, total volume, transfers saved and estimated instructions saved |

`create_invoices_batch` and `create_batch` cover batched invoice creation.

## Notes

- The savings figures come from the contract's own cost model (`compute_cost_estimate`), not from a
  measured run, so they are estimates. See `docs/COMPUTE_BUDGETS.md`.
- `contracts/split/src/batch_processor.rs` (issue #1016) is a separate library that keeps going after a
  failed item and records it in a `BatchReport`. It is not declared in `lib.rs` and the contract does not
  use it, so the on-chain flow stays all-or-nothing.

## What this change does

Records the status above. It changes no code. The behaviour was read, not run, and no tests were added.
