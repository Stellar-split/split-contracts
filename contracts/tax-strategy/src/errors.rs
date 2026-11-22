//! Error variants for the tax-strategy contract.
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
    /// A rate exceeds `MAX_RATE_BPS`.
    InvalidRate = 4,
    /// Brackets are empty, too many, unsorted or do not start at zero.
    InvalidBrackets = 5,
    /// Deduction share exceeds `MAX_DEDUCTION_BPS`, or a non-zero share has no wallet.
    InvalidDeduction = 6,
    /// Period must be between `MIN_PERIOD_LEDGERS` and `MAX_PERIOD_LEDGERS`.
    InvalidPeriod = 7,
    /// The named template does not exist.
    TemplateNotFound = 8,
    /// A template strategy was configured without a template name.
    MissingTemplate = 9,
    /// The creator has no strategy configured.
    StrategyNotFound = 10,
    /// Amount must be strictly positive.
    InvalidAmount = 11,
    /// Arithmetic overflow.
    Overflow = 12,
}
