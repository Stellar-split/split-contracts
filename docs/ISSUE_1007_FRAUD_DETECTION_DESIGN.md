# Issue #1007: Fraud detection system (design scaffold)

**Status:** design only. No fraud detection is added. Two modules already cover part of the space.

## Relevant existing code

- `velocity.rs` (issue #912) limits how many payments one payer can make in a window. That is one
  fraud signal, and it works on counts, not patterns.
- `security_audit.rs` (issue #892) runs audits on demand. It does not flag transactions as they happen.

## Goal

Flag transactions that look fraudulent, so they can be held for review, without blocking honest payers
more than necessary.

## Proposed approach

1. Signals the contract can see on-chain: velocity, amounts relative to the invoice total, a payer who
   funds and then cancels repeatedly, and an address that appears in many invoices at once.
2. Each signal adds to a score for the transaction. A score above a threshold holds the transaction for
   review rather than refusing it.
3. The review decision is recorded by an operator, so a held transaction always has a recorded outcome.

## Constraints

- The contract sees only on-chain data, so detection is limited to patterns in that data. It cannot judge
  intent.
- Holding a transaction is a state change that needs its own release path, which does not exist yet.

## Open questions

- The thresholds, and who sets them. Set by admin, or fixed in code.
- How a false positive is appealed. The blacklist appeal in `invoice-escrow` is one model.

## Not done

No code, no tests, no scoring, no hold state.
