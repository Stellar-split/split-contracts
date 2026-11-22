#![cfg(test)]

extern crate std;

use crate::{
    ClaimStatus, Error, SkillVerificationContract, SkillVerificationContractClient,
    MAX_BATCH_RECIPIENTS, MIN_VALIDITY_LEDGERS,
};
use soroban_sdk::testutils::{Address as _, Events as _, Ledger};
use soroban_sdk::{symbol_short, vec, Address, BytesN, Env, Symbol, TryFromVal, Val, Vec};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct Setup {
    env: Env,
    client: SkillVerificationContractClient<'static>,
    admin: Address,
    verifier: Address,
}

fn skill() -> Symbol {
    symbol_short!("rust")
}

fn evidence(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[7u8; 32])
}

fn setup() -> Setup {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(SkillVerificationContract, ());
    let client = SkillVerificationContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let verifier = Address::generate(&env);
    client.initialize(&admin);
    client.set_verifier(&admin, &verifier, &skill(), &true);
    Setup { env, client, admin, verifier }
}

/// Submits and approves a claim for `skill()` at `level`; returns the recipient.
fn verified_recipient(s: &Setup, level: u32) -> Address {
    let recipient = Address::generate(&s.env);
    let id = s.client.submit_claim(&recipient, &skill(), &evidence(&s.env));
    s.client.approve_claim(&s.verifier, &id, &level, &MIN_VALIDITY_LEDGERS);
    recipient
}

