# Issue #1091: Query result caching (status)

**Status:** the cache API exists (issue #1014). Reading it for this issue shows four defects that
stop it meeting the acceptance criteria. They are listed here and not fixed in this change, because
each one changes storage keys or access rules that existing callers and snapshots depend on.

## Requirements against the code

| Requirement | Where it is | Result |
|---|---|---|
| Cache system | `cache_set(key, value, ttl_seconds)` and `cache_get(key)` in `contracts/split/src/lib.rs`, entries stored as `CacheEntry` | Present. |
| Invalidation | `cache_invalidate(key)` marks the entry `valid = false`, counts it and emits `cache_invalidated` | Manual only. See defect 3. |
| TTL | `expires_at = now + ttl_seconds`; `cache_get` treats an entry past `expires_at` as a miss. A TTL of `0` never expires | Present. The TTL is in seconds of ledger time. |
| Hit monitoring | `get_cache_metrics` returns hits, misses, invalidated, warmed, `entry_count` and `hit_ratio_bps` | Present. |
| Cache warming | `cache_warm(invoice_ids)` | Does not work. See defect 1. |

## Defects found

1. **`cache_warm` caches nothing useful.** For every id it writes the same key (`"inv"`) with an empty
   `Bytes` value (the code comment says "Placeholder; real serialization would go here"), so after
   warming N invoices there is one empty entry and `warmed` is still incremented N times. A fix needs a
   per-invoice key (a `Symbol` built from the id, such as `inv_<id>`) and the invoice's XDR as the value.
2. **No access control.** `cache_set`, `cache_invalidate` and `cache_warm` take no address and call no
   `require_auth`. Any account can overwrite or invalidate any cache key. `create_query_index`, in the
   same section, does call `require_admin`.
3. **Nothing invalidates on change.** No invoice mutation (pay, release, cancel, refund) calls
   `cache_invalidate`, so a cached invoice view can be stale until its TTL passes.
4. **The query indexes are never written.** `get_invoices_by_status` and `get_invoices_by_creator`
   read `query_status_index_key` and `query_creator_index_key`, but no code writes them, and
   `create_query_index` only increments a counter. Both queries return an empty list.

Also, `cache_get` panics on a miss, so a client has to treat a failed call as a miss.

## What this change does

Records the status and defects above. It changes no code. The behaviour was read, not run, and no
tests were added.
