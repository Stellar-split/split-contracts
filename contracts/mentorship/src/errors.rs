//! Error variants for the mentorship contract.
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
    /// The address is already registered as a mentor.
    MentorAlreadyRegistered = 4,
    /// No mentor profile exists for the address.
    MentorNotFound = 5,
    /// The mentor is inactive or suspended and cannot take new mentees.
    MentorUnavailable = 6,
    /// Revenue share exceeds `MAX_REVENUE_SHARE_BPS`.
    InvalidRevenueShare = 7,
    /// Mentee capacity must be between 1 and `MAX_MENTEES_PER_MENTOR`.
    InvalidCapacity = 8,
    /// The mentor has no free mentee slots.
    MentorAtCapacity = 9,
    /// A creator cannot mentor themselves.
    SelfMentorship = 10,
    /// An open (requested or active) program already exists for this pair.
    DuplicateProgram = 11,
    /// Program ID was not found in storage.
    ProgramNotFound = 12,
    /// The program is in a state that does not allow this operation.
    InvalidProgramState = 13,
    /// Caller is not a participant allowed to perform this operation.
    Unauthorized = 14,
    /// Duration must be between `MIN_DURATION_LEDGERS` and `MAX_DURATION_LEDGERS`.
    InvalidDuration = 15,
    /// Amount must be strictly positive.
    InvalidAmount = 16,
    /// The program term has ended; earnings can no longer be shared.
    ProgramExpired = 17,
    /// The mentor tried to complete the program before its term ended.
    ProgramNotMatured = 18,
    /// Rating must be between 1 and 5.
    InvalidRating = 19,
    /// The mentee has already rated this program.
    AlreadyRated = 20,
    /// Arithmetic overflow.
    Overflow = 21,
}