/// Returns true if an event with topics `(skill, action, ..)` was emitted.
fn has_event(env: &Env, action: Symbol) -> bool {
    for event in env.events().all().events().iter() {
        let soroban_sdk::xdr::ContractEventBody::V0(body) = &event.body;
        let topics: &[soroban_sdk::xdr::ScVal] = body.topics.as_ref();
        if topics.len() < 2 {
            continue;
        }
        let t0 = Val::try_from_val(env, &topics[0]).ok();
        let t1 = Val::try_from_val(env, &topics[1]).ok();
        if let (Some(t0), Some(t1)) = (t0, t1) {
            let s0 = Symbol::try_from_val(env, &t0).ok();
            let s1 = Symbol::try_from_val(env, &t1).ok();
            if s0 == Some(symbol_short!("skill")) && s1 == Some(action.clone()) {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Admin & verifiers
// ---------------------------------------------------------------------------

#[test]
fn test_initialize_twice_fails() {
    let s = setup();
    assert_eq!(s.client.try_initialize(&s.admin), Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn test_set_verifier_emits_event() {
    let s = setup();
    let v = Address::generate(&s.env);
    s.client.set_verifier(&s.admin, &v, &symbol_short!("audio"), &true);
    assert!(has_event(&s.env, symbol_short!("verifier")));
    assert!(s.client.is_verifier(&v, &symbol_short!("audio")));
    assert!(!s.client.is_verifier(&v, &skill()));
}

#[test]
fn test_remove_verifier() {
    let s = setup();
    s.client.set_verifier(&s.admin, &s.verifier, &skill(), &false);
    assert!(!s.client.is_verifier(&s.verifier, &skill()));
}

#[test]
fn test_non_admin_cannot_set_verifier() {
    let s = setup();
    let rando = Address::generate(&s.env);
    assert_eq!(
        s.client.try_set_verifier(&rando, &rando, &skill(), &true),
        Err(Ok(Error::NotAdmin))
    );
}

// ---------------------------------------------------------------------------
// Claims
// ---------------------------------------------------------------------------

#[test]
fn test_submit_claim_success() {
    let s = setup();
    let recipient = Address::generate(&s.env);
    let id = s.client.submit_claim(&recipient, &skill(), &evidence(&s.env));
    assert_eq!(id, 0);
    assert!(has_event(&s.env, symbol_short!("claimed")));

    let claim = s.client.get_claim(&id).unwrap();
    assert_eq!(claim.status, ClaimStatus::Pending);
    assert_eq!(claim.recipient, recipient);
    assert_eq!(claim.evidence_hash, evidence(&s.env));
}

#[test]
fn test_duplicate_pending_claim_fails() {
    let s = setup();
    let recipient = Address::generate(&s.env);
    s.client.submit_claim(&recipient, &skill(), &evidence(&s.env));
    assert_eq!(
        s.client.try_submit_claim(&recipient, &skill(), &evidence(&s.env)),
        Err(Ok(Error::ClaimAlreadyPending))
    );
}

#[test]
fn test_withdraw_claim_allows_resubmission() {
    let s = setup();
    let recipient = Address::generate(&s.env);
    let id = s.client.submit_claim(&recipient, &skill(), &evidence(&s.env));
    s.client.withdraw_claim(&recipient, &id);
    assert!(has_event(&s.env, symbol_short!("withdrawn")));
    assert_eq!(s.client.get_claim(&id).unwrap().status, ClaimStatus::Withdrawn);

    let id2 = s.client.submit_claim(&recipient, &skill(), &evidence(&s.env));
    assert_eq!(id2, id + 1);
}

#[test]
fn test_withdraw_others_claim_fails() {
    let s = setup();
    let recipient = Address::generate(&s.env);
    let id = s.client.submit_claim(&recipient, &skill(), &evidence(&s.env));
    let rando = Address::generate(&s.env);
    assert_eq!(s.client.try_withdraw_claim(&rando, &id), Err(Ok(Error::Unauthorized)));
}

// ---------------------------------------------------------------------------
// Approval / rejection
// ---------------------------------------------------------------------------

#[test]
fn test_approve_claim_creates_attestation() {
    let s = setup();
    let recipient = Address::generate(&s.env);
    let id = s.client.submit_claim(&recipient, &skill(), &evidence(&s.env));
    s.client.approve_claim(&s.verifier, &id, &3, &MIN_VALIDITY_LEDGERS);
    assert!(has_event(&s.env, symbol_short!("verified")));

    let claim = s.client.get_claim(&id).unwrap();
    assert_eq!(claim.status, ClaimStatus::Approved);
    assert_eq!(claim.reviewer, Some(s.verifier.clone()));

    let att = s.client.get_attestation(&recipient, &skill()).unwrap();
    assert_eq!(att.level, 3);
    assert_eq!(att.verifier, s.verifier);
    assert_eq!(att.claim_id, id);
    assert!(s.client.is_skill_verified(&recipient, &skill()));
    assert_eq!(s.client.get_skill_level(&recipient, &skill()), 3);
    assert_eq!(s.client.get_recipient_skills(&recipient), vec![&s.env, skill()]);
}

#[test]
fn test_approve_by_unauthorized_verifier_fails() {
    let s = setup();
    let recipient = Address::generate(&s.env);
    let id = s.client.submit_claim(&recipient, &skill(), &evidence(&s.env));
    let rando = Address::generate(&s.env);
    assert_eq!(
        s.client.try_approve_claim(&rando, &id, &3, &MIN_VALIDITY_LEDGERS),
        Err(Ok(Error::NotVerifier))
    );
}

#[test]
fn test_verifier_scoped_to_skill() {
    let s = setup();
    let recipient = Address::generate(&s.env);
    let id = s.client.submit_claim(&recipient, &symbol_short!("audio"), &evidence(&s.env));
    assert_eq!(
        s.client.try_approve_claim(&s.verifier, &id, &3, &MIN_VALIDITY_LEDGERS),
        Err(Ok(Error::NotVerifier))
    );
}

#[test]
fn test_self_verification_fails() {
    let s = setup();
    let id = s.client.submit_claim(&s.verifier, &skill(), &evidence(&s.env));
    assert_eq!(
        s.client.try_approve_claim(&s.verifier, &id, &3, &MIN_VALIDITY_LEDGERS),
        Err(Ok(Error::SelfVerification))
    );
}

#[test]
fn test_approve_invalid_level_fails() {
    let s = setup();
    let recipient = Address::generate(&s.env);
    let id = s.client.submit_claim(&recipient, &skill(), &evidence(&s.env));
    assert_eq!(
        s.client.try_approve_claim(&s.verifier, &id, &0, &MIN_VALIDITY_LEDGERS),
        Err(Ok(Error::InvalidLevel))
    );
    assert_eq!(
        s.client.try_approve_claim(&s.verifier, &id, &6, &MIN_VALIDITY_LEDGERS),
        Err(Ok(Error::InvalidLevel))
    );
}

#[test]
fn test_approve_invalid_validity_fails() {
    let s = setup();
    let recipient = Address::generate(&s.env);
    let id = s.client.submit_claim(&recipient, &skill(), &evidence(&s.env));
    assert_eq!(
        s.client.try_approve_claim(&s.verifier, &id, &3, &(MIN_VALIDITY_LEDGERS - 1)),
        Err(Ok(Error::InvalidValidity))
    );
}

#[test]
fn test_approve_twice_fails() {
    let s = setup();
    let recipient = Address::generate(&s.env);
    let id = s.client.submit_claim(&recipient, &skill(), &evidence(&s.env));
    s.client.approve_claim(&s.verifier, &id, &3, &MIN_VALIDITY_LEDGERS);
    assert_eq!(
        s.client.try_approve_claim(&s.verifier, &id, &3, &MIN_VALIDITY_LEDGERS),
        Err(Ok(Error::ClaimNotPending))
    );
}

#[test]
fn test_reject_claim() {
    let s = setup();
    let recipient = Address::generate(&s.env);
    let id = s.client.submit_claim(&recipient, &skill(), &evidence(&s.env));
    let reason = BytesN::from_array(&s.env, &[9u8; 32]);
    s.client.reject_claim(&s.verifier, &id, &reason);
    assert!(has_event(&s.env, symbol_short!("rejected")));

    let claim = s.client.get_claim(&id).unwrap();
    assert_eq!(claim.status, ClaimStatus::Rejected);
    assert_eq!(claim.reason_hash, Some(reason));
    assert!(!s.client.is_skill_verified(&recipient, &skill()));
}

#[test]
fn test_reverification_upgrades_level_without_duplicate_skill_entry() {
    let s = setup();
    let recipient = verified_recipient(&s, 2);
    let id = s.client.submit_claim(&recipient, &skill(), &evidence(&s.env));
    s.client.approve_claim(&s.verifier, &id, &5, &MIN_VALIDITY_LEDGERS);
    assert_eq!(s.client.get_skill_level(&recipient, &skill()), 5);
    assert_eq!(s.client.get_recipient_skills(&recipient).len(), 1);
}

// ---------------------------------------------------------------------------
// Expiry & revocation
// ---------------------------------------------------------------------------

#[test]
fn test_attestation_expires() {
    let s = setup();
    let recipient = verified_recipient(&s, 3);
    let expires = s.client.get_attestation(&recipient, &skill()).unwrap().expires_at_ledger;

    s.env.ledger().with_mut(|l| l.sequence_number = expires);
    assert!(s.client.is_skill_verified(&recipient, &skill()));

    s.env.ledger().with_mut(|l| l.sequence_number = expires + 1);
    assert!(!s.client.is_skill_verified(&recipient, &skill()));
    assert_eq!(s.client.get_skill_level(&recipient, &skill()), 0);
}

#[test]
fn test_verifier_revokes_attestation() {
    let s = setup();
    let recipient = verified_recipient(&s, 3);
    s.client.revoke(&s.verifier, &recipient, &skill());
    assert!(has_event(&s.env, symbol_short!("revoked")));
    assert!(!s.client.is_skill_verified(&recipient, &skill()));
    assert!(s.client.get_attestation(&recipient, &skill()).unwrap().revoked);
}

#[test]
fn test_admin_revokes_attestation() {
    let s = setup();
    let recipient = verified_recipient(&s, 3);
    s.client.revoke(&s.admin, &recipient, &skill());
    assert!(!s.client.is_skill_verified(&recipient, &skill()));
}

#[test]
fn test_outsider_cannot_revoke() {
    let s = setup();
    let recipient = verified_recipient(&s, 3);
    let rando = Address::generate(&s.env);
    assert_eq!(
        s.client.try_revoke(&rando, &recipient, &skill()),
        Err(Ok(Error::NotVerifier))
    );
}

#[test]
fn test_revoke_twice_fails() {
    let s = setup();
    let recipient = verified_recipient(&s, 3);
    s.client.revoke(&s.verifier, &recipient, &skill());
    assert_eq!(
        s.client.try_revoke(&s.verifier, &recipient, &skill()),
        Err(Ok(Error::AlreadyRevoked))
    );
}

#[test]
fn test_revoke_missing_attestation_fails() {
    let s = setup();
    let recipient = Address::generate(&s.env);
    assert_eq!(
        s.client.try_revoke(&s.admin, &recipient, &skill()),
        Err(Ok(Error::AttestationNotFound))
    );
}

// ---------------------------------------------------------------------------
// Requirement checks
// ---------------------------------------------------------------------------

#[test]
fn test_meets_requirement_by_level() {
    let s = setup();
    let recipient = verified_recipient(&s, 3);
    assert!(s.client.meets_requirement(&recipient, &skill(), &1));
    assert!(s.client.meets_requirement(&recipient, &skill(), &3));
    assert!(!s.client.meets_requirement(&recipient, &skill(), &4));
    assert!(!s.client.meets_requirement(&recipient, &symbol_short!("audio"), &1));
}

#[test]
fn test_all_meet_requirement() {
    let s = setup();
    let a = verified_recipient(&s, 4);
    let b = verified_recipient(&s, 2);
    assert!(s.client.all_meet_requirement(&vec![&s.env, a.clone(), b.clone()], &skill(), &2));
    assert!(!s.client.all_meet_requirement(&vec![&s.env, a, b], &skill(), &3));
}

#[test]
fn test_all_meet_requirement_rejects_bad_lists() {
    let s = setup();
    let empty: Vec<Address> = Vec::new(&s.env);
    assert_eq!(
        s.client.try_all_meet_requirement(&empty, &skill(), &1),
        Err(Ok(Error::InvalidRecipientList))
    );

    let mut too_many: Vec<Address> = Vec::new(&s.env);
    for _ in 0..=MAX_BATCH_RECIPIENTS {
        too_many.push_back(Address::generate(&s.env));
    }
    assert_eq!(
        s.client.try_all_meet_requirement(&too_many, &skill(), &1),
        Err(Ok(Error::InvalidRecipientList))
    );
}
