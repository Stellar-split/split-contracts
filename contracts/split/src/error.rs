use soroban_sdk::contracterror;

/// Unified error taxonomy (issue #273). Discriminants are stable — never reorder, only append.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    NotAuthorized        = 1,
    InvoiceNotFound      = 2,
    DeadlinePassed       = 3,
    AlreadyFunded        = 4,
    InvalidAmount        = 5,
    InvoiceFrozen        = 6,
    InvalidStatus        = 7,
    PayerNotAllowed      = 8,
    FundingInsufficient  = 9,
    OracleCallFailed     = 10,
    NotArbiter           = 11,
    NotDisputed          = 12,
    AlreadyExecuted      = 13,
    TimelockPending      = 14,
    ContractPaused       = 15,
    InvalidRecipients    = 16,
    PrerequisiteNotMet   = 17,
    BatchLimitExceeded   = 18,
    /// Issue #330: Recipient has already been paid on this invoice.
    RecipientAlreadyPaid = 19,
    /// Issue #327: Funds are still time-locked and cannot be released yet.
    FundsLockedUntil     = 20,
    /// Aggregate protocol statistics would exceed their numeric bounds.
    StatsOverflow        = 21,
    /// Oracle-priced invoice: the configured price oracle is unreachable or returned a
    /// non-positive rate at payment time.
    OracleUnavailable    = 22,
    InvalidRating        = 23,
    AlreadyRated         = 24,
    RateLimitExceeded    = 25,
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
    MemoMismatch = 31,
    /// Issue #439: Creator is in cooldown after cancelling an invoice.
    CreatorCooldownActive = 31,
    /// The provided ratios do not sum to exactly BASIS_POINTS_TOTAL (10 000).
    InvalidRatioSum = 33,
    /// The recipient/ratio list is empty; at least one entry is required.
    EmptyRecipientList = 34,
}
