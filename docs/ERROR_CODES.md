# Contract error codes

Every `ContractError` variant from `contracts/split/src/error.rs`, with its stable
numeric discriminant. A failed invocation surfaces these as `Error(Contract, #code)`.

`contracts/split/src/test.rs` (`test_789_error_codes_doc_matches_enum`) fails if this
table and the enum drift apart, so update both together. Discriminants are stable:
never reorder or reuse a code, only append.

## Why there are no diagnostic events

Soroban rolls back every contract event published by an invocation that fails, so
an event emitted just before a panic is never observable on-chain or in tests. The
error code above (plus the panic message in the host's diagnostic log when running
with diagnostics enabled) is the structured context a caller actually receives.

| Code | Name | Meaning |
|-----:|------|---------|
| 1 | `NotAuthorized` | Caller is not authorized for this operation (missing admin/role/signer or wrong identity). |
| 2 | `InvoiceNotFound` | The referenced invoice id does not exist in storage. |
| 3 | `DeadlinePassed` | The operation requires the invoice to still be open, but its deadline has already passed. |
| 4 | `AlreadyFunded` | The invoice has already been fully funded and a further fund would over-fund it. |
| 5 | `InvalidAmount` | A supplied amount is non-positive, would overflow, or fails the invoice's amount rules. |
| 6 | `InvoiceFrozen` | The invoice (or a related entity) is frozen and the requested mutation is disallowed. |
| 7 | `InvalidStatus` | The invoice is not in a status that permits the requested operation. |
| 8 | `PayerNotAllowed` | The caller is not in the invoice's allowed-payer set (or `allowed_payers` rejects them). |
| 9 | `FundingInsufficient` | The payment/fund amount is less than the remaining amount required to fund the invoice. |
| 10 | `OracleCallFailed` | A call to an external oracle/price/dependency contract returned an error or aborted. |
| 11 | `NotArbiter` | The caller is not the arbiter required to perform this dispute-related action. |
| 12 | `NotDisputed` | The operation is only valid while the invoice is under dispute, but no dispute is active. |
| 13 | `AlreadyExecuted` | The action has already been executed and is not idempotent/retryable. |
| 14 | `TimelockPending` | A time-lock / vesting window has not yet elapsed; the operation must wait. |
| 15 | `ContractPaused` | The contract is globally paused; all mutating entry points are blocked. |
| 16 | `InvalidRecipients` | The recipient list is invalid (e.g. mismatched lengths of recipients/amounts/tokens). |
| 17 | `PrerequisiteNotMet` | A prerequisite (invoice, milestone, or dependency) that must be satisfied first is not met. |
| 18 | `BatchLimitExceeded` | The requested batch exceeds the maximum number of items permitted in a single call. |
| 19 | `RecipientAlreadyPaid` | Issue #330: Recipient has already been paid on this invoice. |
| 20 | `FundsLockedUntil` | Issue #327: Funds are still time-locked and cannot be released yet. |
| 21 | `StatsOverflow` | Aggregate protocol statistics would exceed their numeric bounds. |
| 22 | `OracleUnavailable` | Oracle-priced invoice: the configured price oracle is unreachable or returned a non-positive rate at payment time. |
| 23 | `InvalidRating` | Caller supplied a rating outside the valid range (e.g. 0, negative, or above the max score). |
| 24 | `AlreadyRated` | Caller attempted to rate the same target more than once. |
| 25 | `RateLimitExceeded` | Caller exceeded the per-window rate limit for this operation. |
| 26 | `RecipientRevealMismatch` | Issue #438: Recipient reveal commitment does not match stored hash. |
| 27 | `PayoutNotYetClaimable` | Issue #437: Delayed payout is not yet claimable (before claimable_at_ledger). |
| 28 | `ContractFrozen` | Issue #435: Contract is frozen for upgrade; write operations are blocked. |
| 29 | `DuplicatePayment` | Issue #431: Duplicate payment detected within the duplicate window. |
| 30 | `GroupMemberExpired` | Issue #434: Invoice group member expired unfunded; group rollback triggered. |
| 31 | `SlippageExceeded` | Issue #448: token balance deviated beyond slippage tolerance. |
| 32 | `InvalidPhaseTransition` | Issue #449: invalid phase transition. |
| 33 | `MemoMismatch` | Issue #451: payer-provided memo does not match the required memo hash. |
| 34 | `CreatorCooldownActive` | Issue #439: Creator is in cooldown after cancelling an invoice. |
| 35 | `InvoiceFullyFunded` | Issue #420: Payment rejected because the invoice's `Cap` overfunding policy does not allow `funded` to exceed the invoice total. |
| 36 | `InvalidRatioSum` | The provided ratios do not sum to exactly BASIS_POINTS_TOTAL (10 000). |
| 37 | `EmptyRecipientList` | The recipient/ratio list is empty; at least one entry is required. |
| 38 | `ReentrantCall` | Reentrant call detected: a fund-moving function was invoked recursively within the same transaction. Cleared automatically at transaction boundary because the lock lives in temporary storage. |
| 39 | `RoleNotHeld` | RBAC: Caller does not hold the required role for this entry point. |
| 40 | `ArithmeticOverflow` | Issue #482: Intermediate multiplication or division overflowed i128 bounds. |
| 41 | `ZeroAmountNotAllowed` | Issue #483: A zero-value or negative amount was passed where a positive amount is required. |
| 42 | `ContributorNotAllowed` | Issue #485: Caller is not on the invoice contributor allowlist. |
| 43 | `UnauthorisedToken` | Issue #473: Token is not in the allowed tokens list. |
| 44 | `DependencyNotMet` | Issue #456: Invoice dependency chain - predecessor invoice is not yet Released. |
| 45 | `CircularDependency` | Issue #456: Circular dependency detected in invoice dependency chain. |
| 46 | `SourceContractRateLimited` | Issue #453: Source contract has exceeded its call rate limit within the current window. |
| 47 | `FeeSumExceedsCap` | Issue #559: creator fee bps + platform fee bps exceeds cap of 10000. |
| 48 | `InvoiceDeleted` | Issue #562: operation not allowed on a soft-deleted invoice. |
| 49 | `FundsUnclaimed` | Issue #562: invoice has unclaimed funds and cannot be soft-deleted. |
| 50 | `NotDeleted` | Issue #562: attempted to read tombstone for an invoice that is not deleted. |
| 51 | `RecipientMissingTrustline` | Issue #558: A recipient address has not established a trustline for the payment token. The offending address is surfaced in the panic message. |
| 52 | `InvalidStateTransition` | Issue #519: An invoice status transition is not permitted by the state machine. |
| 53 | `InvalidRatio` | Issue #518: A split ratio is invalid (e.g. >= denominator or sum mismatch). |
| 54 | `MigrationRequired` | Storage migration framework: `schema_version` is behind the version this Wasm build expects. Call `migrate` before retrying. |
| 55 | `DuplicateRecipient` | Issue #556: The recipient list supplied at invoice creation contains a duplicate address. Rejected before any storage is written. |
| 56 | `SplitRatioLocked` | Issue #557: Split ratios are locked after the first contribution is recorded and may no longer be mutated. |
| 57 | `CreatorInvoiceLimitReached` | Issue #503: Creator has reached their open invoice limit. |
| 58 | `CoCreatorLimitReached` | Issue #503: Co-creator count would exceed the maximum allowed. |
| 59 | `InvoiceDisputed` | Invoice is under active dispute and cannot be released. |
| 60 | `PayerSpendLimitExceeded` | Issue #503: Payer spend limit would be exceeded by this payment. |
| 61 | `RecipientAccountMissing` | Issue #505: Recipient account does not exist on-ledger. |
| 62 | `RecipientNotFound` | Recipient not found in the invoice recipient list. |
| 63 | `ParentChainTooDeep` | Issue #522: Parent chain depth exceeds the allowed maximum. |
| 64 | `CheckpointMismatch` | Issue #564: Checkpoint index does not match stored value during payout recovery. |
| 65 | `AlreadyPaid` | Issue #564: Recipient at this index has already been paid in a prior payout attempt. |
| 66 | `TooFewRecipients` | Recipient list is shorter than the configured minimum recipient count. |
