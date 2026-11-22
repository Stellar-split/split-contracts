# Error code reference (split contract)

Every error the `split` contract can return, with its code, cause, and what a caller can do about it.
The codes are the `ContractError` enum in `contracts/split/src/error.rs`. Their numbers are stable: never
reorder or reuse one, only append. The recovery text is `error::recovery::suggestion_for`.

The `invoice-escrow` contract has its own codes. They are in [`ERROR_CODES.md`](./ERROR_CODES.md).

There are 82 codes. Code 51 is declared after code 56 in the source, but is a normal code.

| Code | Name | Cause | Recovery |
|---|---|---|---|
| 1 | `NotAuthorized` | Caller is not authorized for this operation (missing admin/role/signer or wrong identity). | Ensure the caller is the admin, owner, or holds the required role/signer, then retry. |
| 2 | `InvoiceNotFound` | The referenced invoice id does not exist in storage. | Verify the invoice id exists (it may have been deleted); create the invoice first. |
| 3 | `DeadlinePassed` | The operation requires the invoice to still be open, but its deadline has already passed. | The funding deadline has passed; ask the creator to extend the deadline or open a new invoice. |
| 4 | `AlreadyFunded` | The invoice has already been fully funded and a further fund would over-fund it. | The invoice is already fully funded; no further funding is accepted. |
| 5 | `InvalidAmount` | A supplied amount is non-positive, would overflow, or fails the invoice's amount rules. | Supply a positive amount that fits within the invoice's remaining capacity and i128 bounds. |
| 6 | `InvoiceFrozen` | The invoice (or a related entity) is frozen and the requested mutation is disallowed. | The invoice is frozen; wait for an admin to unfreeze it before retrying. |
| 7 | `InvalidStatus` | The invoice is not in a status that permits the requested operation. | Check the invoice status; perform the required prior transition (e.g. fund before release). |
| 8 | `PayerNotAllowed` | The caller is not in the invoice's allowed-payer set (or `allowed_payers` rejects them). | Use an address on the invoice's allowed-payer list, or ask the creator to add it. |
| 9 | `FundingInsufficient` | The payment/fund amount is less than the remaining amount required to fund the invoice. | Increase the payment to at least the remaining amount required to fully fund the invoice. |
| 10 | `OracleCallFailed` | A call to an external oracle/price/dependency contract returned an error or aborted. | The oracle call failed; retry later or ask the admin to verify the oracle configuration. |
| 11 | `NotArbiter` | The caller is not the arbiter required to perform this dispute-related action. | Only the designated arbiter may perform this action; call from the arbiter address. |
| 12 | `NotDisputed` | The operation is only valid while the invoice is under dispute, but no dispute is active. | Open a dispute first; this action is only valid while a dispute is active. |
| 13 | `AlreadyExecuted` | The action has already been executed and is not idempotent/retryable. | This action already completed and is not idempotent; do not retry it. |
| 14 | `TimelockPending` | A time-lock / vesting window has not yet elapsed; the operation must wait. | Wait until the time-lock/vesting window elapses, then retry. |
| 15 | `ContractPaused` | The contract is globally paused; all mutating entry points are blocked. | The contract is paused; wait for an admin to unpause it before retrying. |
| 16 | `InvalidRecipients` | The recipient list is invalid (e.g. mismatched lengths of recipients/amounts/tokens). | Ensure recipients, amounts, and tokens lists have matching, non-empty lengths. |
| 17 | `PrerequisiteNotMet` | A prerequisite (invoice, milestone, or dependency) that must be satisfied first is not met. | Satisfy the prerequisite (invoice, milestone, or dependency) before retrying. |
| 18 | `BatchLimitExceeded` | The requested batch exceeds the maximum number of items permitted in a single call. | Split the batch into smaller chunks within the per-call item limit. |
| 19 | `RecipientAlreadyPaid` | Issue #330: Recipient has already been paid on this invoice. | This recipient was already paid; skip them or use the payout-recovery path. |
| 20 | `FundsLockedUntil` | Issue #327: Funds are still time-locked and cannot be released yet. | Funds are still time-locked; wait until the lock expires, then release. |
| 21 | `StatsOverflow` | Aggregate protocol statistics would exceed their numeric bounds. | Aggregate statistics would overflow; retry with a smaller amount or contact the admin. |
| 22 | `OracleUnavailable` | Oracle-priced invoice: the configured price oracle is unreachable or returned a non-positive rate at payment time. | The price oracle is unreachable or returned an invalid rate; retry later. |
| 23 | `InvalidRating` | Caller supplied a rating outside the valid range (e.g. 0, negative, or above the max score). | Supply a rating within the allowed range (1..=max score). |
| 24 | `AlreadyRated` | Caller attempted to rate the same target more than once. | This target has already been rated by the caller; ratings are one-per-target. |
| 25 | `RateLimitExceeded` | Caller exceeded the per-window rate limit for this operation. | Wait for the current rate-limit window to reset before retrying. |
| 26 | `RecipientRevealMismatch` | Issue #438: Recipient reveal commitment does not match stored hash. | Verify the reveal preimage matches the stored commitment hash, then retry. |
| 27 | `PayoutNotYetClaimable` | Issue #437: Delayed payout is not yet claimable (before claimable_at_ledger). | Wait until the claimable ledger is reached, then claim the payout. |
| 28 | `ContractFrozen` | Issue #435: Contract is frozen for upgrade; write operations are blocked. | The contract is frozen for upgrade; wait for the upgrade to complete. |
| 29 | `DuplicatePayment` | Issue #431: Duplicate payment detected within the duplicate window. | A matching payment was already recorded within the duplicate window; do not resubmit. |
| 30 | `GroupMemberExpired` | Issue #434: Invoice group member expired unfunded; group rollback triggered. | A group member expired unfunded and the group rolled back; re-create the group. |
| 31 | `SlippageExceeded` | Issue #448: token balance deviated beyond slippage tolerance. | Price moved beyond tolerance; retry with an updated quote or wider slippage tolerance. |
| 32 | `InvalidPhaseTransition` | Issue #449: invalid phase transition. | Check the current phase; only permitted phase transitions are allowed. |
| 33 | `MemoMismatch` | Issue #451: payer-provided memo does not match the required memo hash. | Ensure the supplied memo hashes to the required memo hash. |
| 34 | `CreatorCooldownActive` | Issue #439: Creator is in cooldown after cancelling an invoice. | The creator is in cooldown after a cancellation; wait for the cooldown to elapse. |
| 35 | `InvoiceFullyFunded` | Issue #420: Payment rejected because the invoice's `Cap` overfunding policy does not allow `funded` to exceed the invoice total. | The invoice is fully funded under its Cap policy; no further funding is accepted. |
| 36 | `InvalidRatioSum` | The provided ratios do not sum to exactly BASIS_POINTS_TOTAL (10 000). | Adjust split ratios so they sum to exactly 10 000 basis points. |
| 37 | `EmptyRecipientList` | The recipient/ratio list is empty; at least one entry is required. | Provide at least one recipient/ratio entry. |
| 38 | `ReentrantCall` | Reentrant call detected: a fund-moving function was invoked recursively within the same transaction. Cleared automatically at transaction boundary because the lock lives in temporary storage. | A reentrant call was detected; retry the operation in a fresh transaction. |
| 39 | `RoleNotHeld` | RBAC: Caller does not hold the required role for this entry point. | Grant the caller the required RBAC role, then retry. |
| 40 | `ArithmeticOverflow` | Issue #482: Intermediate multiplication or division overflowed i128 bounds. | Reduce the amount so intermediate arithmetic stays within i128 bounds. |
| 41 | `ZeroAmountNotAllowed` | Issue #483: A zero-value or negative amount was passed where a positive amount is required. | Supply a strictly positive amount. |
| 42 | `ContributorNotAllowed` | Issue #485: Caller is not on the invoice contributor allowlist. | Use an address on the invoice contributor allowlist, or ask the creator to add it. |
| 43 | `UnauthorisedToken` | Issue #473: Token is not in the allowed tokens list. | Use a token from the allowed tokens list, or ask the admin to allow it. |
| 44 | `DependencyNotMet` | Issue #456: Invoice dependency chain - predecessor invoice is not yet Released. | Wait for the predecessor invoice to reach Released before retrying. |
| 45 | `CircularDependency` | Issue #456: Circular dependency detected in invoice dependency chain. | Remove the circular reference from the invoice dependency chain. |
| 46 | `SourceContractRateLimited` | Issue #453: Source contract has exceeded its call rate limit within the current window. | The source contract exceeded its call rate limit; wait for the window to reset. |
| 47 | `FeeSumExceedsCap` | Issue #559: creator fee bps + platform fee bps exceeds cap of 10000. | Reduce creator + platform fee bps so their sum is at most 10 000. |
| 48 | `InvoiceDeleted` | Issue #562: operation not allowed on a soft-deleted invoice. | The invoice is soft-deleted; restore it or operate on a different invoice. |
| 49 | `FundsUnclaimed` | Issue #562: invoice has unclaimed funds and cannot be soft-deleted. | Claim all outstanding funds before soft-deleting the invoice. |
| 50 | `NotDeleted` | Issue #562: attempted to read tombstone for an invoice that is not deleted. | The invoice is not deleted; no tombstone exists to read. |
| 51 | `RecipientMissingTrustline` | Issue #558: A recipient address has not established a trustline for the payment token. The offending address is surfaced in the panic message. | Ask the recipient to establish a trustline for the payment token, then retry. |
| 52 | `InvalidStateTransition` | Issue #519: An invoice status transition is not permitted by the state machine. | Check the invoice state machine; only permitted transitions are allowed. |
| 53 | `InvalidRatio` | Issue #518: A split ratio is invalid (e.g. >= denominator or sum mismatch). | Ensure each split ratio is below the denominator and the set is consistent. |
| 54 | `MigrationRequired` | Storage migration framework: `schema_version` is behind the version this Wasm build expects. Call `migrate` before retrying. | Call `migrate` to bring storage to the expected schema version, then retry. |
| 55 | `DuplicateRecipient` | Issue #556: The recipient list supplied at invoice creation contains a duplicate address. Rejected before any storage is written. | Remove duplicate recipient addresses from the list. |
| 56 | `SplitRatioLocked` | Issue #557: Split ratios are locked after the first contribution is recorded and may no longer be mutated. | Split ratios are locked after the first contribution and cannot be changed. |
| 57 | `CreatorInvoiceLimitReached` | Issue #503: Creator has reached their open invoice limit. | Close or cancel an existing invoice before creating a new one. |
| 58 | `CoCreatorLimitReached` | Issue #503: Co-creator count would exceed the maximum allowed. | Reduce the number of co-creators to within the allowed maximum. |
| 59 | `InvoiceDisputed` | Invoice is under active dispute and cannot be released. | Resolve the active dispute before attempting to release funds. |
| 60 | `PayerSpendLimitExceeded` | Issue #503: Payer spend limit would be exceeded by this payment. | Reduce the payment amount to stay within the payer's spend limit. |
| 61 | `RecipientAccountMissing` | Issue #505: Recipient account does not exist on-ledger. | Ask the recipient to create their account on-ledger, then retry. |
| 62 | `RecipientNotFound` | Recipient not found in the invoice recipient list. | Use an address present in the invoice recipient list. |
| 63 | `ParentChainTooDeep` | Issue #522: Parent chain depth exceeds the allowed maximum. | Shorten the parent chain to within the allowed maximum depth. |
| 64 | `CheckpointMismatch` | Issue #564: Checkpoint index does not match stored value during payout recovery. | Restart the payout from the last consistent checkpoint index. |
| 65 | `AlreadyPaid` | Issue #564: Recipient at this index has already been paid in a prior payout attempt. | This recipient was already paid; resume from the next unpaid index. |
| 66 | `TooFewRecipients` | Recipient list is shorter than the configured minimum recipient count. | Add more recipients to meet the configured minimum recipient count. |
| 67 | `InvalidSignature` | Issue #937: A co-signer signature failed cryptographic verification against the expected signer set (wrong signer, malformed payload, or bad signature bytes). | The provided signature is invalid; ensure the correct signer and signing key are used. |
| 68 | `MissingSignature` | Issue #937: A required co-signer signature was not supplied for the operation. | A required co-signer signature is missing; collect all required signatures and retry. |
| 69 | `SignatureReplayed` | Issue #937: The supplied signature (or its nonce/ledger window) has already been consumed; replaying it is rejected. | This signature has already been used; provide a fresh signature for this transaction. |
| 70 | `SignatureExpired` | Issue #937: The signature's validity window has expired (e.g. signed ledger bound is in the past) and it can no longer be accepted. | The signature has expired; generate a new signature with a future expiry time. |
| 71 | `InvalidOracleAddress` | Issue #938: The configured oracle address is malformed or not a valid contract address for the expected network. | The oracle address is invalid or not registered; verify the oracle configuration. |
| 72 | `OracleConditionNotMet` | Issue #938: The oracle's reported condition (e.g. price threshold or resolution outcome) was not satisfied, so the operation cannot proceed. | The oracle's reported condition was not satisfied; wait for the condition to be met and retry. |
| 73 | `OracleFeedNotFound` | Issue #938: No oracle feed is configured for the requested asset/pair. | No oracle feed is configured for the requested asset/pair; configure the oracle feed and retry. |
| 74 | `InvalidArgument` | Issue #951: A supplied argument failed a generic range/format validation check that has no more specific variant. | A supplied argument failed validation; check the argument format and range before retrying. |
| 75 | `MissingStorageEntry` | Issue #951: A required storage entry was absent when the operation needed it. | A required storage entry is missing; ensure the contract is properly initialized. |
| 76 | `UnsupportedOperation` | Issue #951: The requested operation is not supported by this contract version. | This operation is not supported by this contract version; upgrade the contract or use a supported operation. |
| 77 | `RecoveryIncomplete` | Issue #1011: Emergency pause recovery plan is not yet complete; unpause is blocked. | Complete all recovery plan steps before unpausing the contract. |
| 78 | `InvalidDeadline` | A supplied deadline value is invalid (past or far in future). | Provide a deadline that is in the future and not beyond 10 years from now. |
| 79 | `InvalidRecipientAddress` | A supplied recipient address is invalid or missing. | Provide a valid recipient address on the Stellar network. |
| 80 | `InvalidOracleCondition` | The oracle condition provided is invalid. | Ensure the oracle condition is properly formatted and valid. |
| 81 | `InvalidShardIndex` | An invalid shard index was provided. | Provide a shard index within the valid range for the contract. |
| 82 | `ShardCapacityExceeded` | The shard capacity has been exceeded. | Reduce the number of operations or increase the shard capacity. |

## Using the codes

- A contract call that fails with a `ContractError` surfaces as `Error(Contract, #<code>)` in the
  simulation result and in the transaction result. Match on the number, not the name.
- Several entry points still fail with a plain `panic!` or `assert!` message instead of a code
  (for example `"invoice not archived"` and `"batch limit out of range"`). Those have no code in the table.
- `error::recovery::get_recovery_suggestion` returns the recovery text as a `String`. It is a helper in
  the crate. No contract entry point exposes it, so clients read the text from this page.
