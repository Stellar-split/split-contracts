use soroban_sdk::contracterror;

/// Unified error taxonomy (issue #273). Discriminants are stable — never reorder, only append.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    /// Caller is not authorized for this operation (missing admin/role/signer or wrong identity).
    NotAuthorized = 1,
    /// The referenced invoice id does not exist in storage.
    InvoiceNotFound = 2,
    /// The operation requires the invoice to still be open, but its deadline has already passed.
    DeadlinePassed = 3,
    /// The invoice has already been fully funded and a further fund would over-fund it.
    AlreadyFunded = 4,
    /// A supplied amount is non-positive, would overflow, or fails the invoice's amount rules.
    InvalidAmount = 5,
    /// The invoice (or a related entity) is frozen and the requested mutation is disallowed.
    InvoiceFrozen = 6,
    /// The invoice is not in a status that permits the requested operation.
    InvalidStatus = 7,
    /// The caller is not in the invoice's allowed-payer set (or `allowed_payers` rejects them).
    PayerNotAllowed = 8,
    /// The payment/fund amount is less than the remaining amount required to fund the invoice.
    FundingInsufficient = 9,
    /// A call to an external oracle/price/dependency contract returned an error or aborted.
    OracleCallFailed = 10,
    /// The caller is not the arbiter required to perform this dispute-related action.
    NotArbiter = 11,
    /// The operation is only valid while the invoice is under dispute, but no dispute is active.
    NotDisputed = 12,
    /// The action has already been executed and is not idempotent/retryable.
    AlreadyExecuted = 13,
    /// A time-lock / vesting window has not yet elapsed; the operation must wait.
    TimelockPending = 14,
    /// The contract is globally paused; all mutating entry points are blocked.
    ContractPaused = 15,
    /// The recipient list is invalid (e.g. mismatched lengths of recipients/amounts/tokens).
    InvalidRecipients = 16,
    /// A prerequisite (invoice, milestone, or dependency) that must be satisfied first is not met.
    PrerequisiteNotMet = 17,
    /// The requested batch exceeds the maximum number of items permitted in a single call.
    BatchLimitExceeded = 18,
    /// Issue #330: Recipient has already been paid on this invoice.
    RecipientAlreadyPaid = 19,
    /// Issue #327: Funds are still time-locked and cannot be released yet.
    FundsLockedUntil = 20,
    /// Aggregate protocol statistics would exceed their numeric bounds.
    StatsOverflow = 21,
    /// Oracle-priced invoice: the configured price oracle is unreachable or returned a
    /// non-positive rate at payment time.
    OracleUnavailable = 22,
    /// Caller supplied a rating outside the valid range (e.g. 0, negative, or above the max score).
    InvalidRating = 23,
    /// Caller attempted to rate the same target more than once.
    AlreadyRated = 24,
    /// Caller exceeded the per-window rate limit for this operation.
    RateLimitExceeded = 25,
    /// Issue #438: Recipient reveal commitment does not match stored hash.
    RecipientRevealMismatch = 26,
    /// Issue #437: Delayed payout is not yet claimable (before claimable_at_ledger).
    PayoutNotYetClaimable = 27,
    /// Issue #435: Contract is frozen for upgrade; write operations are blocked.
    ContractFrozen = 28,
    /// Issue #431: Duplicate payment detected within the duplicate window.
    DuplicatePayment = 29,
    /// Issue #434: Invoice group member expired unfunded; group rollback triggered.
    GroupMemberExpired = 30,
    /// Issue #448: token balance deviated beyond slippage tolerance.
    SlippageExceeded = 31,
    /// Issue #449: invalid phase transition.
    InvalidPhaseTransition = 32,
    /// Issue #451: payer-provided memo does not match the required memo hash.
    MemoMismatch = 33,
    /// Issue #439: Creator is in cooldown after cancelling an invoice.
    CreatorCooldownActive = 34,
    /// Issue #420: Payment rejected because the invoice's `Cap` overfunding
    /// policy does not allow `funded` to exceed the invoice total.
    InvoiceFullyFunded = 35,
    /// The provided ratios do not sum to exactly BASIS_POINTS_TOTAL (10 000).
    InvalidRatioSum = 36,
    /// The recipient/ratio list is empty; at least one entry is required.
    EmptyRecipientList = 37,
    /// Reentrant call detected: a fund-moving function was invoked recursively
    /// within the same transaction. Cleared automatically at transaction boundary
    /// because the lock lives in temporary storage.
    ReentrantCall = 38,
    /// RBAC: Caller does not hold the required role for this entry point.
    RoleNotHeld = 39,
    /// Issue #482: Intermediate multiplication or division overflowed i128 bounds.
    ArithmeticOverflow = 40,
    /// Issue #483: A zero-value or negative amount was passed where a positive amount is required.
    ZeroAmountNotAllowed = 41,
    /// Issue #485: Caller is not on the invoice contributor allowlist.
    ContributorNotAllowed = 42,
    /// Issue #473: Token is not in the allowed tokens list.
    UnauthorisedToken = 43,
    /// Issue #456: Invoice dependency chain - predecessor invoice is not yet Released.
    DependencyNotMet = 44,
    /// Issue #456: Circular dependency detected in invoice dependency chain.
    CircularDependency = 45,
    /// Issue #453: Source contract has exceeded its call rate limit within the current window.
    SourceContractRateLimited = 46,
    /// Issue #559: creator fee bps + platform fee bps exceeds cap of 10000.
    FeeSumExceedsCap = 47,
    /// Issue #562: operation not allowed on a soft-deleted invoice.
    InvoiceDeleted = 48,
    /// Issue #562: invoice has unclaimed funds and cannot be soft-deleted.
    FundsUnclaimed = 49,
    /// Issue #562: attempted to read tombstone for an invoice that is not deleted.
    NotDeleted = 50,
    /// Issue #519: An invoice status transition is not permitted by the state machine.
    InvalidStateTransition = 52,
    /// Issue #518: A split ratio is invalid (e.g. >= denominator or sum mismatch).
    InvalidRatio = 53,
    /// Storage migration framework: `schema_version` is behind the version
    /// this Wasm build expects. Call `migrate` before retrying.
    MigrationRequired = 54,
    /// Issue #556: The recipient list supplied at invoice creation contains a
    /// duplicate address. Rejected before any storage is written.
    DuplicateRecipient = 55,
    /// Issue #557: Split ratios are locked after the first contribution is
    /// recorded and may no longer be mutated.
    SplitRatioLocked = 56,
    /// Issue #558: A recipient address has not established a trustline for the
    /// payment token. The offending address is surfaced in the panic message.
    RecipientMissingTrustline = 51,
    /// Issue #503: Creator has reached their open invoice limit.
    CreatorInvoiceLimitReached = 57,
    /// Issue #503: Co-creator count would exceed the maximum allowed.
    CoCreatorLimitReached = 58,
    /// Invoice is under active dispute and cannot be released.
    InvoiceDisputed = 59,
    /// Issue #503: Payer spend limit would be exceeded by this payment.
    PayerSpendLimitExceeded = 60,
    /// Issue #505: Recipient account does not exist on-ledger.
    RecipientAccountMissing = 61,
    /// Recipient not found in the invoice recipient list.
    RecipientNotFound = 62,
    /// Issue #522: Parent chain depth exceeds the allowed maximum.
    ParentChainTooDeep = 63,
    /// Issue #564: Checkpoint index does not match stored value during payout recovery.
    CheckpointMismatch = 64,
    /// Issue #564: Recipient at this index has already been paid in a prior payout attempt.
    AlreadyPaid = 65,
    /// Recipient list is shorter than the configured minimum recipient count.
    TooFewRecipients = 66,
    /// Issue #937: A co-signer signature failed cryptographic verification against
    /// the expected signer set (wrong signer, malformed payload, or bad signature bytes).
    InvalidSignature = 67,
    /// Issue #937: A required co-signer signature was not supplied for the operation.
    MissingSignature = 68,
    /// Issue #937: The supplied signature (or its nonce/ledger window) has already
    /// been consumed; replaying it is rejected.
    SignatureReplayed = 69,
    /// Issue #937: The signature's validity window has expired (e.g. signed ledger
    /// bound is in the past) and it can no longer be accepted.
    SignatureExpired = 70,
    /// Issue #938: The configured oracle address is malformed or not a valid
    /// contract address for the expected network.
    InvalidOracleAddress = 71,
    /// Issue #938: The oracle's reported condition (e.g. price threshold or
    /// resolution outcome) was not satisfied, so the operation cannot proceed.
    OracleConditionNotMet = 72,
    /// Issue #938: No oracle feed is configured for the requested asset/pair.
    OracleFeedNotFound = 73,
    /// Issue #951: A supplied argument failed a generic range/format validation
    /// check that has no more specific variant.
    InvalidArgument = 74,
    /// Issue #951: A required storage entry was absent when the operation needed it.
    MissingStorageEntry = 75,
    /// Issue #951: The requested operation is not supported by this contract version.
    UnsupportedOperation = 76,
    /// Issue #931: Deadline is further in the future than the allowed horizon.
    InvalidDeadline = 77,
    /// Issue #932: A recipient address is malformed or not a valid account/contract.
    InvalidRecipientAddress = 78,
    /// Issue #938: An oracle condition (threshold/comparison) is malformed.
    InvalidOracleCondition = 79,
    /// A shard index is outside the configured shard range.
    InvalidShardIndex = 80,
    /// A shard has reached its maximum entry capacity.
    ShardCapacityExceeded = 81,
    /// Issue #1011: Emergency pause recovery plan is not yet complete; unpause is blocked.
    RecoveryIncomplete = 82,
}

