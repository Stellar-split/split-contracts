# Issue #979: Reversible operations (design scaffold)

**Status:** design only. No reversal is implemented.

## Problem

Some operations cannot be undone once they run: a release sends funds, and a cancellation closes an
invoice. A mistake in either has no recovery path inside the contract.

## Proposed approach

1. A reversal window per operation. A release can be reversed for a short, fixed time, with the funds
   held back until the window closes.
2. Reversal is an explicit call by the admin or the creator, naming the operation it reverses. It is
   not a general undo.
3. Each reversal writes a record that links to the original operation, so the audit trail shows both.

## Constraints

- A reversal cannot move funds that have already left the contract. The window has to be applied
  before the transfer, which means holding funds, not recalling them.
- Holding funds for a window changes the release path, so it needs the same care as the
  refund-grace logic already in `invoice-escrow`.

## Open questions

- Which operations are reversible. A cancellation may be reversible, a payout to a recipient probably
  not.
- Who may reverse: creator, admin, or both.

## Not done

No code, no tests, no window logic.
