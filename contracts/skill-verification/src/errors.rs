//! Error variants for the skill-verification contract.
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
    /// Caller is not authorised to verify this skill.
    NotVerifier = 4,
    /// The recipient already has a pending claim for this skill.
    ClaimAlreadyPending = 5,
    /// Claim ID was not found in storage.
    ClaimNotFound = 6,
    /// The claim has already been reviewed.
    ClaimNotPending = 7,
    /// Verifiers cannot verify their own claims.
    SelfVerification = 8,
    /// Level must be between 1 and `MAX_SKILL_LEVEL`.
    InvalidLevel = 9,
    /// Validity must be between `MIN_VALIDITY_LEDGERS` and `MAX_VALIDITY_LEDGERS`.
    InvalidValidity = 10,
    /// No attestation exists for this recipient and skill.
    AttestationNotFound = 11,
    /// The attestation has already been revoked.
    AlreadyRevoked = 12,
    /// The recipient list passed to a batch check is empty or too long.
    InvalidRecipientList = 13,
    /// Arithmetic overflow.
    Overflow = 14,
    /// Caller does not own the claim.
    Unauthorized = 15,
}
