# Issue #978: Recipient authentication (design scaffold)

**Status:** design only. No recipient check is added.

## Problem

A payout goes to an address the invoice lists as a recipient. The contract currently checks that the
recipient list is well formed (see `InvalidRecipients` in [`ERROR_CODES.md`](./ERROR_CODES.md)), but a
recipient cannot prove it controls the address it is given.

## Proposed approach

1. A recipient confirms the invoice by signing a challenge that names the invoice ID, the recipient
   address, and a nonce. The contract checks the signature with `require_auth` on that address.
2. Payouts to a recipient are held until that recipient has confirmed, or until an admin override.
3. A confirmation is stored under a `RecipientAuth` key family (see
   [`STORAGE_KEY_CONVENTIONS.md`](./STORAGE_KEY_CONVENTIONS.md)), so it is a separate record from the
   invoice itself.

## Open questions

- Soroban authorises an address by `require_auth`, and the recipient must sign a transaction to do so.
  Whether a recipient has to submit a transaction, or whether a signed off-chain message is accepted, is
  not decided.
- What happens to funds held for a recipient who never confirms: refund to the payers, or an admin
  decision.

## Not done

No code, no tests, no key family.
