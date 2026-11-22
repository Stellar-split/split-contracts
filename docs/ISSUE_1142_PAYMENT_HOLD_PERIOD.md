# Issue #1142: Payment hold period

**Status:** `escrow_hold_period` is implemented on the payment path; the
hold-expiry check on `release` is partial; the design is not documented.

---

## Goal

Let a creator configure a mandatory waiting period between the moment an
invoice becomes fully funded and the moment funds can be released to recipients.
This gives payers a window to raise a dispute, request a refund, or perform
off-chain verification before money leaves the contract.

---

## Background: existing implementation

`InvoiceCore` (and the extended options structs) already carries:

```rust
pub escrow_hold_period: Option<u32>,  // duration in ledgers
pub held_until: Option<u32>,          // absolute ledger sequence; set when funding completes
```

Inside `_pay`, when an invoice reaches full funding:

```rust
if let Some(hold) = invoice.escrow_hold_period {
    if invoice.held_until.is_none() {
        let unlock = env.ledger().sequence().saturating_add(hold);
        invoice.held_until = Some(unlock);
        events::escrow_hold_started(env, invoice_id, unlock);
    }
}
```

The `guarded` flag in the auto-release block is set to `true` when
`invoice.held_until.is_some()`, which suppresses automatic release on the
funding transaction.

The existing design notes are in
[`ISSUE_1001_PAYMENT_HOLDS_DESIGN.md`](./ISSUE_1001_PAYMENT_HOLDS_DESIGN.md),
which describes a more general hold-and-reserve model. This issue is narrower:
it covers only the per-invoice escrow hold period that is already partially
wired.

---

## What is missing

### 1. Hold expiry not enforced at `release` time

`_release` (the internal helper called by `release`) does not check
`invoice.held_until` before transferring funds. A creator can call `release`
while the hold is still active and bypass the waiting period entirely.

**Required:** add this check to `_release`, after the circuit-breaker and
pause checks and before the status check:

```rust
if let Some(held_until) = invoice.held_until {
    assert!(
        env.ledger().sequence() >= held_until,
        "hold period has not elapsed"
    );
}
```

The error should use `ContractError::HoldPeriodNotElapsed` (a new error
variant). See `ERROR_CODES.md` for the numbering convention.

### 2. No query for hold state

There is no entry point that returns the current hold state for an invoice —
when the hold unlocks, whether it has elapsed, and how many ledgers remain.

**Required:** a `get_invoice_hold_status(invoice_id)` entry point returning:

```rust
pub struct InvoiceHoldStatus {
    pub invoice_id: u64,
    pub hold_period_ledgers: Option<u32>,
    pub held_until_ledger: Option<u32>,
    pub current_ledger: u32,
    pub hold_active: bool,
    pub ledgers_remaining: u32,
}
```

`hold_active` is `true` when `held_until` is `Some` and
`current_ledger < held_until`. `ledgers_remaining` is
`held_until.saturating_sub(current_ledger)`, or 0 when the hold has elapsed.

### 3. `held_until` is not reset after hold elapses

Once `held_until` is set it is never cleared, even after the hold has elapsed
and the invoice is released. This is benign (the field is ignored once the
invoice is terminal) but adds persistent storage cost and may confuse indexers.

**Recommended:** clear `held_until` (set to `None`) when `release` succeeds,
writing the updated invoice back to storage.

### 4. No `escrow_hold_elapsed` event

The `escrow_hold_started` event is emitted when the hold begins but there is
no corresponding event when the hold expires (i.e. when
`env.ledger().sequence() >= held_until`). Indexers currently have no on-chain
signal that a held invoice has become releasable.

**Required:** emit `("split", "hold_end", invoice_id)` with data
`(unlock_ledger, current_ledger, event_seq)` inside `release` or `_release`
the first time the hold condition is found to have elapsed (i.e. the first
successful `release` call after the hold clears). Alternatively, emit it at
the top of `_release` when the hold check passes.

