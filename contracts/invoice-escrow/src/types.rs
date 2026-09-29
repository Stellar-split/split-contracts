//! Type definitions for the invoice-escrow contract.

use soroban_sdk::{contracttype, Address, BytesN, Vec};

/// Lifecycle state of an escrow invoice.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum EscrowStatus {
    /// Invoice has been created but no funds deposited yet.
    Pending,
    /// At least one deposit has been received; not yet fully funded.
    Active,
    /// Invoice is fully funded and funds have been released to recipients.
    Released,
    /// Deadline passed without full funding; payers have been refunded.
    Refunded,
    /// Admin explicitly cancelled the invoice before release.
    Cancelled,
}

/// Core escrow invoice record.
#[contracttype]
#[derive(Clone, Debug)]
pub struct EscrowInvoice {
    /// Address that created the invoice.
    pub creator: Address,
    /// Token contract address (e.g. USDC).
    pub token: Address,
    /// Total amount required to fully fund this invoice.
    pub total_amount: i128,
    /// Amount deposited so far.
    pub funded_amount: i128,
    /// Unix timestamp after which refunds are permitted if not fully funded.
    pub deadline: u64,
    /// Current lifecycle status.
    pub status: EscrowStatus,
}

/// Emitted when the admin initiates a two-step admin authority transfer.
///
/// Event topics: `(escrow, adm_prop)`
/// Event data: `(current_admin, proposed_admin, ledger)`
#[contracttype]
#[derive(Clone, Debug)]
pub struct AdminTransferProposedEvent {
    /// The current admin initiating the transfer.
    pub current_admin: Address,
    /// The new admin being proposed.
    pub proposed_admin: Address,
    /// Ledger sequence at which the proposal was made.
    pub ledger: u32,
}

/// Emitted when the proposed admin accepts the role.
///
/// Event topics: `(escrow, adm_accpt)`
/// Event data: `(new_admin, ledger)`
#[contracttype]
#[derive(Clone, Debug)]
pub struct AdminTransferAcceptedEvent {
    /// The address that accepted the admin role.
    pub new_admin: Address,
    /// Ledger sequence at which acceptance occurred.
    pub ledger: u32,
}

/// Emitted when the current admin cancels a pending transfer.
///
/// Event topics: `(escrow, adm_cncl)`
/// Event data: `(admin, cancelled_pending, ledger)`
#[contracttype]
#[derive(Clone, Debug)]
pub struct AdminTransferCancelledEvent {
    /// The admin who cancelled the pending transfer.
    pub admin: Address,
    /// The pending admin address that was cancelled.
    pub cancelled_pending: Address,
    /// Ledger sequence at which cancellation occurred.
    pub ledger: u32,
}

// ──────────────────────────────────────────────────────────────────────
// Escrow release event
// ──────────────────────────────────────────────────────────────────────

/// Emitted after funds are successfully transferred to a recipient during
/// a release call.
///
/// Event topics: `(escrow, released)`
/// Event data: `EscrowReleased { invoice_id, recipient, amount }`
#[contracttype]
#[derive(Clone, Debug)]
pub struct EscrowReleased {
    /// The ID of the invoice that was released.
    pub invoice_id: u64,
    /// The address that received the released funds.
    pub recipient: Address,
    /// The amount of tokens transferred to the recipient.
    pub amount: i128,
}

// ──────────────────────────────────────────────────────────────────────
// Payer Blacklist types
// ──────────────────────────────────────────────────────────────────────

// ──────────────────────────────────────────────────────────────────────
// Invoice config versioning
// ──────────────────────────────────────────────────────────────────────

/// A snapshot of an invoice's mutable config, recorded before an update is
/// applied via `update_invoice_config`. Lets creators evolve `total_amount`
/// and `deadline` while preserving a full audit trail of prior versions.
#[contracttype]
#[derive(Clone, Debug)]
pub struct InvoiceConfigVersion {
    /// Version number, starting at 1 for the invoice's original config.
    pub version: u32,
    /// `total_amount` in effect for this version.
    pub total_amount: i128,
    /// `deadline` in effect for this version.
    pub deadline: u64,
    /// Timestamp at which this version was superseded.
    pub updated_at: u64,
}

