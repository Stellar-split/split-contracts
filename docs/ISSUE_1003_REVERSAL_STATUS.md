# Issue #1003: Payment reversal and refund (status)

**Status:** refund exists in `contracts/invoice-escrow/src/lib.rs`: `refund(invoice_id, payers)`
returns escrowed funds to the listed payers after the deadline, subject to the refund grace period.
Its errors are `DeadlineNotPassed` (13), `RefundGraceActive` (38), `RefundGraceElapsed` (40), and
`InvalidGracePeriod` (39), documented in [`ERROR_CODES.md`](./ERROR_CODES.md). A reversal of a
completed release is not implemented.

## What this change does

Records the status above. It changes no code and adds no tests.

## What still needs checking

- That `refund` returns each payer's own deposit, and no more, when payers have partly paid.
- Whether the issue's "reversal" means reversing a release. The refund path does not do that, and a
  release is final.
- Whether `waive_refund_grace` and `extend_refund_grace` are restricted to the admin.

## Not done

No review of the refund logic, no reversal of a release, no new tests.