### 5. Hold period cannot be updated after creation

`escrow_hold_period` is set at `create_invoice` time and cannot be changed.
If the creator misconfigures it, the invoice is stuck until the hold elapses.

**Recommended:** add `set_invoice_hold_period(creator, invoice_id, new_period)`
that requires `Pending` status and creator auth. If the new period is `None`,
the hold is cancelled. Document that changing the hold period after full
funding does not reset `held_until` — the hold end is computed once, at
funding time.

---

## Model

```
create_invoice(escrow_hold_period = Some(N ledgers))
       │
       ▼
  Pending ──pay()──► fully funded ──► held_until = current_ledger + N
                                             │
                                             │  (N ledgers pass)
                                             ▼
                                    release() allowed
                                             │
                                             ▼
                                        Released
```

The hold begins at full funding, not at invoice creation. Partial payments do
not start the clock. If additional payments arrive after the invoice is fully
funded (overfunding case), the `held_until` value is not recalculated — it is
set once and left unchanged.

---

## New error code

| Code | Constant | Meaning |
|---|---|---|
| (next available) | `HoldPeriodNotElapsed` | `release` called before `held_until` ledger |

Add to `error.rs` and `ERROR_CODES.md`.

---

## New type

```rust
/// Issue #1142: Hold state for an invoice with an escrow hold period.
#[contracttype]
pub struct InvoiceHoldStatus {
    pub invoice_id: u64,
    /// The configured hold duration in ledgers, or None if no hold was set.
    pub hold_period_ledgers: Option<u32>,
    /// The ledger sequence at which the hold lifts, or None if not yet funded.
    pub held_until_ledger: Option<u32>,
    /// The current ledger sequence at query time.
    pub current_ledger: u32,
    /// True when held_until is set and current_ledger < held_until.
    pub hold_active: bool,
    /// Ledgers remaining until the hold lifts; 0 if already elapsed or no hold.
    pub ledgers_remaining: u32,
}
```

---

## Proposed entry points

| Function | Access | Description |
|---|---|---|
| `get_invoice_hold_status(invoice_id)` | anyone | Return `InvoiceHoldStatus`. Read-only. |
| `set_invoice_hold_period(creator, invoice_id, period)` | creator | Update `escrow_hold_period`. Requires `Pending` status. |

`release` is modified (not a new entry point) to enforce the hold check and
emit `hold_end`.

---

## Storage

No new storage keys. `escrow_hold_period` and `held_until` are already fields
in `InvoiceCore`.

---

## Relationship to other features

- **Release delay (`release_delay_ledgers`):** a separate field that counts
  from full funding, similar to `escrow_hold_period`. The two are independent
  guards; both must pass before `_release` transfers funds. See the release
  guards list in `ARCHITECTURE.md` §4.3.
- **Tranches:** tranche unlock timestamps are per-stage, not per-invoice.
  `held_until` applies globally to the invoice and stacks on top of tranche
  timing.
- **Disputes:** a payer who wants to dispute during the hold period uses
  `raise_dispute` normally. The hold period is not a dispute mechanism; it is
  a mandatory cooling-off window.

---

## Open questions

- Whether `held_until` should be stored in wall-clock seconds (using
  `env.ledger().timestamp()`) rather than ledger sequence numbers.
  Ledger sequences are more predictable on Soroban but harder for end users to
  reason about. The existing code uses ledger sequence; keep it consistent
  unless there is a specific reason to switch.
- Whether an admin override (`admin_release_held_invoice`) should be allowed
  to bypass the hold for exceptional circumstances. If added, it must require
  SuperAdmin auth and emit an auditable event.

---

## Not done

The `held_until` check in `_release`, the `HoldPeriodNotElapsed` error code,
`get_invoice_hold_status`, `set_invoice_hold_period`, the `hold_end` event,
and clearing `held_until` on release are all to be implemented.
