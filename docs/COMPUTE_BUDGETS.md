# Compute Budget Reference

Measured instruction counts for typical inputs using `estimate_compute()`.
Soroban instruction limit: **100,000,000** per transaction.

## Function Budget Table

| Function | 1 Recipient | 5 Recipients | 20 Recipients | % of Limit (20) |
|---|---|---|---|---|
| `create_invoice` | 1,200,000 | 2,000,000 | 5,000,000 | 5.0% |
| `pay` | 1,800,000 | 1,800,000 | 1,800,000 | 1.8% |
| `pay_invoice_delegated` | 1,800,000 | 1,800,000 | 1,800,000 | 1.8% |
| `release` | 2,300,000 | 3,300,000 | 11,300,000 | 11.3% |
| `get_invoice` | 250,000 | 250,000 | 250,000 | 0.25% |
| `get_leaderboard` | 500,000 | 500,000 | 500,000 | 0.5% |
| `get_stats` | 500,000 | 500,000 | 500,000 | 0.5% |

## Notes

- Values produced by `estimate_compute(function_name, recipient_count)`.
- A **warning event** (`split/bdgt_w`) is emitted when a function exceeds 80,000,000 instructions (80% of limit).
- CI benchmark runs `estimate_compute` on each PR and posts a budget table as a comment (see `.github/workflows/compute-budget.yml`).

## Formula

| Stage | Cost |
|---|---|
| Base overhead | 1,000,000 instructions |
| Per recipient (release) | +500,000 instructions |
| Per payment shard (8 shards) | +100,000 instructions each |
| Per recipient (create) | +200,000 instructions |
