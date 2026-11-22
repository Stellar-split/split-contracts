# Issue #1095: Compute budget estimation (status)

**Status:** covered. Upfront estimates, an estimation API, usage tracking and an estimate-versus-actual
comparison all exist. The design is in
[`ISSUE_924_COMPUTE_BUDGET_ESTIMATION.md`](./ISSUE_924_COMPUTE_BUDGET_ESTIMATION.md), and the older
static estimators are in [`COMPUTE_BUDGETS.md`](./COMPUTE_BUDGETS.md) and
[`ISSUE_298_COMPUTE_ESTIMATION.md`](./ISSUE_298_COMPUTE_ESTIMATION.md).

## Requirements against the code

| Requirement | Where it is in `contracts/split/src/lib.rs` |
|---|---|
| Estimation model | `OperationCostModel` (base, per recipient, per payer, per item, memory) with built-in models for `create`, `pay`, `release`, `refund` and `batch_pay`. `set_operation_cost_model` (SuperAdmin) overrides one or defines a new operation. |
| Cost calculation | `compute_cost_estimate`, scaled by a calibration learned from recorded usage. `simulate_release` and `estimate_compute` are the static estimators. |
| Estimation API | `estimate_operation_cost(operation, recipient_count, payer_count, item_count)` returns a `CostEstimate` with `cpu_insns`, `mem_bytes`, `fee_stroops`, `within_budget` and `model_version`. Results are cached in temporary storage and dropped when a model or calibration changes. |
| Cost tracking | `record_compute_usage(reporter, operation, ..., actual_insns)` (Operator or higher) keeps min, max, last and total per operation, up to `MAX_PROFILED_OPERATIONS` (32) operations. |
| Actual versus estimated | The same call scores the current estimate against the measured value and stores `last_error_bps` and a `within_tolerance` count (tolerance `COST_ESTIMATE_TOLERANCE_BPS`, 10 percent). `get_compute_cost_report` returns average actual and estimated instructions, `calibration_bps` and `accuracy_bps` per operation. |

## Notes

- The built-in numbers are modeled constants. They only reflect real cost after an operator reports
  measured usage with `record_compute_usage`, which is an off-chain step: a transaction's real
  instruction count is not visible to the contract.
- `record_compute_usage` requires the Operator role, so only trusted reporters can move the calibration.
- `.github/workflows/compute-budget.yml` runs `cargo test --package split -- compute_budget` on pull
  requests, which checks functions against the per-transaction limit.

## What this change does

Records the status above. It changes no code. The behaviour was read, not run, and no tests were added.
