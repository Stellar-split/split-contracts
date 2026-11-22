# Issue #959: Performance metric events (design scaffold)

**Status:** design only. No metric events are emitted yet.

## Goal

Emit events that report compute cost and storage usage, so monitoring can index them.

## What can and cannot be measured on-chain

- **Storage usage** can be counted inside the contract: the number of entries a call wrote, and
  the number of invoices in a list. These are exact.
- **Compute cost** is not readable from inside a Soroban contract. Budget figures come from
  simulation (`compute-budget.yml` and `COMPUTE_BUDGETS.md`), not from the contract. So a metric
  event can carry a counter the contract knows, but not the instruction count.

## Proposed event

```rust
env.events().publish(
    (symbol_short!("metric"), symbol_short!("write")),
    (invoice_id, entries_written, env.ledger().timestamp()),
);
```

One topic pair per operation family (`write`, `read_scan`), so an indexer can filter on the
topic without decoding the data.

## Open questions

- Whether the compute figure should come from the existing budget workflow, published alongside
  the event by a separate tool.
- Event volume: one event per write adds cost to every call. A sampled or opt-in mode may be needed.

## Not done

No code, no tests, no indexer schema.
