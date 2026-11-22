//! Error variants for the charity-split contract.
//!
//! Discriminants are stable — never reorder, only append.

use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    /// Contract has already been initialized.
    AlreadyInitialized = 1,
    /// Contract has not been initialized yet.
    NotInitialized = 2,
    /// Caller is not the contract admin.
    NotAdmin = 3,
    /// The charity is already registered.
    CharityAlreadyRegistered = 4,
    /// No charity is registered at this address.
    CharityNotFound = 5,
    /// The charity has been deactivated by the admin.
    CharityInactive = 6,
    /// Donation share must be between `MIN_DONATION_BPS` and `MAX_DONATION_BPS`.
    InvalidDonationBps = 7,
    /// Recipient and amount lists are empty, mismatched or too long.
    InvalidRecipients = 8,
    /// Amount must be strictly positive.
    InvalidAmount = 9,
    /// Recipient amounts do not sum to the invoice total.
    TotalMismatch = 10,
    /// Invoice ID was not found in storage.
    InvoiceNotFound = 11,
    /// The invoice is not open for this operation.
    InvoiceNotOpen = 12,
    /// The payment would exceed the invoice total.
    OverFunded = 13,
    /// Caller is not the invoice creator.
    NotCreator = 14,
    /// The invoice is not cancelled, so refunds are unavailable.
    NotCancelled = 15,
    /// The payer has nothing to refund.
    NothingToRefund = 16,
    /// The charity cannot also be a split recipient.
    CharityIsRecipient = 17,
    /// Arithmetic overflow.
    Overflow = 18,
}
