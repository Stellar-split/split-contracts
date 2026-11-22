# Issue #972: Fee waiver system (status)

**Status:** the design is in [`ISSUE_296_FEE_WAIVER.md`](./ISSUE_296_FEE_WAIVER.md), and
[`ISSUE_922_FEE_WAIVER_SYSTEM.md`](./ISSUE_922_FEE_WAIVER_SYSTEM.md) covers a later revision. This note
records what is known. It is not a full review.

## What the design covers

- An admin-managed list of creators who are exempt from platform fees (for example non-profits and
  launch partners).
- The fee calculation skips the platform fee for a creator on the list.

## What this change does

Adds this status note. It changes no code and adds no tests.

## What still needs checking

- Whether the waiver check runs in every path that charges a fee, including batches.
- Whether adding and removing a creator emits events, as the later design asks.
- The fee formula the waiver bypasses is in [`FEE_FLOW.md`](./FEE_FLOW.md). Confirm the waiver path
  returns 0 and not an error.

## Not done

No review of the implementation, no new tests.
