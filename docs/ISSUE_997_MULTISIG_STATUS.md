# Issue #997: Multi-signature payments (status)

**Status:** co-signer approval is implemented in `contracts/invoice-escrow/src/lib.rs`:
`set_cosigners`, `approve_release`, `reject_release`, `revoke_vote`, `get_approval`, and
`approvals_remaining`. Its errors are codes 20 to 27 in [`ERROR_CODES.md`](./ERROR_CODES.md).

## What this change does

Records the status above. It changes no code and adds no tests.

## What still needs checking

- Whether the issue's "multi-signature payment" means release approval (covered) or a payment that
  needs several payers to sign (not covered by this status).
- That `set_cosigners` cannot change the threshold after voting has started (`CosignersLocked`).
- That a rejection that makes the threshold unreachable ends the release, as `ApprovalRejected` says.

## Not done

No review of the approval logic, no new tests.
