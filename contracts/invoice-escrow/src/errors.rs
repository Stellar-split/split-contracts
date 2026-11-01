//! Error variants for the invoice-escrow contract.
//!
//! Discriminants are stable — never reorder, only append.

use soroban_sdk::{contracterror, contracttype, Env, String};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    /// Caller is not the contract admin.
    NotAdmin = 1,
    /// No pending admin transfer exists to accept or cancel.
    NoPendingAdmin = 2,
    /// Caller is not the pending admin (for accept_admin).
    Unauthorized = 3,
    /// Contract has already been initialized.
    AlreadyInitialized = 4,
    /// Contract has not been initialized yet.
    NotInitialized = 5,
    /// Invoice ID was not found in storage.
    InvoiceNotFound = 6,
    /// The invoice deadline has already passed.
    DeadlinePassed = 7,
    /// The invoice is in a state that does not allow this operation.
    InvalidStatus = 8,
    /// Amount must be greater than zero.
    InvalidAmount = 9,
    /// Total amount must be greater than zero.
    InvalidTotalAmount = 10,
    /// Deposit would exceed the invoice total; partial overflow not accepted.
    OverFunded = 11,
    /// The invoice is not yet fully funded; release is not permitted.
    InsufficientFunding = 12,
    /// The deadline has not yet passed; refund is not permitted.
    DeadlineNotPassed = 13,
    /// Recipient and amount vectors must be the same length and non-empty.
    InvalidRecipients = 14,
    /// Payer is blacklisted and cannot interact with invoices.
    PayerBlacklisted = 15,
    /// The blacklist appeal window has expired.
    AppealWindowExpired = 16,
    /// The payer is not blacklisted; cannot submit appeal or finalise.
    NotBlacklisted = 17,
    /// The blacklist entry has already been finalised.
    AlreadyFinalised = 18,
    /// Caller is not the blacklisted payer.
    NotBlacklistedPayer = 19,
    /// Co-signer list is empty, has duplicates, exceeds the maximum, or the
    /// threshold is outside `1..=signers.len()`.
    InvalidCosignerConfig = 20,
    /// Caller is not one of the invoice's configured co-signers.
    NotCosigner = 21,
    /// Co-signer has already approved or rejected this invoice.
    AlreadyVoted = 22,
    /// Release requires co-signer approval that has not yet reached threshold.
    ApprovalRequired = 23,
    /// Co-signers rejected the release; threshold can no longer be reached.
    ApprovalRejected = 24,
    /// Co-signer has not voted, so there is nothing to revoke.
    NotVoted = 25,
    /// Co-signer configuration cannot change once voting has started.
    CosignersLocked = 26,
    /// No co-signer approval is configured for this invoice.
    NoCosigners = 27,
    /// Oracle condition is malformed (non-positive threshold, zero staleness
    /// window, or fallback identical to the primary oracle).
    InvalidOracleConfig = 28,
    /// No oracle could be reached or none returned a price for the asset.
    OracleUnavailable = 29,
    /// The oracle price is older than the condition's staleness window.
    OracleStalePrice = 30,
    /// The oracle returned a non-positive price or a future timestamp.
    OracleInvalidPrice = 31,
    /// The oracle price does not satisfy the release condition.
    OracleConditionNotMet = 32,
    /// No oracle condition is configured for this invoice.
    NoOracleCondition = 33,
    /// Schedule is malformed: `release_at` not in the future, or the ledger
    /// delay exceeds the maximum / overflows.
    InvalidSchedule = 34,
    /// The scheduled release time has not been reached yet.
    ReleaseNotYetDue = 35,
    /// The scheduled minimum ledger count has not elapsed yet.
    ScheduleLedgerNotReached = 36,
    /// No active release schedule exists for this invoice.
    ScheduleNotFound = 37,
    /// The refund grace period is still running; refund is not permitted yet.
    RefundGraceActive = 38,
    /// Grace period exceeds `MAX_REFUND_GRACE_SECS`.
    InvalidGracePeriod = 39,
    /// The refund grace period has already elapsed or been waived.
    RefundGraceElapsed = 40,
    /// The deadline is too far in the future; it exceeds `MAX_DEADLINE_HORIZON`.
    DeadlineOutOfRange = 41,
}

