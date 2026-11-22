# Issue #1096: Data archival strategy (status)

**Status:** covered. A scheduled archival sweep, restore, purge, status and statistics exist (issue
#927). An earlier note on the manual entry points is in
[`ISSUE_977_INVOICE_ARCHIVAL_STATUS.md`](./ISSUE_977_INVOICE_ARCHIVAL_STATUS.md).

## Requirements against the code

| Requirement | Where it is in `contracts/split/src/lib.rs` |
|---|---|
| Archival system | `set_archival_policy(admin, policy)` (Operator or higher) sets `enabled`, `retention_ledgers`, `max_batch` (1 to 50), `sweep_interval_ledgers` and `auto_purge`. `archive_invoice` and `archive_invoices_batch` archive on request. |
| Archive logic | `run_archival_sweep()` is permissionless so keepers can call it, and is limited to one run per `sweep_interval_ledgers`. It scans up to `max_batch` ids from a stored cursor, archives each eligible invoice, and purges old archived ones when `auto_purge` is set. |
| Restore capability | `restore_archived_invoice(caller, invoice_id)` moves the invoice, its extensions, payment shards, audit log and creation ledger back to persistent storage. The creator or an Operator may call it. A purged invoice cannot be restored, and a restored invoice is held out of re-archival for `restore_hold_ledgers`. |
| Queries on the archive | `is_archived(invoice_id)`, `get_archived_invoice(invoice_id)`, and `get_archival_status(invoice_id)`, which reports `Active`, `Archived`, `Restored` or `Purged` with the ledger and actor of each step. `get_audit_log` keeps working for an archived invoice. |
| Archive stats | `get_archival_stats()` returns counters and the sweep cursor. |
| No data loss | Archiving moves the invoice records, and `restore_archived_invoice` moves them back. `purge_archived_invoices(admin, ids)` (Operator, at most 20 ids per call) is the only step that deletes data, and only after `retention_ledgers`. |

Events: `invoice_archived`, `invoice_restored`, `archival_policy_updated`, `archival_sweep_completed`.

## Notes

- Purging is permanent. With `auto_purge` on, a sweep can delete archived invoices that passed the
  retention period, so the policy value should be set with the compliance retention rules in mind.
- Archival is also lazy. `load_invoice` calls `maybe_archive_invoice`, which archives a `Released` or
  `Refunded` invoice older than `ARCHIVE_AFTER_LEDGERS` (100,000, or the value from
  `set_archive_after_ledgers`) the next time anything reads it, so a read can write storage. A
  recently restored invoice is held out of this for `restore_hold_ledgers`.
- `archive_invoice_storage` moves the invoice records, payment shards and audit log from persistent
  storage into **instance** storage, and `restore_archived_invoice` moves them back. Instance storage is
  loaded with the contract on every call and is a single ledger entry with a size limit, so a large
  number of archived invoices grows what every call reads. ADR 0001 says authoritative per-entity state
  belongs in persistent storage. The storage layout is described in
  [`../STORAGE_KEY_REGISTRY.md`](../STORAGE_KEY_REGISTRY.md) and
  [`adr/0001-storage-tiers.md`](./adr/0001-storage-tiers.md). Whether instance storage is the intended
  archive tier should be confirmed against that ADR.

## What this change does

Records the status above. It changes no code. The behaviour was read, not run, and no tests were added.
