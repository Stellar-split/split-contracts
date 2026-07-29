use soroban_sdk::{contracttype, Address, Vec};

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
}

/// Category of a token transfer recorded in the audit log.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum TransferKind {
    /// A payer contributing funds toward an invoice.
    Contribution,
    /// Funds released to a recipient.
    Payout,
    /// Funds refunded to a payer.
    Refund,
    /// A fee charged by the contract.
    Fee,
    /// Sweep of remaining funds to a designated address.
    Sweep,
}

/// A single token transfer event recorded on-chain.
#[contracttype]
#[derive(Clone, Debug)]
pub struct TransferRecord {
    /// Source of the transfer.
    pub from: Address,
    /// Destination of the transfer.
    pub to: Address,
    /// Amount transferred in stroops.
    pub amount: i128,
    /// Category of the transfer.
    pub kind: TransferKind,
    /// Ledger sequence at the time of the transfer.
    pub ledger: u32,
}

/// A single payment made toward an invoice.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Payment {
    /// Address of the payer.
    pub payer: Address,
    /// Amount paid in stroops (7 decimal places).
    pub amount: i128,
}

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
    /// Ledger sequence after which the invoice can be refunded.
    pub deadline_ledger: u32,
    /// Total amount collected so far.
    pub funded: i128,
    /// Current lifecycle status.
    pub status: InvoiceStatus,
    /// All payments made toward this invoice.
    pub payments: Vec<Payment>,
}