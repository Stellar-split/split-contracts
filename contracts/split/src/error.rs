use soroban_sdk::contracterror;

/// Unified error taxonomy (issue #273). Discriminants are stable — never reorder, only append.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    NotAuthorized       = 1,
    InvoiceNotFound     = 2,
    DeadlinePassed      = 3,
    AlreadyFunded       = 4,
    InvalidAmount       = 5,
    InvoiceFrozen       = 6,
    InvalidStatus       = 7,
    PayerNotAllowed     = 8,
    FundingInsufficient = 9,
    OracleCallFailed    = 10,
    NotArbiter          = 11,
    NotDisputed         = 12,
    AlreadyExecuted     = 13,
    TimelockPending     = 14,
    ContractPaused      = 15,
    InvalidRecipients   = 16,
    PrerequisiteNotMet  = 17,
    BatchLimitExceeded  = 18,
    /// Issue #330: Recipient has already been paid on this invoice.
    RecipientAlreadyPaid = 19,
    /// Issue #327: Funds are still time-locked and cannot be released yet.
    FundsLockedUntil    = 20,
    /// Aggregate protocol statistics would exceed their numeric bounds.
    StatsOverflow       = 21,
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
    /// Issue #327: Funds are still time-locked and cannot be released yet.
    FundsLockedUntil = 20,
    /// Issue #330: Recipient has already been paid on this invoice.
    RecipientAlreadyPaid = 19,
    /// Oracle-priced invoice: the configured price oracle is unreachable, trapped, or
    /// returned a non-positive rate at payment time.
    OracleUnavailable  = 21,
}