/// Returns a human-readable recovery suggestion for the given error.
///
/// The suggestion describes how a caller can recover from the failure or
/// what state the contract is in, so clients can surface actionable guidance.
pub fn get_recovery_suggestion(error: Error) -> String {
    match error {
        Error::NotAdmin => String::from_str(
            &Env::default(),
            "Only the contract admin may perform this action; retry from the admin account.",
        ),
        Error::NoPendingAdmin => String::from_str(
            &Env::default(),
            "No admin transfer is pending; start a transfer with propose_admin first.",
        ),
        Error::Unauthorized => String::from_str(
            &Env::default(),
            "Caller is not authorized; retry from the pending admin account.",
        ),
        Error::AlreadyInitialized => String::from_str(
            &Env::default(),
            "Contract is already initialized; no further initialization is required.",
        ),
        Error::NotInitialized => String::from_str(
            &Env::default(),
            "Contract is not initialized; call initialize before any other operation.",
        ),
        Error::InvoiceNotFound => String::from_str(
            &Env::default(),
            "Invoice does not exist; verify the invoice ID or create the invoice first.",
        ),
        Error::DeadlinePassed => String::from_str(
            &Env::default(),
            "Invoice deadline has passed; create a new invoice or request a refund.",
        ),
        Error::InvalidStatus => String::from_str(
            &Env::default(),
            "Invoice is in the wrong state for this operation; check the current status and retry when valid.",
        ),
        Error::InvalidAmount => String::from_str(
            &Env::default(),
            "Amount must be greater than zero; retry with a positive amount.",
        ),
        Error::InvalidTotalAmount => String::from_str(
            &Env::default(),
            "Total amount must be greater than zero; retry with a positive total.",
        ),
        Error::OverFunded => String::from_str(
            &Env::default(),
            "Deposit would exceed the invoice total; reduce the deposit to the remaining balance.",
        ),
        Error::InsufficientFunding => String::from_str(
            &Env::default(),
            "Invoice is not fully funded; fund the remaining balance before releasing.",
        ),
        Error::DeadlineNotPassed => String::from_str(
            &Env::default(),
            "Deadline has not passed yet; wait until the deadline before requesting a refund.",
        ),
        Error::InvalidRecipients => String::from_str(
            &Env::default(),
            "Recipients and amounts must be non-empty and equal in length; correct the inputs and retry.",
        ),
        Error::PayerBlacklisted => String::from_str(
            &Env::default(),
            "Payer is blacklisted; submit an appeal or wait for the appeal window to resolve.",
        ),
        Error::AppealWindowExpired => String::from_str(
            &Env::default(),
            "Appeal window has expired; no further appeals can be submitted.",
        ),
        Error::NotBlacklisted => String::from_str(
            &Env::default(),
            "Payer is not blacklisted; no appeal or finalisation is needed.",
        ),
        Error::AlreadyFinalised => String::from_str(
            &Env::default(),
            "Blacklist entry is already finalised; no further action is possible.",
        ),
        Error::NotBlacklistedPayer => String::from_str(
            &Env::default(),
            "Only the blacklisted payer may perform this action; retry from the payer account.",
        ),
        Error::InvalidCosignerConfig => String::from_str(
            &Env::default(),
            "Co-signer configuration is invalid; ensure the list is non-empty, has no duplicates, and threshold is valid.",
        ),
        Error::NotCosigner => String::from_str(
            &Env::default(),
            "Caller is not a configured co-signer for this invoice.",
        ),
        Error::AlreadyVoted => String::from_str(
            &Env::default(),
            "Co-signer has already voted; vote cannot be changed.",
        ),
        Error::ApprovalRequired => String::from_str(
            &Env::default(),
            "Co-signer approval is required but has not reached the required threshold.",
        ),
        Error::ApprovalRejected => String::from_str(
            &Env::default(),
            "Co-signers have rejected the release; threshold can no longer be reached.",
        ),
        Error::NotVoted => String::from_str(
            &Env::default(),
            "Co-signer has not voted yet; there is nothing to revoke.",
        ),
        Error::CosignersLocked => String::from_str(
            &Env::default(),
            "Co-signer configuration is locked and cannot be changed after voting has started.",
        ),
        Error::NoCosigners => String::from_str(
            &Env::default(),
            "No co-signer approval is configured for this invoice.",
        ),
        Error::InvalidOracleConfig => String::from_str(
            &Env::default(),
            "Oracle condition configuration is invalid; check threshold, staleness window, and oracle addresses.",
        ),
        Error::OracleUnavailable => String::from_str(
            &Env::default(),
            "Oracle is unavailable or did not return a price; try again later.",
        ),
        Error::OracleStalePrice => String::from_str(
            &Env::default(),
            "Oracle price is too old and exceeds the staleness window; wait for a fresh price.",
        ),
        Error::OracleInvalidPrice => String::from_str(
            &Env::default(),
            "Oracle returned an invalid price (non-positive or future timestamp).",
        ),
        Error::OracleConditionNotMet => String::from_str(
            &Env::default(),
            "Oracle price does not satisfy the release condition; condition must be met before release.",
        ),
        Error::NoOracleCondition => String::from_str(
            &Env::default(),
            "No oracle condition is configured for this invoice.",
        ),
        Error::InvalidSchedule => String::from_str(
            &Env::default(),
            "Release schedule is invalid; ensure release_at is in the future and ledger delay is within limits.",
        ),
        Error::ReleaseNotYetDue => String::from_str(
            &Env::default(),
            "Scheduled release time has not been reached yet; try again later.",
        ),
        Error::ScheduleLedgerNotReached => String::from_str(
            &Env::default(),
            "Scheduled minimum ledger count has not elapsed yet; try again later.",
        ),
        Error::ScheduleNotFound => String::from_str(
            &Env::default(),
            "No active release schedule exists for this invoice.",
        ),
        Error::RefundGraceActive => String::from_str(
            &Env::default(),
            "Refund grace period is still active; refund is not permitted yet.",
        ),
        Error::InvalidGracePeriod => String::from_str(
            &Env::default(),
            "Refund grace period exceeds the maximum allowed duration.",
        ),
        Error::RefundGraceElapsed => String::from_str(
            &Env::default(),
            "Refund grace period has already elapsed or been waived; no refund is possible.",
        ),
        Error::DeadlineOutOfRange => String::from_str(
            &Env::default(),
            "Deadline is too far in the future; it exceeds the maximum allowed horizon.",
        ),
    }
}

