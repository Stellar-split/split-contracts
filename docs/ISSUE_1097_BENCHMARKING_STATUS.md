# Issue #1097: Performance benchmarking suite (status)

**Status:** not met. The plan is in [`ISSUE_969_BENCHMARKS.md`](./ISSUE_969_BENCHMARKS.md) and is still
a plan. This note records what exists today that serves the same purpose, and what is missing.

## What exists

| Need | What covers it today |
|---|---|
| Metrics collection | On-chain: `record_compute_usage` and `get_compute_cost_report` keep min, max and average instruction counts per operation (see [`ISSUE_1095_COMPUTE_BUDGET_ESTIMATION_STATUS.md`](./ISSUE_1095_COMPUTE_BUDGET_ESTIMATION_STATUS.md)). |
| Baselines | [`COMPUTE_BUDGETS.md`](./COMPUTE_BUDGETS.md) lists the estimator's resource ranges per operation. |
| Budget check in CI | `.github/workflows/compute-budget.yml` runs `cargo test --package split -- compute_budget` on pull requests, and `tests/test_compute_budget.rs` holds the test. |
| Hot-path tests | `contracts/split/src/perf_regression_tests.rs` exists but is switched off in `lib.rs` because it depends on `std`. |

## What is missing

- A benchmark target. The workspace has no `benches/` directory and no harness. `STORAGE_KEY_REGISTRY.md`
  mentions `cargo bench --bench storage_keys`, but that target does not exist.
- Trend tracking. Nothing stores results between runs, so there are no trends or reports.
- Recorded baselines tied to a commit.

The pure functions the plan wants to start with (`funding_bps`, `calc_platform_fee`) are in `mod calc`,
which is private, so a separate bench crate cannot call them without exposing the module.

## What this change does

Records the status above and leaves the plan in `ISSUE_969_BENCHMARKS.md` as the next step. It changes
no code, adds no benchmark and no CI job, and was not run.
