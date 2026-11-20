//! Type definitions for the invoice-escrow contract.

use soroban_sdk::{contracttype, Address, BytesN};

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
    /// A stakeholder has raised a dispute; deposits/release are frozen
    /// pending admin arbitration or automatic timeout resolution.
    Disputed,
}

// ──────────────────────────────────────────────────────────────────────
// Dispute resolution automation
// ──────────────────────────────────────────────────────────────────────

/// Record of an active (or most recently resolved) dispute on an invoice.
///
/// Created by [`crate::InvoiceEscrowContract::raise_dispute`]. The admin may
/// arbitrate manually at any time via
/// [`crate::InvoiceEscrowContract::arbitrate_dispute`]. If the admin does not
/// act within `timeout_secs` of `raised_at`, anyone may call
/// [`crate::InvoiceEscrowContract::auto_resolve_dispute`] to automatically
/// refund depositors — protecting the disputing party from an unresponsive
/// creator/admin by default.
#[contracttype]
#[derive(Clone, Debug)]
pub struct DisputeRecord {
    /// The invoice this dispute belongs to.
    pub invoice_id: u64,
    /// Address that raised the dispute (must have a deposit on the invoice).
    pub raised_by: Address,
    /// Commitment hash of the dispute reason/evidence (revealed off-chain).
    pub reason_hash: BytesN<32>,
    /// Unix timestamp the dispute was raised at.
    pub raised_at: u64,
    /// Seconds after `raised_at` before automatic resolution becomes callable.
    pub timeout_secs: u64,
    /// Whether the dispute has already been resolved (manually or automatically).
    pub resolved: bool,
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
