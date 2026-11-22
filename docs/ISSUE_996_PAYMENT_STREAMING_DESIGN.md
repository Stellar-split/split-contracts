# Issue #996: Payment streaming (design scaffold)

**Status:** design only. No streaming is implemented.

## Goal

Pay a recipient continuously, in proportion to time elapsed, instead of in one release, with
settlement that a recipient can claim at any time.

## Proposed model

- A stream has a rate (stroops per second), a start time and an end time, and a recipient.
- Accrued amount at time `t` is `rate * (min(t, end) - start)`, minus what has been claimed.
- A recipient claims by naming the amount; the contract checks it does not exceed the accrued amount.

## Constraints

- Time comes from the ledger timestamp, which advances in jumps, so accrual is an approximation at
  the ledger resolution. Document that accrual is quantised to ledgers.
- Accrual is computed on claim, not stored each second. Storing per-second state is not possible on
  Soroban.
- A claim is one transfer, so the stream must not allow more than the escrowed balance to be claimed.

## Open questions

- Whether a stream can be cancelled by the payer, and how the accrued part is settled when it is.
- How a stream sits beside the existing single release and schedule logic in `invoice-escrow`.

## Not done

No code, no tests, no accrual function.
