# Issue #1001: Payment holds and reserves (design scaffold)

**Status:** design only. No hold or reserve mechanism is implemented.

## Goal

Hold a part of a payment back for a set time, or keep a reserve against future claims, so the funds
are not released in full at once.

## Proposed model

- A hold: an amount, a release time, and the recipient it applies to. Funds stay in escrow until the
  release time, then become claimable.
- A reserve: a percentage of each payment kept back as a pool, which the admin can draw on for a
  documented reason, and which is released when the reserve is closed.

## Constraints

- The escrow already tracks balances per invoice. A hold is a sub-balance, so the sum of holds and
  free funds must always equal the escrow balance. That is the invariant to test first.
- A reserve drawn by the admin is a transfer. It needs the same authorisation rules as any admin
  transfer.

## Open questions

- Whether holds apply per recipient or per invoice.
- Whether a reserve percentage can change after payments have been made.

## Not done

No code, no tests, no sub-balance.
