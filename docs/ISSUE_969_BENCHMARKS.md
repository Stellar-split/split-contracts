# Issue #969: Benchmarking suite (plan)

**Status:** plan only. No benchmark has been added or run.

## Current state

- `STORAGE_KEY_REGISTRY.md` mentions `cargo bench --bench storage_keys`, so a benchmark target for
  storage keys is expected but its source is not part of this change.
- `COMPUTE_BUDGETS.md` and `.github/workflows/compute-budget.yml` track budgets through simulation.
  Those are the numbers a reviewer uses today.

## Operations to benchmark

The hot paths are the operations called on every invoice: `create_invoice`, `deposit`, `release`,
`refund`, and the funding and fee calculations in `calc.rs`.

## Approach

1. Benchmark the pure functions first (`funding_bps`, `calc_platform_fee`). They run natively, so
   timings are stable.
2. For contract entry points, use the budget from simulation, not wall-clock time. Native timings say
   little about on-chain cost.
3. Record a baseline in `COMPUTE_BUDGETS.md` with the commit it was measured at.
4. In CI, fail a change that raises a baseline by more than an agreed margin.

## Open questions

- Which benchmark harness to use, since the crate has none set up.
- Whether the CI gate should block merges or only report.

## Not done

No benchmarks written, no baselines recorded, no CI job added.
