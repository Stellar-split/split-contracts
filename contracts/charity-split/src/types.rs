//! Shared types for the charity-split contract.

use soroban_sdk::{contracttype, Address, Symbol, Vec};

/// An admin-vetted charity that invoices may donate to.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Charity {
    pub address: Address,
    pub name: Symbol,
    pub active: bool,
    pub registered_at: u64,
    /// Number of invoices that have released a donation to this charity.
    pub donation_count: u32,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CharityInvoiceStatus {
    /// Accepting payments.
    Open,
    /// Fully funded and distributed to recipients and the charity.
    Released,
    /// Cancelled by the creator; payers may claim refunds.
    Cancelled,
}

/// An invoice whose payout donates a fixed share to a charity.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CharityInvoice {
    pub id: u64,
    pub creator: Address,
    pub token: Address,
    pub total: i128,
    pub funded: i128,
    pub recipients: Vec<Address>,
    /// Gross amounts per recipient, before the donation is deducted.
    pub amounts: Vec<i128>,
    pub charity: Address,
    /// Share of every recipient's gross amount donated to the charity.
    pub donation_bps: u32,
    /// Amount sent to the charity at release (0 until released).
    pub donated: i128,
    pub status: CharityInvoiceStatus,
    pub created_at: u64,
}

/// Result of applying the donation to an invoice's recipient amounts.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DonationSplit {
    /// Net payout per recipient, in the same order as `recipients`.
    pub payouts: Vec<i128>,
    /// Amount going to the charity (absorbs rounding remainders).
    pub donation: i128,
}