/// Entry in the payer blacklist.
///
/// When a payer is blacklisted by the admin, an entry is created with
/// `finalised: false`. The payer can submit an appeal during the
/// `APPEAL_WINDOW_LEDGERS` window. After the window closes (or the payer
/// submits an appeal), the admin calls `finalise_blacklist` with either
/// `uphold: true` (ban stays) or `uphold: false` (payer reinstated).
#[contracttype]
#[derive(Clone, Debug)]
pub struct BlacklistEntry {
    /// Unix timestamp when the blacklist entry was created.
    pub banned_at: u64,
    /// Optional appeal hash submitted by the payer (32-byte hash).
    pub appeal_hash: Option<BytesN<32>>,
    /// Whether the blacklist entry has been finalised by the admin.
    pub finalised: bool,
    /// Whether the ban was upheld after finalisation. Only meaningful
    /// when `finalised` is `true`.
    pub upheld: bool,
    /// Hash of the reason for blacklisting (provided by admin).
    pub reason_hash: BytesN<32>,
}

// ──────────────────────────────────────────────────────────────────────
// Recipient approval workflow types (#855)
// ──────────────────────────────────────────────────────────────────────

/// Lifecycle state of a recipient proposal.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalStatus {
    /// Waiting for recipient votes.
    Pending,
    /// `required_approvals` recipients approved; release pays the recipients.
    Approved,
    /// Enough recipients rejected that the threshold can no longer be met.
    Rejected,
}

/// Creator-proposed payout split for an escrow invoice that the listed
/// recipients must approve before funds can be released.
///
/// Each call to `propose_recipients` bumps `version` and clears all votes, so
/// recipients always vote on the exact split they will receive.
#[contracttype]
#[derive(Clone, Debug)]
pub struct RecipientProposal {
    /// Monotonic revision number, starting at 1.
    pub version: u32,
    /// Proposed recipients (unique).
    pub recipients: Vec<Address>,
    /// Share of the release for each recipient, in basis points (sum 10 000).
    pub shares_bps: Vec<u32>,
    /// Number of approvals needed for the proposal to pass (N-of-M).
    pub required_approvals: u32,
    /// Recipients that approved this version.
    pub approvals: Vec<Address>,
    /// Recipients that rejected this version.
    pub rejections: Vec<Address>,
    /// Unix timestamp after which votes are no longer accepted.
    pub approval_deadline: u64,
    /// Current status.
    pub status: ProposalStatus,
}

// ──────────────────────────────────────────────────────────────────────
// Milestone escrow types (#854)
// ──────────────────────────────────────────────────────────────────────

/// Condition that must hold for a milestone payment to be released.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum MilestoneTrigger {
    /// The payer must sign the release.
    PayerApproval,
    /// Anyone may release once the ledger timestamp reaches this value.
    Timestamp(u64),
    /// The given arbiter (e.g. an oracle or mediator) must sign the release.
    Arbiter(Address),
}

/// Caller-supplied milestone definition for `create_milestone_escrow`.
#[contracttype]
#[derive(Clone, Debug)]
pub struct MilestoneInput {
    /// Amount paid to the payee when this milestone is released.
    pub amount: i128,
    /// Release condition.
    pub trigger: MilestoneTrigger,
}

/// Stored milestone state.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Milestone {
    /// Amount paid to the payee when this milestone is released.
    pub amount: i128,
    /// Release condition.
    pub trigger: MilestoneTrigger,
    /// Whether this milestone has been paid out.
    pub released: bool,
}

/// Lifecycle state of a milestone escrow.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum MilestoneEscrowStatus {
    /// Funds are held; at least one milestone is unreleased.
    Active,
    /// Every milestone was paid to the payee.
    Completed,
    /// Escrow was closed and the unreleased remainder refunded to the payer.
    Cancelled,
}

