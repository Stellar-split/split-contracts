//! StellarSplit Skill Verification — on-chain attestations of recipient skills.
//!
//! Split recipients submit skill claims backed by an evidence hash. Verifiers
//! appointed by the admin for a specific skill approve claims at a
//! proficiency level with an expiry, or reject them. Approved attestations
//! can be revoked by the issuing verifier or the admin.
//!
//! Other contracts (e.g. the split contract) can gate payouts with
//! [`SkillVerificationContract::meets_requirement`] or check a whole
//! recipient list with [`SkillVerificationContract::all_meet_requirement`].
//! Every state change emits an event under the `skill` topic.

#![no_std]

mod errors;
mod types;

pub use errors::Error;
pub use types::{ClaimStatus, SkillAttestation, SkillClaim};

use soroban_sdk::{contract, contractimpl, symbol_short, Address, BytesN, Env, Symbol, Vec};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const MAX_SKILL_LEVEL: u32 = 5;
/// Roughly one week at 5s ledgers.
pub const MIN_VALIDITY_LEDGERS: u32 = 120_960;
/// Roughly two years at 5s ledgers.
pub const MAX_VALIDITY_LEDGERS: u32 = 12_614_400;
/// Upper bound for batch recipient checks.
pub const MAX_BATCH_RECIPIENTS: u32 = 50;

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

fn admin_key() -> Symbol {
    symbol_short!("admin")
}

fn counter_key() -> Symbol {
    symbol_short!("claim_ctr")
}

fn verifier_key(verifier: &Address, skill: &Symbol) -> (Symbol, Address, Symbol) {
    (symbol_short!("verifier"), verifier.clone(), skill.clone())
}

fn claim_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("claim"), id)
}

fn pending_claim_key(recipient: &Address, skill: &Symbol) -> (Symbol, Address, Symbol) {
    (symbol_short!("pend_clm"), recipient.clone(), skill.clone())
}

fn attestation_key(recipient: &Address, skill: &Symbol) -> (Symbol, Address, Symbol) {
    (symbol_short!("attest"), recipient.clone(), skill.clone())
}

