# Issue #1004: Batch payment processing (status)

**Status:** batch processing exists. The design is in [`ISSUE_925_PAYMENT_BATCHING.md`](./ISSUE_925_PAYMENT_BATCHING.md),
and the batch functions `create_batch`, `add_to_payment_batch`, `cancel_payment_batch`, and
`create_invoices_batch` are in `contracts/split/src/lib.rs`. `pool_pay` is the accounting it builds on.

## What this change does

Records the status above. It changes no code and adds no tests.

## What still needs checking

- That a batch settles with one transfer, as the design says, and that a failure in any payment rolls
  the whole batch back.
- That an oversized batch is refused before any write.
- Whether this issue's "batch processing" is the same as the batching in #925, or a separate operation
  that processes many batches. The issue text does not say.

## Not done

No review of the batch logic, no new tests.
