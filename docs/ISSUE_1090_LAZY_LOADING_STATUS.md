# Issue #1090: Lazy loading for invoice history (status)

**Status:** partly covered. On-demand, paged reads exist for the histories a client asks for. A
generic lazy-load cache exists as a library file, but it is not part of the contract build.

## Requirements against the code

| Requirement | Where it is | Notes |
|---|---|---|
| Lazy load system | Every `get_*` read in `contracts/split/src/lib.rs` loads one record at a time. Nothing is preloaded. | `load_invoice` reads one invoice. Payments are kept in shards (`read_shard`) and read per shard. |
| Pagination | `list_invoices(cursor, limit)`, `get_payer_history(payer, offset, limit)`, `get_recipient_invoice_ids_page(recipient, offset, limit)` | Limits are clamped (100, none, and 50). See [`ISSUE_1092_STORAGE_PAGINATION_STATUS.md`](./ISSUE_1092_STORAGE_PAGINATION_STATUS.md). |
| Caching layer | `cache_set`, `cache_get`, `cache_invalidate`, `cache_warm`, `get_cache_metrics` | See [`ISSUE_1091_QUERY_CACHING_STATUS.md`](./ISSUE_1091_QUERY_CACHING_STATUS.md). |
| Filtering | `get_invoices_by_status`, `get_invoices_by_creator` | These read an index that nothing writes yet. See the 1091 note. |
| Load stats | `get_cache_metrics`, `get_query_metrics` | Hits, misses, invalidations and warmed entries are counted. |

## The generic lazy loader

`contracts/split/src/lazy_loader.rs` (issue #1018) is a bounded read-through cache: `get_or_load`,
`prefetch`, `invalidate`, `invalidate_all`, ledger-based TTL, least-recently-used eviction and
`LoadMetrics` with a hit rate in basis points. It is heap free and holds plain `Copy` values.

`lib.rs` has no `mod lazy_loader;`, so the file is not compiled into the contract and nothing calls
it. That fits how a contract runs: each invocation is a separate transaction, so an in-memory cache
does not survive between calls. It is useful to off-chain tooling that links the crate as a library.

## Gaps

- Invoice history is not exposed as one filtered, paged query. A client combines `list_invoices`
  (all invoices, no filter) with `get_invoice_status` per id, or uses the status and creator
  indexes once those are populated.
- The cursor in `list_invoices` skips ids whose invoice was deleted or archived, so a page can be
  shorter than `limit` even when more ids follow. Callers should stop on an empty page, not a short one.

## What this change does

Records the status above. It changes no code. The behaviour was read, not run, and no tests were added.