fn recipient_skills_key(recipient: &Address) -> (Symbol, Address) {
    (symbol_short!("rcp_skls"), recipient.clone())
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

fn emit_verifier_set(env: &Env, verifier: &Address, skill: &Symbol, authorized: bool) {
    env.events().publish(
        (symbol_short!("skill"), symbol_short!("verifier"), verifier.clone()),
        (skill.clone(), authorized),
    );
}

fn emit_claim_submitted(env: &Env, claim: &SkillClaim) {
    env.events().publish(
        (symbol_short!("skill"), symbol_short!("claimed"), claim.id),
        (claim.recipient.clone(), claim.skill.clone(), claim.evidence_hash.clone()),
    );
}

fn emit_claim_withdrawn(env: &Env, claim: &SkillClaim) {
    env.events().publish(
        (symbol_short!("skill"), symbol_short!("withdrawn"), claim.id),
        (claim.recipient.clone(), claim.skill.clone()),
    );
}

fn emit_skill_verified(env: &Env, att: &SkillAttestation) {
    env.events().publish(
        (symbol_short!("skill"), symbol_short!("verified"), att.recipient.clone()),
        (
            att.skill.clone(),
            att.verifier.clone(),
            att.level,
            att.expires_at_ledger,
            att.claim_id,
        ),
    );
}

fn emit_claim_rejected(env: &Env, claim: &SkillClaim, verifier: &Address, reason_hash: &BytesN<32>) {
    env.events().publish(
        (symbol_short!("skill"), symbol_short!("rejected"), claim.id),
        (claim.recipient.clone(), verifier.clone(), reason_hash.clone()),
    );
}

fn emit_skill_revoked(env: &Env, recipient: &Address, skill: &Symbol, by: &Address) {
    env.events().publish(
        (symbol_short!("skill"), symbol_short!("revoked"), recipient.clone()),
        (skill.clone(), by.clone()),
    );
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn get_admin(env: &Env) -> Result<Address, Error> {
    env.storage()
        .instance()
        .get(&admin_key())
        .ok_or(Error::NotInitialized)
}

fn require_admin(env: &Env, admin: &Address) -> Result<(), Error> {
    if get_admin(env)? != *admin {
        return Err(Error::NotAdmin);
    }
    admin.require_auth();
    Ok(())
}

fn is_verifier_for(env: &Env, verifier: &Address, skill: &Symbol) -> bool {
    env.storage()
        .persistent()
        .get(&verifier_key(verifier, skill))
        .unwrap_or(false)
}

fn load_claim(env: &Env, id: u64) -> Result<SkillClaim, Error> {
    env.storage()
        .persistent()
        .get(&claim_key(id))
        .ok_or(Error::ClaimNotFound)
}

fn load_pending_claim(env: &Env, verifier: &Address, claim_id: u64) -> Result<SkillClaim, Error> {
    let claim = load_claim(env, claim_id)?;
    if claim.status != ClaimStatus::Pending {
        return Err(Error::ClaimNotPending);
    }
    if !is_verifier_for(env, verifier, &claim.skill) {
        return Err(Error::NotVerifier);
    }
    if claim.recipient == *verifier {
        return Err(Error::SelfVerification);
    }
    Ok(claim)
}

fn close_claim(env: &Env, claim: &SkillClaim) {
    env.storage().persistent().set(&claim_key(claim.id), claim);
    env.storage()
        .persistent()
        .remove(&pending_claim_key(&claim.recipient, &claim.skill));
}

/// Returns the attestation only when it is neither revoked nor expired.
fn valid_attestation(env: &Env, recipient: &Address, skill: &Symbol) -> Option<SkillAttestation> {
    let att: SkillAttestation = env
        .storage()
        .persistent()
        .get(&attestation_key(recipient, skill))?;
    if att.revoked || env.ledger().sequence() > att.expires_at_ledger {
        return None;
    }
    Some(att)
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct SkillVerificationContract;

#[contractimpl]
impl SkillVerificationContract {
    /// Initialise the contract with an admin who appoints verifiers.
    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&admin_key()) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&admin_key(), &admin);
        env.storage().instance().set(&counter_key(), &0u64);
        Ok(())
    }

    /// Admin-only: authorise or de-authorise `verifier` for `skill`.
    pub fn set_verifier(
        env: Env,
        admin: Address,
        verifier: Address,
        skill: Symbol,
        authorized: bool,
    ) -> Result<(), Error> {
        require_admin(&env, &admin)?;
        let key = verifier_key(&verifier, &skill);
        if authorized {
            env.storage().persistent().set(&key, &true);
        } else {
            env.storage().persistent().remove(&key);
        }
        emit_verifier_set(&env, &verifier, &skill, authorized);
        Ok(())
    }

    /// Recipient submits a claim for `skill` with a hash of supporting evidence.
    /// Only one pending claim per (recipient, skill) is allowed. Returns the claim ID.
    pub fn submit_claim(
        env: Env,
        recipient: Address,
        skill: Symbol,
        evidence_hash: BytesN<32>,
    ) -> Result<u64, Error> {
        recipient.require_auth();
        let pending_key = pending_claim_key(&recipient, &skill);
        if env.storage().persistent().has(&pending_key) {
            return Err(Error::ClaimAlreadyPending);
        }
        let id: u64 = env
            .storage()
            .instance()
            .get(&counter_key())
            .ok_or(Error::NotInitialized)?;
        let next = id.checked_add(1).ok_or(Error::Overflow)?;
        env.storage().instance().set(&counter_key(), &next);

        let claim = SkillClaim {
            id,
            recipient: recipient.clone(),
            skill,
            evidence_hash,
            status: ClaimStatus::Pending,
            submitted_at: env.ledger().timestamp(),
            reviewer: None,
            reason_hash: None,
        };
        env.storage().persistent().set(&claim_key(id), &claim);
        env.storage().persistent().set(&pending_key, &id);
        emit_claim_submitted(&env, &claim);
        Ok(id)
    }

    /// Recipient withdraws their own pending claim.
    pub fn withdraw_claim(env: Env, recipient: Address, claim_id: u64) -> Result<(), Error> {
        recipient.require_auth();
        let mut claim = load_claim(&env, claim_id)?;
        if claim.recipient != recipient {
            return Err(Error::Unauthorized);
        }
        if claim.status != ClaimStatus::Pending {
            return Err(Error::ClaimNotPending);
        }
        claim.status = ClaimStatus::Withdrawn;
        close_claim(&env, &claim);
        emit_claim_withdrawn(&env, &claim);
        Ok(())
    }

    /// Verifier approves a pending claim at `level`, valid for
    /// `validity_ledgers` from now. Replaces any earlier attestation for the
    /// same skill.
    pub fn approve_claim(
        env: Env,
        verifier: Address,
        claim_id: u64,
        level: u32,
        validity_ledgers: u32,
    ) -> Result<(), Error> {
        verifier.require_auth();
        if level == 0 || level > MAX_SKILL_LEVEL {
            return Err(Error::InvalidLevel);
        }
        if !(MIN_VALIDITY_LEDGERS..=MAX_VALIDITY_LEDGERS).contains(&validity_ledgers) {
            return Err(Error::InvalidValidity);
        }
        let mut claim = load_pending_claim(&env, &verifier, claim_id)?;

        let expires_at_ledger = env
            .ledger()
            .sequence()
            .checked_add(validity_ledgers)
            .ok_or(Error::Overflow)?;
        let att = SkillAttestation {
            recipient: claim.recipient.clone(),
            skill: claim.skill.clone(),
            verifier: verifier.clone(),
            level,
            evidence_hash: claim.evidence_hash.clone(),
            claim_id,
            verified_at: env.ledger().timestamp(),
            expires_at_ledger,
            revoked: false,
        };
        let att_key = attestation_key(&claim.recipient, &claim.skill);
        if !env.storage().persistent().has(&att_key) {
            let skills_key = recipient_skills_key(&claim.recipient);
            let mut skills: Vec<Symbol> = env
                .storage()
                .persistent()
                .get(&skills_key)
                .unwrap_or_else(|| Vec::new(&env));
            skills.push_back(claim.skill.clone());
            env.storage().persistent().set(&skills_key, &skills);
        }
        env.storage().persistent().set(&att_key, &att);

        claim.status = ClaimStatus::Approved;
        claim.reviewer = Some(verifier);
        close_claim(&env, &claim);
        emit_skill_verified(&env, &att);
        Ok(())
    }

    /// Verifier rejects a pending claim, recording a hash of the reason.
    pub fn reject_claim(
        env: Env,
        verifier: Address,
        claim_id: u64,
        reason_hash: BytesN<32>,
    ) -> Result<(), Error> {
        verifier.require_auth();
        let mut claim = load_pending_claim(&env, &verifier, claim_id)?;
        claim.status = ClaimStatus::Rejected;
        claim.reviewer = Some(verifier.clone());
        claim.reason_hash = Some(reason_hash.clone());
        close_claim(&env, &claim);
        emit_claim_rejected(&env, &claim, &verifier, &reason_hash);
        Ok(())
    }

    /// Revoke a recipient's attestation. Callable by the issuing verifier or the admin.
    pub fn revoke(env: Env, caller: Address, recipient: Address, skill: Symbol) -> Result<(), Error> {
        caller.require_auth();
        let key = attestation_key(&recipient, &skill);
        let mut att: SkillAttestation = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::AttestationNotFound)?;
        if caller != att.verifier && caller != get_admin(&env)? {
            return Err(Error::NotVerifier);
        }
        if att.revoked {
            return Err(Error::AlreadyRevoked);
        }
        att.revoked = true;
        env.storage().persistent().set(&key, &att);
        emit_skill_revoked(&env, &recipient, &skill, &caller);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Views
    // -----------------------------------------------------------------------

    pub fn is_verifier(env: Env, verifier: Address, skill: Symbol) -> bool {
        is_verifier_for(&env, &verifier, &skill)
    }

    pub fn get_claim(env: Env, claim_id: u64) -> Option<SkillClaim> {
        env.storage().persistent().get(&claim_key(claim_id))
    }

    /// Raw attestation, including revoked or expired ones.
    pub fn get_attestation(env: Env, recipient: Address, skill: Symbol) -> Option<SkillAttestation> {
        env.storage().persistent().get(&attestation_key(&recipient, &skill))
    }

    /// Every skill the recipient has ever been verified for (valid or not).
    pub fn get_recipient_skills(env: Env, recipient: Address) -> Vec<Symbol> {
        env.storage()
            .persistent()
            .get(&recipient_skills_key(&recipient))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// True when the recipient holds a valid (unrevoked, unexpired) attestation.
    pub fn is_skill_verified(env: Env, recipient: Address, skill: Symbol) -> bool {
        valid_attestation(&env, &recipient, &skill).is_some()
    }

    /// Current valid level for the skill, or 0 if not verified.
    pub fn get_skill_level(env: Env, recipient: Address, skill: Symbol) -> u32 {
        valid_attestation(&env, &recipient, &skill).map_or(0, |a| a.level)
    }

    /// True when the recipient holds `skill` at `min_level` or above.
    pub fn meets_requirement(env: Env, recipient: Address, skill: Symbol, min_level: u32) -> bool {
        valid_attestation(&env, &recipient, &skill).is_some_and(|a| a.level >= min_level)
    }

    /// True when every recipient holds `skill` at `min_level` or above.
    /// Intended for gating split recipient lists before invoice creation.
    pub fn all_meet_requirement(
        env: Env,
        recipients: Vec<Address>,
        skill: Symbol,
        min_level: u32,
    ) -> Result<bool, Error> {
        if recipients.is_empty() || recipients.len() > MAX_BATCH_RECIPIENTS {
            return Err(Error::InvalidRecipientList);
        }
        for recipient in recipients.iter() {
            let ok = valid_attestation(&env, &recipient, &skill).is_some_and(|a| a.level >= min_level);
            if !ok {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod test;
