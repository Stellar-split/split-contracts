# Issue #1005: Transaction verification system (design scaffold)

**Status:** design only. No verification step is added.

## Goal

Check a transaction against the invoice's rules before it changes state, and record the result, so a
bad transaction is caught on-chain rather than in review.

## Relevant existing checks

- Input validation in `contracts/split/src/validation.rs` covers the shape of each argument.
- The security audit framework in `security_audit.rs` (issue #892) runs checks on demand, rather than on
  each transaction.

The verification step would sit between these: it runs on each state change, and it returns a result the
caller can read.

## Proposed approach

1. A `verify` step that takes the operation, the invoice state, and the arguments, and returns `Ok` or a
   named failure. It reads state; it does not write.
2. Each state-changing entry point calls `verify` first, and the write happens only on `Ok`.
3. The result is a `VerificationResult` the caller can fetch for a transaction, so an integrator can see
   why a call was refused.

## Constraints

- Verification adds reads to every call. Budgets in `COMPUTE_BUDGETS.md` must be re-measured with it on.
- It must not duplicate checks `validation.rs` already makes. Each rule should live in one place.

## Open questions

- Whether verification results are stored, or only returned. Storing costs writes; returning does not.

## Not done

No code, no tests, no `verify` step.
