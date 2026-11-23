# Issue #1101: Payment failure tracking (status)

**Status:** partly covered. Failed payouts to recipients are tracked, with a reason and a retry. Failed
payment attempts by payers cannot be tracked on chain.

## Why payer failures are not stored

A `pay` call that fails reverts the transaction, so nothing it would write survives. A contract cannot
keep a record of a payment that was rejected by the same transaction. Failed payer attempts are visible
in the transaction results that wallets, RPC providers and indexers keep. The error code in each result
is explained in [`ERROR_CODES_SPLIT.md`](./ERROR_CODES_SPLIT.md).

## What is tracked: failed payouts

When a transfer to a recipient fails during release, the contract catches it, keeps going with the other
recipients, and records the failure:

| Requirement | Where it is in `contracts/split/src/lib.rs` |
|---|---|
| Failure tracking | `record_failed_payout(env, invoice_id, recipient, amount, reason)` stores the owed amount under `failed_payout_record_key` and adds the recipient to `failed_payouts_key`. `get_failed_payouts(invoice_id)` returns the recipient addresses. |
| Reason codes | The reason is a string. Every current call site passes `"TransferFailed"`. It is published in the `PayoutFailed` event (topics `(split, PayoutFailed, invoice_id)`, data `(recipient, amount, reason)`). Retrying a recipient whose account is missing fails with `ContractError::RecipientAccountMissing` (61). |
| Retry logic | `retry_failed_payout(invoice_id, recipient)`, `claim_pending_payout` and `claim_fallback` (which also clears recorded failures) |
| Failure analysis | A watcher can read `PayoutFailed` events, and the audit log gets a `pay_fail` entry. |
| Failure stats | Not present. There is no counter of failed payouts and no per-reason breakdown. |

## Gaps

- The amount owed is stored but there is no public getter for it, so a client reads the invoice's
  recipient amounts to work it out.
- There is a single reason string, so analysis by reason needs more than one value.

## What this change does

Records the status above. It changes no code. The behaviour was read, not run, and no tests were added.
