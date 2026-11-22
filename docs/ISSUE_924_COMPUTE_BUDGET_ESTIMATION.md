# Issue #924: Compute Budget Estimation

Runtime cost estimation with a configurable cost model, usage tracking, calibration,
caching and per-operation profiling. This complements the static `estimate_compute`
(#351) and `simulate_release` (#298) entry points.

## Cost model

Each operation has an `OperationCostModel`:

```
cpu = base_insns
    + recipients * per_recipient_insns
    + payers     * per_payer_insns
    + items      * per_item_insns
mem = base_mem_bytes + (recipients + payers + items) * per_unit_mem_bytes
```

Built-in models, derived from the `estimate_compute` constants:

| Operation | base | per recipient | per payer | per item |
|-----------|------|---------------|-----------|----------|
| `create` | 1,000,000 | 200,000 | – | – |
| `pay` | 1,800,000 | 250,000 | – | – |
| `release` | 1,800,000 | 500,000 | – | – |
| `refund` | 1,800,000 | – | 350,000 | – |
| `batch_pay` | 1,800,000 | – | – | 400,000 |

`set_operation_cost_model(admin, operation, model)` (SuperAdmin) can override a built-in
model or define a new operation. Unknown operations panic with `"unknown operation"`.

## Estimation

`estimate_operation_cost(operation, recipient_count, payer_count, item_count) -> CostEstimate`
returns `{ operation, cpu_insns, raw_cpu_insns, mem_bytes, fee_stroops, calibration_bps, within_budget, cached, model_version }`.

- `raw_cpu_insns` comes straight from the model.
- `cpu_insns = raw_cpu_insns * calibration_bps / 10_000`.
- `within_budget` checks against the 100M instruction per-transaction limit.

## Tracking and calibration

Contracts cannot read their own instruction meter, so measured costs (from transaction
simulation or ledger meta) are reported by an Operator:

`record_compute_usage(reporter, operation, recipient_count, payer_count, item_count, actual_insns) -> OperationProfile`

Each report:

1. Scores the current calibrated estimate against `actual_insns` (`last_error_bps`). If the error is within 10%, `within_tolerance` goes up by one.
2. Adds the sample to the operation's totals, min, max and last.
3. Recomputes `calibration_bps = total_actual / total_raw_estimated`. This is what keeps estimates within 10% of measured usage.

## Caching

Estimates are cached in temporary storage, keyed by `(operation, recipients, payers, items)`.
A global `cost_ver` counter goes up on every model change, usage report or profile reset.
A cached entry is served (`cached = true`) only while its `model_version` still matches.

## Reporting and profiling

- `get_operation_profile(operation) -> Option<OperationProfile>`: raw profiling data.
- `get_compute_cost_report() -> Vec<OperationCostReport>`: one row per tracked operation, with average actual and estimated cost, min, max, calibration and `accuracy_bps` (the share of samples estimated within 10%).
- `reset_operation_profile(admin, operation)` (SuperAdmin) clears profiling and calibration for an operation.
- `get_operation_cost_model(operation)`

Each usage report emits a `cost_rec` event `(actual_insns, estimated_insns, error_bps)`.
