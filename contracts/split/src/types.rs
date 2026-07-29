use soroban_sdk::{contracttype, Address, Vec};

// ---------------------------------------------------------------------------
// Invoice status
// ---------------------------------------------------------------------------

/// Status of an invoice lifecycle.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum InvoiceStatus {
    /// Invoice created, awaiting full payment.
    Pending,
    /// All shares paid; funds released to recipients.
    Released,
    /// Deadline passed before full funding; payers refunded.
    Refunded,
    /// Alias for Released used as the parent-finalisation gate (#522).
    /// An invoice is considered Finalised once it has been Released.
    Finalised,
}

// ---------------------------------------------------------------------------
// Payment
// ---------------------------------------------------------------------------

/// A single payment made toward an invoice.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Payment {
    /// Address of the payer.
    pub payer: Address,
    /// Amount paid in stroops (7 decimal places).
    pub amount: i128,
}

// ---------------------------------------------------------------------------
// Invoice
// ---------------------------------------------------------------------------

/// An on-chain invoice splitting payment among multiple recipients.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Invoice {
    /// Address that created the invoice.
    pub creator: Address,
    /// Ordered list of recipient addresses.
    pub recipients: Vec<Address>,
    /// Amounts owed to each recipient (parallel to `recipients`).
    pub amounts: Vec<i128>,
    /// USDC token contract address.
    pub token: Address,
    /// Unix timestamp after which unfunded invoices can be refunded.
    pub deadline: u64,
    /// Total amount collected so far.
    pub funded: i128,
    /// Current lifecycle status.
    pub status: InvoiceStatus,
    /// All payments made toward this invoice.
    pub payments: Vec<Payment>,

    // -----------------------------------------------------------------------
    // #522 — Cross-Invoice Split Linkage
    // -----------------------------------------------------------------------

    /// Optional parent invoice ID.
    ///
    /// When `Some(id)`, this child invoice's release is blocked until the
    /// parent invoice is in the `Released` / `Finalised` state.
    /// `None` means no dependency and the invoice releases normally.
    pub parent_invoice_id: Option<u64>,

    // -----------------------------------------------------------------------
    // #523 — Late Payment Penalty Fee
    // -----------------------------------------------------------------------

    /// Penalty applied to late contributions, expressed in basis points.
    ///
    /// A contribution is "late" when it arrives after the invoice `deadline`
    /// but within any grace window the application may enforce.  The penalty
    /// is charged on top of any platform fee and transferred to the treasury.
    ///
    /// `0` disables the penalty (late contributions are treated identically to
    /// on-time contributions).
    pub late_penalty_bps: u32,
}

// ---------------------------------------------------------------------------
// #524 — Batch Invoice Creation
// ---------------------------------------------------------------------------

/// Parameters for a single invoice inside a batch creation request.
///
/// Mirrors the arguments of `create_invoice` so that a `Vec<InvoiceParams>`
/// can be passed to `batch_create_invoices`.
#[contracttype]
#[derive(Clone, Debug)]
pub struct InvoiceParams {
    /// Address that owns the invoice (must authorise the batch call).
    pub creator: Address,
    /// Ordered list of recipient addresses.
    pub recipients: Vec<Address>,
    /// Amount owed to each recipient (parallel to `recipients`).
    pub amounts: Vec<i128>,
    /// USDC token contract address.
    pub token: Address,
    /// Unix timestamp after which unfunded invoices can be refunded.
    pub deadline: u64,
    /// Optional parent invoice ID (see `Invoice::parent_invoice_id`).
    pub parent_invoice_id: Option<u64>,
    /// Late-payment penalty in basis points (see `Invoice::late_penalty_bps`).
    pub late_penalty_bps: u32,
}

// ---------------------------------------------------------------------------
// Contract errors
// ---------------------------------------------------------------------------

/// Error codes returned by the contract.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ContractError {
    /// #522 — A child invoice cannot be released until the parent is finalised.
    ParentInvoiceNotFinalised,
    /// #522 — A circular parent reference was detected at creation time.
    CircularParentReference,
    /// #522 — The parent chain exceeds the maximum allowed depth.
    ParentChainTooDeep,
    /// #524 — The batch size exceeds the `MaxBatchSize` cap.
    BatchTooLarge,
}
