# Issue #1120: Payment hold period (status)

**Status:** covered by two existing mechanisms, which are described below. A third, `claim_delayed_payout`, is
not reachable. The design note [`ISSUE_1001_PAYMENT_HOLDS_DESIGN.md`](./ISSUE_1001_PAYMENT_HOLDS_DESIGN.md) is
about a different idea, a hold on part of a payment, and is still only a design.

Both mechanisms hold the whole release of an invoice after it is fully funded. Neither holds a part of one
recipient's share.

## Mechanism 1: escrow hold period

| Requirement | Where it is |
|---|---|
| Configurable | `escrow_hold_period: Option<u32>` in the invoice options (`options.ext`), in ledgers, set when the invoice is created. |
| Hold starts | In `_pay`, when `funded >= total` and the invoice has a period, `held_until = current ledger + period` is stored and `escrow_hold_started` is emitted. |
| Hold enforced | The release path panics with `EscrowHoldActive` while the ledger is before `held_until` (`_release_invoice_inner` and `release_tranche`). A funded invoice with a hold is never auto-released, because `held_until` makes the payment path hand over to a manual `release`. |
| Query | `is_invoice_releasable(invoice_id)` returns `false` during a hold. `held_until` is part of the invoice's extended record and of `get_invoice_snapshot`. |
| Release | The hold ends by itself. `resolve_escrow(creator, invoice_id, resolution_hash)` (creator or a co-creator) clears `held_until` early and emits `escrow_resolved`. |
| Status | `held_until` is `Some` during the hold and `None` before it starts or after it is resolved. Events `escrow_hold_started` and `escrow_resolved` mark the changes. |

## Mechanism 2: release delay

| Requirement | Where it is |
|---|---|
| Configurable | `release_delay_ledgers: Option<u32>` in the invoice options. Creation panics above 100,000. |
| Hold starts | `funded_at_ledger` is stored the first time the invoice is fully funded. |
| Hold enforced | `release` panics with `FundsLockedUntil` before `funded_at_ledger + delay`. |
| Query | `get_invoice_ext3(invoice_id)` returns `release_delay_ledgers`, `funded_at_ledger` and `unlock_at_ledger`. |
| Release | The delay runs out by itself. There is no early release. `funds_unlocked` is emitted when a release first passes the check. |

## What does not work

- **`claim_delayed_payout` can never succeed.** It reads a `DelayedPayout` record per recipient, but nothing in
  the contract writes one, so it always panics with `no delayed payout found`. Issue #437 added the claim but not the
  step that would create the record on release. A per-recipient hold would need `_release_full` to store a
  `DelayedPayout` instead of transferring, which changes who holds the funds, so it is not done here.
- **The two mechanisms are independent.** An invoice can have both, and then both must pass.
- **`escrow_hold_period` has no upper bound.** Creation does not limit it, while `release_delay_ledgers` is
  limited to 100,000. A creator can set a very long hold, but can also end it with `resolve_escrow`, so the
  recipients depend on the creator for it. If that is not intended, the hold should be bounded, or released by an
  admin or arbiter.
- **`resolve_escrow` works when no hold is active** and needs only the creator's signature and a hash. It records
  the hash, and there is no check that a hold existed.

## Tests

I found no tests that mention `held_until`, `escrow_hold_period`, `EscrowHoldActive`, `FundsLockedUntil`,
`resolve_escrow` or `claim_delayed_payout` in `test.rs`. None are added here, because this is a feature issue.

## What this change does

Records the status above. It changes no code. The behaviour was read, not run.
