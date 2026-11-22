# Issue #1100: Event emitter for state changes (status)

**Status:** covered for status transitions. The contract emits a state-change event, has typed event
helpers, and offers filtering through webhook event masks.

## Requirements against the code

| Requirement | Where it is |
|---|---|
| Event system | `contracts/split/src/events.rs` holds one helper per event (about 180). Entry points call the helper and do not publish directly, see [`adr/0003-observability-from-event-helpers.md`](./adr/0003-observability-from-event-helpers.md). |
| Emitter | `events::invoice_state_changed(env, invoice_id, from_status, to_status, actor)` publishes topics `(split, st_chg, invoice_id)` and data `(from_status, to_status, actor, ledger)`. It is called from 24 places in `lib.rs`. |
| Event types | One topic per event, for example `created`, `PayoutFailed`, `InstalmentTranchePaid`, `st_chg`, `wh_stat`, `cost_rec`. The status values reported by `st_chg` are `none`, `pending`, `released`, `refunded`, `expired`, `cancld`, `disputed` and `part_rel`. |
| Event filtering | Because the invoice id is the last topic of `st_chg`, an indexer can filter on it. For subscribers, `register_webhook` takes an `event_mask` built from `EVENT_CREATED`, `EVENT_PAID`, `EVENT_RELEASED`, `EVENT_REFUNDED` and `EVENT_CANCELLED`, plus an optional invoice id. |
| Event stats | Usage totals through `get_usage_totals` and `get_usage_report` (counted from the event helpers), and webhook delivery records |

## Notes

- A state transition is reported only if its code path calls `invoice_state_changed`. A path that sets
  `invoice.status` directly without calling it is not covered. A full audit of all transitions needs a
  walk through every assignment to `status` in `lib.rs`, which this note does not claim to have done.
- `EVENT_ALL` covers five lifecycle events. Disputes, pauses and tranche releases are not available as
  webhook masks, so a subscriber has to read the ledger events for those.

## What this change does

Records the status above. It changes no code. The behaviour was read, not run, and no tests were added.
