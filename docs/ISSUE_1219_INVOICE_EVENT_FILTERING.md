# Issue #1219: invoice event filtering and querying

`contracts/split/src/invoice_event_filtering.rs` is a first slice of pure
helpers for filtering a bounded in-memory log of normalised invoice events.

Implemented:

- `InvoiceEvent` / `InvoiceEventType` — normalised view of the events emitted
  by `events.rs` (type mirrors the `split` action topics).
- `InvoiceEventLog` — bounded append-only log (`MAX_EVENTS`), rejects when
  full.
- `EventFilter` — composite query; every set field is AND-ed (event type,
  invoice id, inclusive date range, `offset`/`limit`).
- `query` — collects matches with `offset`/`limit` and early-exits once the
  limit is reached; `count` counts without materialising.
- `filter_by_type` / `filter_by_date_range` shorthands (inverted range
  rejected).

Remaining gaps against the issue:

- Not wired to contract storage or entrypoints; events are projected in by an
  indexer, not read from the on-chain event stream.
- No persisted log across transactions (in-memory only).
- No sort/binary-search optimisation for range queries (linear scan with
  early-exit).
- No integration with `contribute`/`release` to populate the log live.