/// Issue #952: Human-readable, actionable recovery suggestions for every
/// `ContractError` variant. Each suggestion tells the caller what to change
/// before retrying so that failures never require guesswork.
pub mod recovery {
    use super::ContractError;
    use soroban_sdk::{Env, String};

    /// Returns a short, actionable recovery suggestion for `error`.
    ///
    /// The mapping is exhaustive: every `ContractError` variant has a
    /// dedicated suggestion. Unknown discriminants fall back to a generic
    /// "retry after checking inputs" message.
    pub fn suggestion_for(error: ContractError) -> &'static str {
        match error {
            ContractError::NotAuthorized => {
                "Ensure the caller is the admin, owner, or holds the required role/signer, then retry."
            }
            ContractError::InvoiceNotFound => {
                "Verify the invoice id exists (it may have been deleted); create the invoice first."
            }
            ContractError::DeadlinePassed => {
                "The funding deadline has passed; ask the creator to extend the deadline or open a new invoice."
            }
            ContractError::AlreadyFunded => {
                "The invoice is already fully funded; no further funding is accepted."
            }
            ContractError::InvalidAmount => {
                "Supply a positive amount that fits within the invoice's remaining capacity and i128 bounds."
            }
            ContractError::InvoiceFrozen => {
                "The invoice is frozen; wait for an admin to unfreeze it before retrying."
            }
            ContractError::InvalidStatus => {
                "Check the invoice status; perform the required prior transition (e.g. fund before release)."
            }
            ContractError::PayerNotAllowed => {
                "Use an address on the invoice's allowed-payer list, or ask the creator to add it."
            }
            ContractError::FundingInsufficient => {
                "Increase the payment to at least the remaining amount required to fully fund the invoice."
            }
            ContractError::OracleCallFailed => {
                "The oracle call failed; retry later or ask the admin to verify the oracle configuration."
            }
            ContractError::NotArbiter => {
                "Only the designated arbiter may perform this action; call from the arbiter address."
            }
            ContractError::NotDisputed => {
                "Open a dispute first; this action is only valid while a dispute is active."
            }
            ContractError::AlreadyExecuted => {
                "This action already completed and is not idempotent; do not retry it."
            }
            ContractError::TimelockPending => {
                "Wait until the time-lock/vesting window elapses, then retry."
            }
            ContractError::ContractPaused => {
                "The contract is paused; wait for an admin to unpause it before retrying."
            }
            ContractError::InvalidRecipients => {
                "Ensure recipients, amounts, and tokens lists have matching, non-empty lengths."
            }
            ContractError::PrerequisiteNotMet => {
                "Satisfy the prerequisite (invoice, milestone, or dependency) before retrying."
            }
            ContractError::BatchLimitExceeded => {
                "Split the batch into smaller chunks within the per-call item limit."
            }
            ContractError::RecipientAlreadyPaid => {
                "This recipient was already paid; skip them or use the payout-recovery path."
            }
            ContractError::FundsLockedUntil => {
                "Funds are still time-locked; wait until the lock expires, then release."
            }
            ContractError::StatsOverflow => {
                "Aggregate statistics would overflow; retry with a smaller amount or contact the admin."
            }
            ContractError::OracleUnavailable => {
                "The price oracle is unreachable or returned an invalid rate; retry later."
            }
            ContractError::InvalidRating => {
                "Supply a rating within the allowed range (1..=max score)."
            }
            ContractError::AlreadyRated => {
                "This target has already been rated by the caller; ratings are one-per-target."
            }
            ContractError::RateLimitExceeded => {
                "Wait for the current rate-limit window to reset before retrying."
            }
            ContractError::RecipientRevealMismatch => {
                "Verify the reveal preimage matches the stored commitment hash, then retry."
            }
            ContractError::PayoutNotYetClaimable => {
                "Wait until the claimable ledger is reached, then claim the payout."
            }
            ContractError::ContractFrozen => {
                "The contract is frozen for upgrade; wait for the upgrade to complete."
            }
            ContractError::DuplicatePayment => {
                "A matching payment was already recorded within the duplicate window; do not resubmit."
            }
            ContractError::GroupMemberExpired => {
                "A group member expired unfunded and the group rolled back; re-create the group."
            }
            ContractError::SlippageExceeded => {
                "Price moved beyond tolerance; retry with an updated quote or wider slippage tolerance."
            }
            ContractError::InvalidPhaseTransition => {
                "Check the current phase; only permitted phase transitions are allowed."
            }
            ContractError::MemoMismatch => {
                "Ensure the supplied memo hashes to the required memo hash."
            }
            ContractError::CreatorCooldownActive => {
                "The creator is in cooldown after a cancellation; wait for the cooldown to elapse."
            }
            ContractError::InvoiceFullyFunded => {
                "The invoice is fully funded under its Cap policy; no further funding is accepted."
            }
            ContractError::InvalidRatioSum => {
                "Adjust split ratios so they sum to exactly 10 000 basis points."
            }
            ContractError::EmptyRecipientList => {
                "Provide at least one recipient/ratio entry."
            }
            ContractError::ReentrantCall => {
                "A reentrant call was detected; retry the operation in a fresh transaction."
            }
            ContractError::RoleNotHeld => {
                "Grant the caller the required RBAC role, then retry."
            }
            ContractError::ArithmeticOverflow => {
                "Reduce the amount so intermediate arithmetic stays within i128 bounds."
            }
            ContractError::ZeroAmountNotAllowed => {
                "Supply a strictly positive amount."
            }
            ContractError::ContributorNotAllowed => {
                "Use an address on the invoice contributor allowlist, or ask the creator to add it."
            }
            ContractError::UnauthorisedToken => {
                "Use a token from the allowed tokens list, or ask the admin to allow it."
            }
            ContractError::DependencyNotMet => {
                "Wait for the predecessor invoice to reach Released before retrying."
            }
            ContractError::CircularDependency => {
                "Remove the circular reference from the invoice dependency chain."
            }
            ContractError::SourceContractRateLimited => {
                "The source contract exceeded its call rate limit; wait for the window to reset."
            }
            ContractError::FeeSumExceedsCap => {
                "Reduce creator + platform fee bps so their sum is at most 10 000."
            }
            ContractError::InvoiceDeleted => {
                "The invoice is soft-deleted; restore it or operate on a different invoice."
            }
            ContractError::FundsUnclaimed => {
                "Claim all outstanding funds before soft-deleting the invoice."
            }
            ContractError::NotDeleted => {
                "The invoice is not deleted; no tombstone exists to read."
            }
            ContractError::InvalidStateTransition => {
                "Check the invoice state machine; only permitted transitions are allowed."
            }
            ContractError::InvalidRatio => {
                "Ensure each split ratio is below the denominator and the set is consistent."
            }
            ContractError::MigrationRequired => {
                "Call `migrate` to bring storage to the expected schema version, then retry."
            }
            ContractError::DuplicateRecipient => {
                "Remove duplicate recipient addresses from the list."
            }
            ContractError::SplitRatioLocked => {
                "Split ratios are locked after the first contribution and cannot be changed."
            }
            ContractError::RecipientMissingTrustline => {
                "Ask the recipient to establish a trustline for the payment token, then retry."
            }
            ContractError::CreatorInvoiceLimitReached => {
                "Close or cancel an existing invoice before creating a new one."
            }
            ContractError::CoCreatorLimitReached => {
                "Reduce the number of co-creators to within the allowed maximum."
            }
            ContractError::InvoiceDisputed => {
                "Resolve the active dispute before attempting to release funds."
            }
            ContractError::PayerSpendLimitExceeded => {
                "Reduce the payment amount to stay within the payer's spend limit."
            }
            ContractError::RecipientAccountMissing => {
                "Ask the recipient to create their account on-ledger, then retry."
            }
            ContractError::RecipientNotFound => {
                "Use an address present in the invoice recipient list."
            }
            ContractError::ParentChainTooDeep => {
                "Shorten the parent chain to within the allowed maximum depth."
            }
            ContractError::CheckpointMismatch => {
                "Restart the payout from the last consistent checkpoint index."
            }
            ContractError::AlreadyPaid => {
                "This recipient was already paid; resume from the next unpaid index."
            }
            ContractError::TooFewRecipients => {
                "Add more recipients to meet the configured minimum recipient count."
            }
            ContractError::InvalidSignature => {
                "Re-sign the payload with the expected co-signer key and retry."
            }
            ContractError::MissingSignature => {
                "Supply every required co-signer signature, then retry."
            }
            ContractError::SignatureReplayed => {
                "This signature was already used; sign a fresh payload with a new nonce."
            }
            ContractError::SignatureExpired => {
                "The signature's validity window has passed; sign again with a current ledger bound."
            }
            ContractError::InvalidOracleAddress => {
                "Configure a valid oracle contract address for this network."
            }
            ContractError::OracleConditionNotMet => {
                "The oracle condition is not yet satisfied; wait for it to hold, then retry."
            }
            ContractError::OracleFeedNotFound => {
                "Ask the admin to configure an oracle feed for this asset pair."
            }
            ContractError::InvalidArgument => {
                "Check the argument ranges and formats, then retry."
            }
            ContractError::MissingStorageEntry => {
                "Create the required record first (e.g. initialize or configure), then retry."
            }
            ContractError::UnsupportedOperation => {
                "This operation is not supported by the deployed contract version."
            }
            ContractError::InvalidDeadline => {
                "Choose a deadline within the allowed horizon (about 10 years) from now."
            }
            ContractError::InvalidRecipientAddress => {
                "Use a valid Stellar account (G...) or contract (C...) address for every recipient."
            }
            ContractError::InvalidOracleCondition => {
                "Use a positive threshold and a supported comparison operator."
            }
            ContractError::InvalidShardIndex => {
                "Use a shard index within the configured shard count."
            }
            ContractError::ShardCapacityExceeded => {
                "The shard is full; write to a different shard or archive old entries."
            }
            ContractError::RecoveryIncomplete => {
                "Complete all recovery plan steps before unpausing the contract."
            }
        }
    }

    /// Env-aware variant returning a `soroban_sdk::String` suitable for
    /// returning from a contract entrypoint.
    pub fn get_recovery_suggestion(env: &Env, error: ContractError) -> String {
        String::from_str(env, suggestion_for(error))
    }
}
