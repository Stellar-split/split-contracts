# Issue #974: Compute budget estimation (status)

**Status:** the entry point exists. `estimate_compute` in `contracts/split/src/lib.rs` is the read-only
simulation API. The design is in [`ISSUE_298_COMPUTE_ESTIMATION.md`](./ISSUE_298_COMPUTE_ESTIMATION.md),
and [`COMPUTE_BUDGETS.md`](./COMPUTE_BUDGETS.md) explains how the figures are measured. Its helper
`estimate_operation_cost` is also in `lib.rs`.

## What this change does

Records the status above. It changes no code and adds no tests.

## What still needs checking

- That the figures `estimate_compute` returns match the measured ranges in `COMPUTE_BUDGETS.md`
  for each operation it accepts.
- Whether the client SDK uses the same figures, so a caller's resource limit is not too low.

## Not done

No review of the estimate's accuracy, no new tests.
