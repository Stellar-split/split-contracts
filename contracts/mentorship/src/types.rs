//! Shared types for the mentorship contract.

use soroban_sdk::{contracttype, Address, Symbol};

/// Lifecycle of a mentorship program.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramStatus {
    /// Mentee has asked; waiting for the mentor to accept or decline.
    Requested,
    /// Mentor accepted; the program term is running.
    Active,
    /// Program finished successfully.
    Completed,
    /// Program was cancelled by a participant before completion.
    Cancelled,
    /// Mentor declined the request.
    Declined,
}

/// On-chain profile of a creator offering mentorship.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MentorProfile {
    pub mentor: Address,
    /// Area of expertise (e.g. `design`, `music`, `dev`).
    pub expertise: Symbol,
    /// Share of mentee earnings paid to the mentor while a program is active.
    pub revenue_share_bps: u32,
    /// Maximum number of concurrently active mentees.
    pub max_mentees: u32,
    pub active_mentees: u32,
    pub completed_programs: u32,
    pub rating_sum: u32,
    pub rating_count: u32,
    /// Mentor-controlled availability toggle.
    pub available: bool,
    /// Admin-controlled suspension flag.
    pub suspended: bool,
    pub registered_at: u64,
}

/// A single mentor ↔ mentee engagement.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MentorshipProgram {
    pub id: u64,
    pub mentor: Address,
    pub mentee: Address,
    pub status: ProgramStatus,
    /// Revenue share locked in from the mentor profile at acceptance time.
    pub revenue_share_bps: u32,
    pub duration_ledgers: u32,
    pub requested_at: u64,
    /// Ledger sequence at which the program became active (0 until accepted).
    pub start_ledger: u32,
    /// Ledger sequence after which the term has ended (0 until accepted).
    pub end_ledger: u32,
    pub sessions: u32,
    /// Cumulative earnings share transferred to the mentor.
    pub total_shared: i128,
    pub rated: bool,
}