/// Maps a raw error code to its `Error` variant, if it is a known code.
pub fn error_from_code(code: u32) -> Option<Error> {
    match code {
        1 => Some(Error::NotAdmin),
        2 => Some(Error::NoPendingAdmin),
        3 => Some(Error::Unauthorized),
        4 => Some(Error::AlreadyInitialized),
        5 => Some(Error::NotInitialized),
        6 => Some(Error::InvoiceNotFound),
        7 => Some(Error::DeadlinePassed),
        8 => Some(Error::InvalidStatus),
        9 => Some(Error::InvalidAmount),
        10 => Some(Error::InvalidTotalAmount),
        11 => Some(Error::OverFunded),
        12 => Some(Error::InsufficientFunding),
        13 => Some(Error::DeadlineNotPassed),
        14 => Some(Error::InvalidRecipients),
        15 => Some(Error::PayerBlacklisted),
        16 => Some(Error::AppealWindowExpired),
        17 => Some(Error::NotBlacklisted),
        18 => Some(Error::AlreadyFinalised),
        19 => Some(Error::NotBlacklistedPayer),
        20 => Some(Error::InvalidCosignerConfig),
        21 => Some(Error::NotCosigner),
        22 => Some(Error::AlreadyVoted),
        23 => Some(Error::ApprovalRequired),
        24 => Some(Error::ApprovalRejected),
        25 => Some(Error::NotVoted),
        26 => Some(Error::CosignersLocked),
        27 => Some(Error::NoCosigners),
        28 => Some(Error::InvalidOracleConfig),
        29 => Some(Error::OracleUnavailable),
        30 => Some(Error::OracleStalePrice),
        31 => Some(Error::OracleInvalidPrice),
        32 => Some(Error::OracleConditionNotMet),
        33 => Some(Error::NoOracleCondition),
        34 => Some(Error::InvalidSchedule),
        35 => Some(Error::ReleaseNotYetDue),
        36 => Some(Error::ScheduleLedgerNotReached),
        37 => Some(Error::ScheduleNotFound),
        38 => Some(Error::RefundGraceActive),
        39 => Some(Error::InvalidGracePeriod),
        40 => Some(Error::RefundGraceElapsed),
        41 => Some(Error::DeadlineOutOfRange),
        _ => None,
    }
}

/// Storage key for the recovery-suggestion entrypoint's last queried code.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecoveryKey {
    /// Last error code queried through the recovery entrypoint.
    LastErrorCode,
}