/// Fully-prefunded escrow that pays the payee milestone by milestone.
#[contracttype]
#[derive(Clone, Debug)]
pub struct MilestoneEscrow {
    /// Address that funded the escrow.
    pub payer: Address,
    /// Address that receives milestone payments.
    pub payee: Address,
    /// Token contract address.
    pub token: Address,
    /// Sum of all milestone amounts (deposited up front).
    pub total_amount: i128,
    /// Amount paid out to the payee so far.
    pub released_amount: i128,
    /// Ordered milestone list.
    pub milestones: Vec<Milestone>,
    /// Current status.
    pub status: MilestoneEscrowStatus,
    /// Unix timestamp at creation.
    pub created_at: u64,
}

// ──────────────────────────────────────────────────────────────────────
// Insurance pool types (#853)
// ──────────────────────────────────────────────────────────────────────

/// Admin-tunable insurance parameters.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct InsuranceConfig {
    /// Premium charged on purchase, in basis points of the coverage.
    pub premium_bps: u32,
    /// How long a policy stays claimable after purchase, in seconds.
    pub policy_duration: u64,
}

/// Per-token pool of underwriter liquidity.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct InsurancePool {
    /// Tokens held by the pool (provider deposits + premiums − claims − withdrawals).
    pub total_liquidity: i128,
    /// Coverage reserved by active policies; cannot be withdrawn.
    pub locked_coverage: i128,
    /// Outstanding provider shares.
    pub total_shares: i128,
    /// Lifetime premiums collected.
    pub premiums_collected: i128,
    /// Lifetime claims paid out.
    pub claims_paid: i128,
}

/// Lifecycle state of a refund-protection policy.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum PolicyStatus {
    /// Coverage is reserved and claimable.
    Active,
    /// Coverage was paid out to the payer.
    Claimed,
    /// Policy lapsed (term ended or invoice refunded/cancelled normally).
    Expired,
}

/// Refund protection bought by a payer on one escrow invoice.
#[contracttype]
#[derive(Clone, Debug)]
pub struct InsurancePolicy {
    /// Insured payer.
    pub payer: Address,
    /// Invoice the policy covers.
    pub invoice_id: u64,
    /// Token used for premium and payout (the invoice token).
    pub token: Address,
    /// Amount refunded to the payer if the invoice defaults.
    pub coverage: i128,
    /// Premium paid.
    pub premium: i128,
    /// Unix timestamp after which the policy can no longer be claimed.
    pub expires_at: u64,
    /// Current status.
    pub status: PolicyStatus,
}

// ──────────────────────────────────────────────────────────────────────
// Lending marketplace types (#856)
// ──────────────────────────────────────────────────────────────────────

/// Lifecycle state of an invoice-backed loan.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum LoanStatus {
    /// Listed on the marketplace, waiting for a lender.
    Open,
    /// Lender advanced the principal; repayment is owed.
    Funded,
    /// Lender was repaid (from the invoice release or directly).
    Repaid,
    /// Invoice was refunded/cancelled while the loan was outstanding.
    Defaulted,
    /// Borrower withdrew the listing (or it lapsed on release).
    Cancelled,
}

/// Invoice creator's request to borrow against an escrow invoice.
#[contracttype]
#[derive(Clone, Debug)]
pub struct LoanListing {
    /// Invoice used as collateral.
    pub invoice_id: u64,
    /// Borrower (the invoice creator).
    pub borrower: Address,
    /// Loan token (the invoice token).
    pub token: Address,
    /// Amount advanced by the lender to the borrower.
    pub principal: i128,
    /// Amount owed to the lender, paid first out of the invoice release.
    pub repayment: i128,
    /// Unix timestamp after which the listing can no longer be funded.
    pub expires_at: u64,
    /// Current lender, once funded.
    pub lender: Option<Address>,
    /// Current status.
    pub status: LoanStatus,
    /// Unix timestamp at which the loan was funded (0 while open).
    pub funded_at: u64,
}
