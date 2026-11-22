# Architecture decision records

Each record explains one design decision: the context, the decision, and its consequences.

| # | Title | Status |
|---|---|---|
| [0001](./0001-storage-tiers.md) | Storage tiers by access pattern | Accepted |
| [0002](./0002-single-contract-with-modules.md) | One `split` contract, entry points in `lib.rs`, logic in modules | Accepted |
| [0003](./0003-observability-from-event-helpers.md) | Record usage analytics from the lifecycle event helpers | Accepted |
| [0004](./0004-split-invoice-records.md) | Split invoice records across several keys, with a hot-path cache | Accepted |
| [0005](./0005-alert-delivery.md) | Deliver alerts as events plus an optional receiver contract | Accepted |

## Adding a record

1. Copy the structure of an existing record.
2. Number it with the next free four-digit number.
3. Add a row to the table above.
4. Mark a decision that replaces another one as superseded, and link the new record.

The architecture overview that links these records is [`../ARCHITECTURE.md`](../ARCHITECTURE.md).
