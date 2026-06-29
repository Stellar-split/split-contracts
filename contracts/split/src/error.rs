use soroban_sdk::contracterror;

/// Unified error taxonomy (issue #273). Discriminants are stable — never reorder, only append.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    NotAuthorized      = 1,
    InvoiceNotFound    = 2,
    DeadlinePassed     = 3,
    AlreadyFunded      = 4,
    InvalidAmount      = 5,
    InvoiceFrozen      = 6,
    InvalidStatus      = 7,
    PayerNotAllowed    = 8,
    FundingInsufficient = 9,
    OracleCallFailed   = 10,
    NotArbiter         = 11,
    NotDisputed        = 12,
    AlreadyExecuted    = 13,
    TimelockPending    = 14,
    ContractPaused     = 15,
    InvalidRecipients  = 16,
    PrerequisiteNotMet = 17,
    BatchLimitExceeded = 18,
}
