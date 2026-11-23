# Issue #1087: Audit logging for sensitive operations (status)

**Status:** per-invoice mutation logging exists and is append-only. The richer compliance log in
`audit_logging.rs` is declared but nothing writes to it.

## Requirements against the code

| Requirement | Where it is | Result |
|---|---|---|
| Log invoice modifications with actor and timestamp | `append_audit_entry(env, id, action, actor)` in `contracts/split/src/lib.rs` stores an `AuditEntry { action, actor, timestamp }` and is called from about 90 places: payments, pause and resume, co-creators, split changes, disputes, metadata updates, releases | Present. |
| Log queries | `get_audit_log(invoice_id)` returns the full list for one invoice | Present. No paging and no filter by actor or action. |
| Persistent | Entries live in persistent storage under `audit_log_key(id)`. For an archived invoice they move to instance storage. | Present. |
| Immutable | No entry point removes or edits an entry. Entries are only appended. | Holds for the per-invoice log. |
| Track all mutations | Not complete. See below. | Partial. |
| Analysis | None for the per-invoice log | Not present. |

## The compliance log

`contracts/split/src/audit_logging.rs` (issue #1009) adds a contract-wide log with sequence numbers,
event kinds, a `data_hash`, reader roles, `count_by_kind`, `entries_in_range` and stats. It is declared
as `pub mod audit_logging;` but `lib.rs` never calls `audit_logging::append`, so it records nothing.
It is also not immutable by design: it evicts the oldest entry past 500 and `purge_expired` deletes
entries older than the retention window.

## Gaps

- **Unbounded growth.** The per-invoice log is a single `Vec` that grows with every action. A busy
  invoice makes every append read and rewrite the whole list. `set_max_audit_log_entries` writes a
  limit, but `append_audit_entry` does not read it, so the limit has no effect.
- **Not every mutation logs.** Admin configuration calls such as `set_treasury`, `set_creation_fee` and
  `set_protocol_fee` do not append an entry. The timelocked admin queue (`set_tl`, `queue`, `cancel`)
  logs against invoice id `0`.
- **No ledger sequence** in `AuditEntry`, only the timestamp.

## What this change does

Records the status above. It changes no code. The behaviour was read, not run, and no tests were added.
