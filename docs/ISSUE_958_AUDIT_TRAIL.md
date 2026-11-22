# Issue #958: State transition audit trail (design scaffold)

**Status:** design only. Nothing in the contracts records transitions yet.

## Goal

Keep a complete, ordered log of every invoice state change, with a timestamp and the actor, so a
reviewer can replay an invoice's history.

## Proposed shape

```rust
#[contracttype]
pub struct InvoiceTransition {
    pub invoice_id: u64,
    pub from: InvoiceStatus,
    pub to: InvoiceStatus,
    pub at: u64,        // ledger timestamp, seconds
    pub actor: Address, // caller, or the contract for automatic transitions
}
```

Stored per invoice under a key in the `StorageKey` family (see
[`STORAGE_KEY_CONVENTIONS.md`](./STORAGE_KEY_CONVENTIONS.md)), with an index counter so entries
are ordered and replay is a single forward read.

## Where transitions happen

In `contracts/invoice-escrow/src/lib.rs`: `create_invoice`, `deposit` (when the invoice fills),
`release`, `refund`, `cancel_invoice`, `blacklist_payer`, `finalise_blacklist`, and the transfer of
admin. Each would call one `record_transition` helper, so no call site writes the log itself.

## Replay

Replay folds the log from the first entry. It checks that each `from` equals the previous `to`,
and that the final `to` equals the stored status. A mismatch means a transition was not logged.

## Open questions

- Storage cost: an unbounded per-invoice log grows with every transition. A bounded log, or events
  only, may be enough for most users.
- Whether the log is contract storage or events only. Events are cheaper but not replayable from
  the contract alone.

## Not done

No code, no tests, no replay function.
