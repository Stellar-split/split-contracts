# Issue #1149: Invoice analytics and metrics (design)

**Status:** design only. Contract-wide usage analytics already exist (#1034, see
[`ISSUE_1034_USAGE_ANALYTICS.md`](./ISSUE_1034_USAGE_ANALYTICS.md)). This issue adds metrics for each
invoice, plus the aggregates over them that #1034 does not have.

## What already exists

| Piece | Where | What it gives |
|---|---|---|
| Usage totals and daily buckets | `contracts/split/src/analytics.rs` | Contract-wide counts and volumes for created, paid, released and refunded invoices, by UTC day. |
| Insights | `get_usage_insights()` | Averages, release and refund rates, week-over-week trend. |
| Per-invoice snapshot | `get_invoice_stats(invoice_id)` | `funded`, `total`, `payment_count`, `unique_payers`, `completion_bps`, `cumulative_contributed`. Worked out on each call from `invoice.payments`. |
| Funding progress | `get_invoice_funding_progress(invoice_id)` | Funding in basis points. |
| Global stats | `get_stats()` / `stats.rs` | Running totals. |
| Recording hook | `analytics::record`, called from the lifecycle helpers in `events.rs` | One place every lifecycle event goes through. |

## What is missing

1. **Time-based metrics per invoice.** How long from creation to first payment, to full funding, and to
   release. None of these are stored.
2. **Payment shape.** Largest payment and when the last payment arrived are not tracked.
3. **Aggregates over a group of invoices.** There is no way to ask "how are this creator's invoices
   doing" without reading every invoice.
4. **Cost.** `get_invoice_stats` loops over the whole `payments` vector, so it costs more the more
   payments an invoice has.

## Design

### Per-invoice metrics

One persistent entry per invoice, under key `(inv_met, invoice_id)`. It is written by the same hook as
#1034, so no lifecycle function needs to change.

```rust
#[contracttype]
pub struct InvoiceMetrics {
    pub created_at: u64,
    pub first_payment_at: u64,      // 0 until the first payment
    pub fully_funded_at: u64,       // 0 until funded >= total
    pub closed_at: u64,             // release, refund, cancel or expiry; 0 while open
    pub payment_count: u32,
    pub unique_payers: u32,
    pub largest_payment: i128,
    pub last_payment_at: u64,
    pub refunded_amount: i128,
}
```

- `unique_payers` uses one marker entry per `(invoice_id, payer)`, the same approach #1034 uses for
  addresses. This replaces the loop over `payments`.
- The durations (`time_to_first_payment`, `time_to_funded`, `time_to_close`) are worked out when
  read, not stored.

### Aggregates per creator

One persistent entry per creator, key `(crt_met, creator)`:

```rust
#[contracttype]
pub struct CreatorMetrics {
    pub invoices: u32,
    pub open: u32,
    pub released: u32,
    pub refunded: u32,
    pub cancelled: u32,
    pub total_volume: i128,
    pub funded_volume: i128,
    pub sum_time_to_funded: u64,    // divided by `released` to get the average
}
```

Each state change updates this entry with saturating adds. Reads never loop over invoices.

### Tracked events

Add `UsageKind::InvoiceCancelled` and `UsageKind::InvoiceExpired` to `analytics::record`, so the
per-invoice `closed_at` covers every way an invoice can close. As in #1034, recording publishes no new
events. Saturating counters mean recording can never make the call being recorded fail.

### Queries

| Function | Access | Cost |
|---|---|---|
| `get_invoice_metrics(invoice_id)` | anyone | 1 read |
| `get_invoice_insights(invoice_id)` | anyone | 1 read; returns the durations and `completion_bps` |
| `get_creator_metrics(creator)` | anyone | 1 read |
| `get_invoice_metrics_batch(invoice_ids)` | anyone | 1 read per id, capped at 50 ids |

`get_invoice_stats` stays as it is for compatibility. Once the new entries exist it can read
`payment_count` and `unique_payers` from them.

### Switch and migration

- The existing `set_analytics_enabled` switch also controls these metrics.
- Invoices created before this change have no metrics entry. Reads return metrics built from the
  invoice itself (`created_at` set to 0 to mark "unknown"). No backfill is needed.

## Cost

A payment adds one persistent write for the invoice metrics. On a payer's first payment to that
invoice it adds one more for the marker. A state change adds one write for the creator entry.

## Test plan (`test_1149_*`)

- The metrics after create, first payment, more payments, full funding and release match the expected
  timestamps and counts.
- A payer paying twice is counted once in `unique_payers`.
- Refund, cancel and expiry each set `closed_at` and update the right creator counter.
- With analytics switched off, no metrics are written and reads still work.
- An invoice created before the change returns metrics built from the invoice and does not fail.
- `get_invoice_metrics_batch` with more than 50 ids is refused.

## Acceptance criteria

| Criterion | Met by |
|---|---|
| Metrics collected | `InvoiceMetrics`, `CreatorMetrics` written from `analytics::record` |
| Queries efficient | Every query reads a fixed number of entries; no loop over payments or invoices |
| Aggregation works | `CreatorMetrics`, plus the existing #1034 totals and daily buckets |
| Events tracked | The new `UsageKind` values for cancel and expiry |
| Insights useful | Time to first payment, time to funded, time to close, average time to funded per creator |
| All tests pass | `cargo test -p split test_1149` |

## Open questions

- Should aggregates per token be added too? This design leaves them out, since #1034 already mixes
  volumes across tokens.
- Should metrics for archived or purged invoices be deleted with them? This design keeps them.
