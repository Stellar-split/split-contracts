//! Type definitions for the invoice-escrow contract.

use soroban_sdk::{contracttype, Address, BytesN, Vec};

/// Reason a refund was issued, recorded on the invoice and emitted in the
/// refund event so indexers can categorise refunds.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq, Copy)]
pub enum RefundReason {
    /// Deadline passed without the invoice being fully funded.
    DeadlineExpired = 0,
    /// Admin cancelled the invoice before release.
    AdminCancelled = 1,
    /// Invoice was over-funded and the excess is being returned.
    Overfunded = 2,
    /// Payer requested a refund before the deadline.
    PayerRequested = 3,
    /// Refund issued as part of an error recovery path.
    ErrorRecovery = 4,
}

/// Wrapper for Option<RefundReason> to work around Soroban serialization limitations.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OptionalRefundReason {
    /// No refund reason set.
    None,
    /// A refund reason is set.
    Some(RefundReason),
}

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
    /// Reason recorded for the most recent refund, if any.
    pub refund_reason: OptionalRefundReason,
}

/// Emitted when funds are refunded to a payer.
///
/// Event topics: `(escrow, refunded)`
/// Event data: `EscrowRefunded { invoice_id, payer, amount, reason }`
#[contracttype]
#[derive(Clone, Debug)]
pub struct EscrowRefunded {
    /// The ID of the invoice that was refunded.
    pub invoice_id: u64,
    /// The address that received the refund.
    pub payer: Address,
    /// The amount of tokens refunded.
    pub amount: i128,
    /// The reason the refund was issued.
    pub reason: RefundReason,
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
// Co-signer approval types
// ──────────────────────────────────────────────────────────────────────

/// Aggregate state of an invoice's N-of-M co-signer approval.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ApprovalStatus {
    /// Fewer than `threshold` approvals, and the threshold is still reachable.
    Pending,
    /// At least `threshold` co-signers have approved; release is unblocked.
    Approved,
    /// Enough co-signers rejected that `threshold` can no longer be reached.
    Rejected,
}

/// N-of-M co-signer approval record attached to an escrow invoice.
///
/// While an approval record exists, the invoice can only be released once
/// `status` is [`ApprovalStatus::Approved`]. Each co-signer authenticates
/// with `require_auth`, may vote once (approve or reject), and may revoke
/// that vote until the invoice is released. `status` is recomputed after
/// every vote so it always reflects the current tallies.
#[contracttype]
#[derive(Clone, Debug)]
pub struct CosignerApproval {
    /// Addresses allowed to vote on the release.
    pub signers: Vec<Address>,
    /// Number of approvals required to release.
    pub threshold: u32,
    /// Co-signers that have approved.
    pub approvals: Vec<Address>,
    /// Co-signers that have rejected.
    pub rejections: Vec<Address>,
    /// Current aggregate status derived from the tallies.
    pub status: ApprovalStatus,
    /// Ledger sequence of the most recent change to this record.
    pub updated_ledger: u32,
}

// ──────────────────────────────────────────────────────────────────────
// Oracle condition guard types
// ──────────────────────────────────────────────────────────────────────

/// Price observation returned by an oracle's `lastprice`.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PriceData {
    /// Price in the oracle's fixed-point units. Must be positive.
    pub price: i128,
    /// Unix timestamp at which the price was observed.
    pub timestamp: u64,
}

/// How the observed price is compared against `threshold_price`.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum PriceComparator {
    /// Condition holds when `price >= threshold_price`.
    GreaterOrEqual,
    /// Condition holds when `price <= threshold_price`.
    LessOrEqual,
}

/// Which oracle produced a reading.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum OracleSource {
    Primary,
    Fallback,
}

/// Price condition that must hold before an invoice can be released.
///
/// The `oracle` is queried first. If it is unavailable, returns an invalid
/// price, or returns a price older than `max_staleness_secs`, the optional
/// `fallback_oracle` is queried instead. If neither yields a usable price the
/// release is blocked with the corresponding oracle error.
#[contracttype]
#[derive(Clone, Debug)]
pub struct OracleCondition {
    /// Primary oracle contract.
    pub oracle: Address,
    /// Secondary oracle consulted when the primary fails.
    pub fallback_oracle: Option<Address>,
    /// Asset whose price is checked.
    pub asset: Address,
    /// Comparison applied to the observed price.
    pub comparator: PriceComparator,
    /// Price the observation is compared against. Must be positive.
    pub threshold_price: i128,
    /// Maximum accepted age (seconds) of an oracle price.
    pub max_staleness_secs: u64,
}

/// Result of evaluating an [`OracleCondition`].
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct OracleReading {
    /// Price used for the evaluation.
    pub price: i128,
    /// Timestamp reported by the oracle for `price`.
    pub price_timestamp: u64,
    /// Oracle that supplied the price.
    pub source: OracleSource,
    /// Whether the condition held for `price`.
    pub condition_met: bool,
    /// Ledger timestamp at which the evaluation happened.
    pub checked_at: u64,
}

// ──────────────────────────────────────────────────────────────────────
// Scheduled release types
// ──────────────────────────────────────────────────────────────────────

/// Lifecycle state of a release schedule.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ScheduleStatus {
    /// Waiting for `release_at` / `unlock_ledger`; release is blocked until both pass.
    Scheduled,
    /// The invoice was released under this schedule.
    Executed,
    /// The creator cancelled the schedule; it no longer gates release.
    Cancelled,
}

/// Delayed-release schedule attached to an escrow invoice.
///
/// Release is blocked until **both** the ledger timestamp reaches
/// `release_at` and the ledger sequence reaches `unlock_ledger`. The ledger
/// bound gives a manipulation-resistant minimum delay that does not depend
/// on validator-reported close times.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ReleaseSchedule {
    /// Earliest Unix timestamp at which release is allowed.
    pub release_at: u64,
    /// Earliest ledger sequence at which release is allowed.
    pub unlock_ledger: u32,
    /// Ledger timestamp when the schedule was created.
    pub scheduled_at: u64,
    /// Ledger sequence when the schedule was created.
    pub scheduled_ledger: u32,
    /// Current schedule status.
    pub status: ScheduleStatus,
}

// ──────────────────────────────────────────────────────────────────────
// Refund grace period types
// ──────────────────────────────────────────────────────────────────────

/// Per-invoice refund grace configuration.
///
/// After an invoice's deadline passes, refunds stay blocked for
/// `grace_secs + extension_secs` more seconds, unless the admin waives the
/// grace period. This gives creators, co-signers and oracles time to clear
/// release gates before payers are refunded.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct RefundGrace {
    /// Base grace period in seconds (snapshotted from the default or set by the creator).
    pub grace_secs: u64,
    /// Additional seconds granted by admin extensions.
    pub extension_secs: u64,
    /// `true` once the admin waives the grace period; refunds are then
    /// allowed as soon as the deadline passes.
    pub waived: bool,
}

/// Where an invoice is in its refund grace timeline.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum RefundGraceStatus {
    /// No grace period applies; refunds open at the deadline.
    NoGrace,
    /// Deadline not yet reached; the grace timer has not started.
    NotStarted,
    /// Deadline passed and the grace timer is counting down.
    Active,
    /// Grace period is over; refunds can be processed.
    Elapsed,
    /// Admin waived the grace period; refunds can be processed after the deadline.
    Waived,
}
