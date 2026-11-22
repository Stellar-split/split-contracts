# Issue #971: Confidential payment tracking (status)

**Status:** the design is in [`ISSUE_295_CONFIDENTIAL_PAYMENTS.md`](./ISSUE_295_CONFIDENTIAL_PAYMENTS.md).
A module exists at `contracts/split/src/confidential_tracking.rs`. This note records what is known
about it. It is not a full review.

## What the design covers

- Blinded amounts with Pedersen commitments. Amounts are hidden and revealed only to the invoice creator.
- An entry point to submit a confidential payment with a commitment, a range proof and an encrypted
  amount.

## What this change does

Adds this status note. It changes no code and adds no tests.

## What still needs checking

- Whether every entry point in the design has a counterpart in `confidential_tracking.rs`.
- Whether the range proof is verified on-chain, or only recorded. The design text is the reference.
- Whether tests cover reveal by the creator and refusal for anyone else.

## Not done

No review of the module's behaviour, no new tests, no change to the design.
