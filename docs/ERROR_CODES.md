# Error code reference (invoice escrow)

Every error the `invoice-escrow` contract returns, with its code, cause, and what a caller can do
about it. The codes are in `contracts/invoice-escrow/src/errors.rs`. Their numbers are stable, so
never reuse one.

## Admin and setup

| Code | Name | Cause | Recovery |
|---|---|---|---|
| 1 | `NotAdmin` | Caller is not the admin | Call with the admin address |
| 2 | `NoPendingAdmin` | Accept or cancel with no pending transfer | Start a transfer first |
| 3 | `Unauthorized` | Caller is not the pending admin | Accept from the pending admin's account |
| 4 | `AlreadyInitialized` | `initialize` called twice | Nothing; the contract is set up |
| 5 | `NotInitialized` | Call before `initialize` | Call `initialize` first |

## Invoices and funding

| Code | Name | Cause | Recovery |
|---|---|---|---|
| 6 | `InvoiceNotFound` | Unknown invoice ID | Check the ID |
| 7 | `DeadlinePassed` | Deposit after the deadline | Create a new invoice |
| 8 | `InvalidStatus` | Operation not allowed in this state | Read the status, then act on the right state |
| 9 | `InvalidAmount` | Amount is zero or negative | Send a positive amount |
| 10 | `InvalidTotalAmount` | Invoice total is zero or negative | Create with a positive total |
| 11 | `OverFunded` | Deposit would exceed the total | Deposit the remaining amount only |
| 12 | `InsufficientFunding` | Release before full funding | Wait for the rest of the deposits |
| 13 | `DeadlineNotPassed` | Refund before the deadline | Wait until the deadline |
| 14 | `InvalidRecipients` | Recipient and amount lists differ or are empty | Fix the lists |

## Blacklist and appeals

| Code | Name | Cause | Recovery |
|---|---|---|---|
| 15 | `PayerBlacklisted` | A blacklisted payer tried to act | Submit an appeal |
| 16 | `AppealWindowExpired` | Appeal after the window | None on this entry |
| 17 | `NotBlacklisted` | Appeal or finalise for a payer not on the list | Nothing to do |
| 18 | `AlreadyFinalised` | Entry already finalised | Nothing to do |
| 19 | `NotBlacklistedPayer` | Caller is not the blacklisted payer | Use the payer's account |

## Co-signers

| Code | Name | Cause | Recovery |
|---|---|---|---|
| 20 | `InvalidCosignerConfig` | Empty, duplicate, too many, or bad threshold | Fix the config |
| 21 | `NotCosigner` | Caller is not a configured co-signer | Use a co-signer's account |
| 22 | `AlreadyVoted` | Co-signer voted twice | Nothing to do |
| 23 | `ApprovalRequired` | Release before the threshold | Collect more approvals |
| 24 | `ApprovalRejected` | Too many rejections to reach the threshold | Cancel or refund instead |
| 25 | `NotVoted` | Revoke with no vote | Nothing to revoke |
| 26 | `CosignersLocked` | Change co-signers after voting began | Create a new invoice |
| 27 | `NoCosigners` | Approval requested with no co-signers | Configure co-signers first |

## Oracle conditions

| Code | Name | Cause | Recovery |
|---|---|---|---|
| 28 | `InvalidOracleConfig` | Non-positive threshold, zero staleness, or fallback equal to primary | Fix the config |
| 29 | `OracleUnavailable` | No oracle returned a price | Retry later |
| 30 | `OracleStalePrice` | Price older than the staleness window | Retry when the oracle updates |
| 31 | `OracleInvalidPrice` | Non-positive price or future timestamp | Report to the oracle operator |
| 32 | `OracleConditionNotMet` | Price does not satisfy the condition | Wait for the price to move |
| 33 | `NoOracleCondition` | No condition set | Set one first |

## Schedules and refund grace

| Code | Name | Cause | Recovery |
|---|---|---|---|
| 34 | `InvalidSchedule` | Release time not in the future, or ledger delay too large | Set a valid time |
| 35 | `ReleaseNotYetDue` | Release time not reached | Wait |
| 36 | `ScheduleLedgerNotReached` | Minimum ledger count not reached | Wait for more ledgers |
| 37 | `ScheduleNotFound` | No active schedule | Create one first |
| 38 | `RefundGraceActive` | Refund during the grace period | Wait for the grace period to end |
| 39 | `InvalidGracePeriod` | Grace period above the maximum | Use a shorter period |
| 40 | `RefundGraceElapsed` | Grace change after it ended | Nothing to change |

## Deadlines and arbitration

| Code | Name | Cause | Recovery |
|---|---|---|---|
| 41 | `DeadlineOutOfRange` | Deadline more than `MAX_DEADLINE_HORIZON` away | Use a closer deadline |
| 42 | `NoArbiter` | Dispute call with no arbiter configured | Admin calls `set_arbiter` first |
| 43 | `NotArbiter` | Caller is not the configured arbiter | Use the arbiter's account |
| 44 | `DisputeNotEligible` | `raise_dispute` before approval status is `Rejected` | Wait for co-signers to reject, or resolve normally |
| 45 | `DisputeAlreadyOpen` | `raise_dispute` called twice | Wait for the arbiter to decide |
| 46 | `NoDispute` | `arbitrate_dispute` with nothing raised | Call `raise_dispute` first |
| 47 | `DisputeAlreadyResolved` | Arbiter decides the same dispute twice | Nothing to do; the decision already stands |

## Scope

This page covers `invoice-escrow` only. The `split` contract has its own error type in
`contracts/split/src/error.rs`, which is documented separately.
