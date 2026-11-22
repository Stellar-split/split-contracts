# Issue #975: Payment batching (status)

**Status:** the batching entry points exist in `contracts/split/src/lib.rs`: `create_batch`,
`add_to_payment_batch`, `cancel_payment_batch`, and `create_invoices_batch`. The design is in
[`ISSUE_925_PAYMENT_BATCHING.md`](./ISSUE_925_PAYMENT_BATCHING.md).

## What this change does

Records the status above. It changes no code and adds no tests.

## What still needs checking

- That the issue's wording, "payment batching", is covered by the existing batch functions, and that
  each batch is bounded so an oversized batch is refused before any write.
- That a failure in one payment rolls back the whole batch, as the design requires.

## Not done

No review of the batch logic, no new tests.
