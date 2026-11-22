# Issue #980: Webhook notification system (status)

**Status:** the design is in [`ISSUE_930_WEBHOOKS.md`](./ISSUE_930_WEBHOOKS.md), and the module is
`contracts/split/src/webhooks.rs`. It provides a subscription registry, a delivery ledger, and a
backoff function (`backoff_ledgers`). This note records the status. It is not a review.

## What this change does

Records the status above. It changes no code and adds no tests.

## What still needs checking

- That every lifecycle event named in the issue has a delivery record: creation, funding, release,
  refund, and cancellation.
- That `backoff_ledgers` grows with each failed attempt and is capped.
- That delivery is done off-chain by a relayer, since a contract cannot make HTTP calls. The relayer
  is not part of this repo.

## Not done

No review of the webhook logic, no new tests.
