//! Issue #768: on-chain invoice attestation (third-party endorsement).
//!
//! Attesters (auditors, employers, ...) publish a short statement about an
//! invoice. Attestations live under their own persistent key so the invoice
//! structs and the shared `StorageKey` enum are untouched.
//!
//! Events: `(split, attested, invoice_id)` -> attester and
//! `(split, att_rev, invoice_id)` -> attester.

use crate::error::ContractError;
use crate::{SplitContract, SplitContractClient};
use soroban_sdk::{
    contractimpl, contracttype, panic_with_error, symbol_short, Address, Bytes,
    Env, Vec,
};

/// Maximum statement length in bytes.
pub const MAX_ATTESTATION_STATEMENT_LEN: u32 = 256;
/// Maximum number of attestation entries (including revoked) per invoice.
pub const MAX_ATTESTATIONS_PER_INVOICE: u32 = 5;

/// A third-party endorsement of an invoice.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Attestation {
    /// Address that signed the attestation.
    pub attester: Address,
    /// Short free-form statement (max 256 bytes).
    pub statement: Bytes,
    /// Ledger timestamp of the (latest) attestation.
    pub timestamp: u64,
    /// True once the attester has revoked it; the entry is kept for audit.
    pub revoked: bool,
}

/// Persistent storage keys owned by this module (issue #768).
#[contracttype]
#[derive(Clone)]
pub enum AttestKey {
    /// `Vec<Attestation>` for an invoice.
    Attestations(u64),
}

fn load(env: &Env, invoice_id: u64) -> Vec<Attestation> {
    env.storage()
        .persistent()
        .get(&AttestKey::Attestations(invoice_id))
        .unwrap_or_else(|| Vec::new(env))
}

fn save(env: &Env, invoice_id: u64, list: &Vec<Attestation>) {
    env.storage()
        .persistent()
        .set(&AttestKey::Attestations(invoice_id), list);
}

#[contractimpl]
impl SplitContract {
    /// Attest to an invoice. The attester must authorise the call. A repeated
    /// attestation from the same attester overwrites (and un-revokes) the
    /// previous one; at most 5 distinct attesters are allowed per invoice.
    pub fn attest_invoice(env: Env, invoice_id: u64, attester: Address, statement: Bytes) {
        attester.require_auth();
        crate::load_invoice(&env, invoice_id);
        if statement.len() > MAX_ATTESTATION_STATEMENT_LEN {
            panic_with_error!(&env, ContractError::AttestationStatementTooLong);
        }
        let mut list = load(&env, invoice_id);
        let entry = Attestation {
            attester: attester.clone(),
            statement,
            timestamp: env.ledger().timestamp(),
            revoked: false,
        };
        match list.iter().position(|a| a.attester == attester) {
            Some(i) => list.set(i as u32, entry),
            None => {
                if list.len() >= MAX_ATTESTATIONS_PER_INVOICE {
                    panic_with_error!(&env, ContractError::AttestationLimitReached);
                }
                list.push_back(entry);
            }
        }
        save(&env, invoice_id, &list);
        env.events().publish(
            (symbol_short!("split"), symbol_short!("attested"), invoice_id),
            attester,
        );
    }

    /// Revoke the caller's own attestation. The entry stays stored with
    /// `revoked = true`.
    pub fn revoke_attestation(env: Env, invoice_id: u64, attester: Address) {
        attester.require_auth();
        let mut list = load(&env, invoice_id);
        let i = match list.iter().position(|a| a.attester == attester) {
            Some(i) => i as u32,
            None => panic_with_error!(&env, ContractError::AttestationNotFound),
        };
        let mut entry = list.get(i).expect("attestation index in range");
        entry.revoked = true;
        list.set(i, entry);
        save(&env, invoice_id, &list);
        env.events().publish(
            (symbol_short!("split"), symbol_short!("att_rev"), invoice_id),
            attester,
        );
    }

    /// All attestations (including revoked ones) recorded for an invoice.
    pub fn get_attestations(env: Env, invoice_id: u64) -> Vec<Attestation> {
        load(&env, invoice_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{client, make_invoice, setup_initialized};
    use soroban_sdk::testutils::Address as _;

    fn stmt(env: &Env, s: &[u8]) -> Bytes {
        Bytes::from_slice(env, s)
    }

    #[test]
    fn attest_and_read() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let id = make_invoice(&env, &c, &Address::generate(&env), &Address::generate(&env), 100, &token, 9_999);
        let a = Address::generate(&env);
        c.attest_invoice(&id, &a, &stmt(&env, b"ok"));
        let l = c.get_attestations(&id);
        assert_eq!(l.len(), 1);
        assert_eq!(l.get(0).unwrap().attester, a);
        assert!(!l.get(0).unwrap().revoked);
    }

    #[test]
    fn revoke_keeps_entry_flagged() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let id = make_invoice(&env, &c, &Address::generate(&env), &Address::generate(&env), 100, &token, 9_999);
        let a = Address::generate(&env);
        c.attest_invoice(&id, &a, &stmt(&env, b"ok"));
        c.revoke_attestation(&id, &a);
        let l = c.get_attestations(&id);
        assert_eq!(l.len(), 1);
        assert!(l.get(0).unwrap().revoked);
    }

    #[test]
    fn duplicate_attester_overwrites() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let id = make_invoice(&env, &c, &Address::generate(&env), &Address::generate(&env), 100, &token, 9_999);
        let a = Address::generate(&env);
        c.attest_invoice(&id, &a, &stmt(&env, b"one"));
        c.attest_invoice(&id, &a, &stmt(&env, b"two"));
        let l = c.get_attestations(&id);
        assert_eq!(l.len(), 1);
        assert_eq!(l.get(0).unwrap().statement, stmt(&env, b"two"));
    }

    #[test]
    fn max_five_attestations() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let id = make_invoice(&env, &c, &Address::generate(&env), &Address::generate(&env), 100, &token, 9_999);
        for _ in 0..5 {
            c.attest_invoice(&id, &Address::generate(&env), &stmt(&env, b"x"));
        }
        let r = c.try_attest_invoice(&id, &Address::generate(&env), &stmt(&env, b"x"));
        assert!(r.is_err());
    }

    #[test]
    fn statement_too_long_rejected() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let id = make_invoice(&env, &c, &Address::generate(&env), &Address::generate(&env), 100, &token, 9_999);
        let big = Bytes::from_slice(&env, &[7u8; 257]);
        assert!(c.try_attest_invoice(&id, &Address::generate(&env), &big).is_err());
    }
}
