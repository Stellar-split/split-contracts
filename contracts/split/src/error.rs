use soroban_sdk::contracterror;

/// Unified error taxonomy (issue #273). Discriminants are stable — never reorder, only append.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    NotAuthorized = 1,
    InvoiceNotFound = 2,
    DeadlinePassed = 3,
    AlreadyFunded = 4,
    InvalidAmount = 5,
    InvoiceFrozen = 6,
    InvalidStatus = 7,
    PayerNotAllowed = 8,
    FundingInsufficient = 9,
    OracleCallFailed = 10,
    NotArbiter = 11,
    NotDisputed = 12,
    AlreadyExecuted = 13,
    TimelockPending = 14,
    ContractPaused = 15,
    InvalidRecipients = 16,
    PrerequisiteNotMet = 17,
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
    InvalidRating = 23,
    AlreadyRated = 24,
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
    /// Issue #519: An invoice status transition is not permitted by the state machine.
    InvalidStateTransition = 47,
    /// Issue #518: A split ratio is invalid (e.g. >= denominator or sum mismatch).
    InvalidRatio = 48,
    /// Storage migration framework: `schema_version` is behind the version
    /// this Wasm build expects. Call `migrate` before retrying.
    MigrationRequired = 47,
}
