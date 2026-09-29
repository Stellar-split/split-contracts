//! Error variants for the invoice-escrow contract.
//!
//! Discriminants are stable — never reorder, only append.

use soroban_sdk::contracterror;

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

    // ── Recipient approval workflow (#855) ──────────────────────────────
    /// Invoice has a recipient proposal that has not been approved yet.
    RecipientsNotApproved = 20,
    /// Recipient shares must be non-zero and sum to exactly 10 000 bps.
    InvalidShares = 21,
    /// No recipient proposal exists for this invoice.
    ProposalNotFound = 22,
    /// Caller is not one of the proposed recipients (or has no vote to revoke).
    NotARecipient = 23,
    /// Recipient has already approved or rejected this proposal version.
    AlreadyVoted = 24,
    /// Vote targets a proposal version that has been superseded.
    ProposalVersionMismatch = 25,
    /// Proposal is no longer pending (already approved or rejected).
    ProposalClosed = 26,
    /// Required approvals must be between 1 and the number of recipients.
    InvalidThreshold = 27,

    // ── Milestone escrow (#854) ─────────────────────────────────────────
    /// Milestone escrow ID was not found in storage.
    MilestoneEscrowNotFound = 28,
    /// Milestone list is empty, too long, or contains a non-positive amount.
    InvalidMilestones = 29,
    /// Milestone index is outside the escrow's milestone list.
    MilestoneIndexOutOfRange = 30,
    /// Milestone has already been released.
    MilestoneAlreadyReleased = 31,
    /// The milestone's trigger condition is not satisfied for this caller.
    TriggerNotSatisfied = 32,
    /// Milestone escrow is completed or cancelled.
    EscrowClosed = 33,

    // ── Insurance pool (#853) ───────────────────────────────────────────
    /// No refund-protection policy exists for this payer and invoice.
    PolicyNotFound = 34,
    /// Payer already holds a policy on this invoice.
    PolicyAlreadyExists = 35,
    /// Payer has no deposit on this invoice to protect.
    NoDepositToInsure = 36,
    /// Pool has too little unlocked (or any) liquidity for this operation.
    InsufficientPoolLiquidity = 37,
    /// Provider does not hold enough pool shares.
    InsufficientShares = 38,
    /// Policy is not active (already claimed or expired).
    PolicyNotActive = 39,
    /// Invoice default has already been declared.
    InvoiceAlreadyDefaulted = 40,
    /// Insurance configuration value is out of range.
    InvalidConfig = 41,

    // ── Lending marketplace (#856) ──────────────────────────────────────
    /// No loan listing exists for this invoice.
    LoanNotFound = 42,
    /// Loan listing is not open for funding or cancellation.
    LoanNotOpen = 43,
    /// Loan has not been funded (or is already settled).
    LoanNotFunded = 44,
    /// Invoice already has an open or funded loan.
    ActiveLoanExists = 45,
    /// Invoice has a recipient proposal; it cannot be used as collateral.
    RecipientProposalExists = 46,
    /// Principal / repayment / expiry terms are invalid.
    InvalidLoanTerms = 47,

    // ── Shared ──────────────────────────────────────────────────────────
    /// A bounded list (marketplace listings, policies per invoice) is full.
    CapacityReached = 48,
    /// Counterparties must be distinct addresses.
    SelfDealing = 49,
    /// Caller is not the invoice creator.
    NotCreator = 20,
}
