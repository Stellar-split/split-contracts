# Issue #1157: Metadata Version Comparison Tests

## Overview

Test plan for invoice metadata versioning: tracking each change to an invoice's off-chain metadata hash, comparing versions, and rebuilding the full change history.

## Current Model

The contract stores only the **latest** metadata hash per invoice. Version history is reconstructed from two append-only sources:

| Source | Location | Data |
|---|---|---|
| Current hash | `metadata_hash_key(invoice_id)` (persistent) | `BytesN<32>` |
| Audit log | `get_audit_log(invoice_id)` | `AuditEntry` with action `meta_upd` |
| Event | `events::metadata_updated` | topics `(split, meta_upd, invoice_id)`, data `(old_hash, new_hash, ledger_sequence)` |

A **version** is the 1-based position of an update in the invoice's `meta_upd` audit entries. Version 0 is the hash set at creation, if one was set.

## Functions Under Test

#### `update_metadata_hash(invoice_id, creator, new_hash)`
- Creator-only; rejected while the contract is paused
- Rejected on `Released` or `Cancelled` invoices
- Reads the old hash, writes the new one, appends a `meta_upd` audit entry and emits `metadata_updated(old, new, ledger)`

#### Read paths
- `get_invoice(invoice_id).metadata_hash` is filled from `metadata_hash_key`
- `get_audit_log(invoice_id)` returns the full history

## Comparison Rules Tested

- **Equality**: two versions are equal when their hashes are byte-equal
- **Ordering**: a later audit entry or larger ledger sequence means a newer version
- **Chain integrity**: for each event N, `old_hash` equals `new_hash` of event N-1. The first update's `old_hash` is the creation hash, or `None`.
- **Latest**: the current stored hash equals the `new_hash` of the last event

## Test Cases

Add to `contracts/split/src/test.rs`.

### Version tracking
- `test_metadata_initial_version_none`: invoice created without a hash; `metadata_hash == None`
- `test_metadata_initial_version_from_creation`: invoice created with a hash; the first update's `old_hash` is that hash
- `test_metadata_each_update_appends_audit_entry`: N updates produce N `meta_upd` entries

### Comparison logic
- `test_metadata_event_old_new_chain`: after 3 updates, each event's `old_hash` equals the previous event's `new_hash`
- `test_metadata_same_hash_update`: setting the same hash again still records a version and the event shows `old == new` (pins current behaviour)
- `test_metadata_versions_distinct_across_invoices`: updates on invoice A do not appear in B's history

### Version queries
- `test_metadata_latest_matches_get_invoice`: the stored hash equals the last event's `new_hash`
- `test_metadata_query_cost_constant`: reading the current hash is a single storage read, independent of history length (checked with the budget, see `docs/COMPUTE_BUDGETS.md`)

### History
- `test_metadata_history_complete_after_many_updates`: 20 updates; the audit log and events both contain all 20 in order
- `test_metadata_history_ledger_sequence_monotonic`: advance the ledger between updates; sequences never decrease
- `test_metadata_history_survives_other_ops`: interleave fund and adjust calls; metadata entries are not lost or reordered

### Edge cases and rejection
- `test_metadata_update_non_creator_panics`: `"only creator can update metadata hash"`
- `test_metadata_update_released_panics`
- `test_metadata_update_cancelled_panics`
- `test_metadata_update_when_paused_panics`
- `test_metadata_rejected_update_leaves_history_unchanged`: no audit entry, event or hash change on failure
- `test_metadata_update_on_funded_pending_invoice_allowed`: funding does not block updates

## Notes

- The contract does not validate or fetch off-chain content; tests use arbitrary `BytesN<32>` values.
- If audit log retention or capping is added later, history-completeness tests must use events as the source of truth.

## Acceptance Criteria

- [ ] Versions tracked
- [ ] Comparison accurate (hash chain verified)
- [ ] Queries efficient (constant-cost latest read)
- [ ] History complete
- [ ] All tests pass (`cargo test -p split`)
- [ ] No data loss (rejected updates leave state untouched)
