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
