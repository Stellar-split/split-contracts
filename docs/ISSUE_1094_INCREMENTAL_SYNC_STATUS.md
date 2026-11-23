# Issue #1094: Incremental sync (status)

**Status:** the sync system the issue describes is off-chain. This contract gives an indexer the
pieces for a delta sync, but has no "what changed since X" query and stores no sync state.

## Requirements against the code

| Requirement | What the contract offers | Gap |
|---|---|---|
| Delta tracking | The event stream. `events::invoice_state_changed` publishes `(split, st_chg, invoice_id)` with `(from_status, to_status, actor, ledger)` on every status transition. The lifecycle events (`invoice_created`, `payment_received`, `invoice_released`, `invoice_refunded`) carry the invoice id. | Invoices carry a creation ledger, but no "last changed" ledger. An indexer must read events, not poll invoice records, to find changes. |
| Sync coordination | `list_invoices(cursor, limit)` pages through ids in ascending order. `get_invoice_snapshot` and `get_payment_root(invoice_id)` return a state fingerprint a client can compare with its own copy. | No cross-invoice change log or "sync token". |
| Resume | A client resumes from the last event ledger it processed, or from the last invoice id it listed. | Nothing is stored on chain, so resume state lives with the client. |
| Sync stats | None in the contract | Belongs with the indexer. |

## Recommended client approach

1. On first run, page `list_invoices` from cursor `0` and read each invoice once.
2. After that, subscribe to contract events from the last processed ledger. For each event, refetch only
   the invoice id it names.
3. Periodically compare `get_payment_root(invoice_id)` for a sample of invoices to catch missed events.
4. Event retention on the network is limited, so keep the last processed ledger and fall back to a
   full `list_invoices` pass if the gap is larger than the retention window.

## What this change does

Records the status above. It changes no code. The behaviour was read, not run, and no tests were added.
The issue's criteria on bandwidth, resume and statistics describe an indexer or client, which is not in
this repository.
