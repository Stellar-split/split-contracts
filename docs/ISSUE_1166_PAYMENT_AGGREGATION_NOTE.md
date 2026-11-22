# Issue #1166: payment aggregation engine

`contracts/split/src/aggregation.rs` (issue #1022) already provides a bounded
request aggregator: batching up to `MAX_BATCH`, window timeouts, grouping by
target with overflow-checked sums, result distribution (`Flush::group_of`) and
`AggregationMetrics::operations_saved`. No new code is added for #1166.

Remaining gaps against the issue:

- No on-chain storage or contract entrypoints: the aggregator is an in-memory helper.
- No query API over historical aggregations (only current metrics).
- No persisted tracking of past aggregations.
- Fee/cost reduction is reported as operations saved, not measured in fees.
- No integration with the payment path to actually submit consolidated transfers.
