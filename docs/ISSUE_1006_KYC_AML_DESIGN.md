# Issue #1006: KYC and AML compliance checks (design scaffold)

**Status:** design only. No compliance check is added.

## Goal

Refuse payments from, or payouts to, addresses that have not passed a KYC check or that appear on an
AML list, and record that the check was made.

## Constraints

- A contract cannot look up identity data, and it cannot call an outside service. Any KYC result has to be
  written on-chain by an authorised party, so the contract only checks a stored result.
- Storing personal data on-chain is not an option. Only an opaque status per address (for example
  "approved" or "blocked") and a reference to the off-chain record can be kept.

## Proposed approach

1. A `ComplianceStatus` per address, set by a compliance operator, with the operator's address and a
   timestamp.
2. The payment path checks the payer's status, and the payout path checks each recipient's status, before
   any transfer.
3. Status changes emit an event, so the off-chain compliance system can keep its own record in step.

## Relevant existing code

The existing blacklist (`blacklist_payer`, `finalise_blacklist`, and the appeal path in `invoice-escrow`)
is a similar pattern for payers. It is a candidate to build on, but it blocks a payer for misconduct,
not for failing a check, so the two should stay separate.

## Open questions

- Who may act as a compliance operator, and how that role is granted and revoked.
- What happens to funds already held for an address that becomes blocked.

## Not done

No code, no tests, no status key.
