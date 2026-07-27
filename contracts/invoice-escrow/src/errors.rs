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
}
