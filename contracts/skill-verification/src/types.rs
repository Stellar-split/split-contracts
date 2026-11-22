//! Shared types for the skill-verification contract.

use soroban_sdk::{contracttype, Address, BytesN, Symbol};

/// Review state of a skill claim.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimStatus {
    Pending,
    Approved,
    Rejected,
    Withdrawn,
}

/// A recipient's request to have a skill verified.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkillClaim {
    pub id: u64,
    pub recipient: Address,
    pub skill: Symbol,
    /// Hash of off-chain evidence (portfolio, certificate, etc.).
    pub evidence_hash: BytesN<32>,
    pub status: ClaimStatus,
    pub submitted_at: u64,
    /// Verifier that reviewed the claim, once reviewed.
    pub reviewer: Option<Address>,
    /// Hash of the reviewer's rejection reason, if rejected.
    pub reason_hash: Option<BytesN<32>>,
}

/// A verifier's attestation that a recipient holds a skill at a given level.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkillAttestation {
    pub recipient: Address,
    pub skill: Symbol,
    pub verifier: Address,
    /// Proficiency level, 1 (basic) to `MAX_SKILL_LEVEL` (expert).
    pub level: u32,
    pub evidence_hash: BytesN<32>,
    pub claim_id: u64,
    pub verified_at: u64,
    /// Ledger sequence after which the attestation is no longer valid.
    pub expires_at_ledger: u32,
    pub revoked: bool,
}
