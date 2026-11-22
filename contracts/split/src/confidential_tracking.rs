//! Issue #921: Confidential payment tracking.
//!
//! Records payments against an invoice without ever putting the amount
//! on-chain, while still letting anyone check that each record is well formed
//! and letting the creator prove the invoice total — all without revealing
//! any individual amount.
//!
//! ## Amount masking
//! A payer submits a Pedersen commitment `C = v·G + r·H` over BLS12-381 G1,
//! where `v` is the amount and `r` a random blinding scalar. `G` and `H` are
//! the contract's nothing-up-my-sleeve generators pinned at `initialize`
//! (shared with `reveal_confidential_payment`). Without `r`, `C` reveals
//! nothing about `v`. Events carry only a digest of `C` — never an amount.
//!
//! ## Zero-knowledge verification
//! Each record carries a non-interactive proof of knowledge of the opening
//! `(v, r)` (Okamoto / Schnorr protocol, Fiat–Shamir transformed):
//!
//! * prover picks random `a, b`, sends `T = a·G + b·H`
//! * challenge `e = H(domain ‖ C ‖ T ‖ invoice_id ‖ payer)` mod r
//! * responses `s1 = a + e·v`, `s2 = b + e·r`
//! * verifier checks `s1·G + s2·H == T + e·C`
//!
//! The proof convinces the verifier the payer knows a valid opening while
//! revealing nothing about `v`. Binding the challenge to `invoice_id` and
//! `payer` prevents replaying a proof on another invoice or by another payer.
//!
//! Commitments are additively homomorphic, so the contract keeps a running
//! aggregate `ΣC = (Σv)·G + (Σr)·H`. The creator (who receives every payer's
//! encrypted amount) can prove the invoice total `Σv` by revealing only the
//! aggregate blinding `Σr` — individual amounts stay hidden.
//!
//! ## Encrypted storage
//! The payer also submits the amount encrypted to the creator's key
//! (`encrypted_amount`). The contract treats it as opaque ciphertext, bounds
//! its size and stores its SHA-256 digest so later tampering is detectable.
//!
//! ## Audit trail
//! Every state change appends an entry to a per-invoice hash chain:
//! `entry_hash = sha256(prev_hash ‖ seq ‖ action ‖ actor ‖ subject ‖ timestamp)`.
//! `verify_confidential_audit_chain` recomputes the chain end to end.
//!
//! ## Storage
//! * `(ctrk_rec, invoice_id, payer)` → [`ConfidentialRecord`] (persistent)
//! * `(ctrk_pyr, invoice_id)` → `Vec<Address>` payers with a record (persistent)
//! * `(ctrk_agg, invoice_id)` → `BytesN<96>` aggregate commitment (persistent)
//! * `(ctrk_tot, invoice_id)` → `i128` last verified total (persistent)
//! * `(ctrk_aud, invoice_id, seq)` → [`ConfidentialAuditEntry`] (persistent)
//! * `(ctrk_ahd, invoice_id)` → `(u32, BytesN<32>)` chain length and head hash (persistent)

use soroban_sdk::crypto::bls12_381::{Fr, G1Affine};
use soroban_sdk::xdr::ToXdr;
use soroban_sdk::{
    contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env, Symbol, Vec, U256,
};

use crate::events;
use crate::types::InvoiceStatus;
use crate::{
    load_invoice, pedersen_g_key, pedersen_h_key, SplitContract, SplitContractArgs,
    SplitContractClient,
};

/// Maximum distinct payers tracked per invoice.
pub const MAX_CONFIDENTIAL_PAYERS: u32 = 100;

/// Maximum audit entries per invoice (bounds replacement churn).
pub const MAX_CONFIDENTIAL_AUDIT_ENTRIES: u32 = 500;

/// Maximum ciphertext size for `encrypted_amount`, in bytes.
pub const MAX_ENCRYPTED_AMOUNT_LEN: u32 = 512;

/// Maximum entries returned by one `get_confidential_audit_trail` call.
pub const MAX_AUDIT_PAGE: u32 = 50;

/// Fiat–Shamir domain separator for the proof-of-opening challenge.
const POK_DOMAIN: &[u8] = b"StellarSplit-ConfTrack-PoK-v1";

/// Non-interactive proof of knowledge of a commitment opening `(v, r)`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfidentialProof {
    /// `T = a·G + b·H` for random `a, b`.
    pub nonce_commitment: BytesN<96>,
    /// `s1 = a + e·v` (big-endian scalar).
    pub response_value: BytesN<32>,
    /// `s2 = b + e·r` (big-endian scalar).
    pub response_blinding: BytesN<32>,
}

/// A tracked confidential payment. Contains no plaintext amount.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfidentialRecord {
    pub invoice_id: u64,
    pub payer: Address,
    /// Pedersen commitment `C = v·G + r·H` (uncompressed G1 point).
    pub commitment: BytesN<96>,
    pub proof: ConfidentialProof,
    /// Amount encrypted to the invoice creator; opaque to the contract.
    pub encrypted_amount: Bytes,
    /// SHA-256 of `encrypted_amount`, for integrity checks.
    pub ciphertext_hash: BytesN<32>,
    pub recorded_at: u64,
    /// Number of times this payer's record has been written (1 = original).
    pub revision: u32,
}

/// Kind of state change captured by an audit entry.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ConfidentialAuditAction {
    Recorded = 0,
    Replaced = 1,
    TotalVerified = 2,
}

/// One link of the per-invoice audit hash chain.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfidentialAuditEntry {
    pub seq: u32,
    pub action: ConfidentialAuditAction,
    pub actor: Address,
    /// Digest of the commitment (record actions) or aggregate (verification).
    pub subject: BytesN<32>,
    pub timestamp: u64,
    pub prev_hash: BytesN<32>,
    pub entry_hash: BytesN<32>,
}

/// Amount-free overview of an invoice's confidential payments.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfidentialSummary {
    pub invoice_id: u64,
    pub payment_count: u32,
    pub aggregate_commitment: Option<BytesN<96>>,
    /// Total proven by the creator against the current aggregate, if any.
    pub verified_total: Option<i128>,
    pub audit_length: u32,
    pub audit_head: BytesN<32>,
}

fn record_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("ctrk_rec"), invoice_id, payer.clone())
}

fn payers_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("ctrk_pyr"), invoice_id)
}

fn aggregate_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("ctrk_agg"), invoice_id)
}

fn verified_total_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("ctrk_tot"), invoice_id)
}

fn audit_entry_key(invoice_id: u64, seq: u32) -> (Symbol, u64, u32) {
    (symbol_short!("ctrk_aud"), invoice_id, seq)
}

fn audit_head_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("ctrk_ahd"), invoice_id)
}

fn generators(env: &Env) -> (G1Affine, G1Affine) {
    let g: G1Affine = env
        .storage()
        .instance()
        .get(&pedersen_g_key())
        .expect("not initialized");
    let h: G1Affine = env
        .storage()
        .instance()
        .get(&pedersen_h_key())
        .expect("not initialized");
    (g, h)
}

/// Parse and validate a G1 point supplied by a caller.
fn parse_point(env: &Env, bytes: &BytesN<96>) -> G1Affine {
    let point = G1Affine::from_bytes(bytes.clone());
    assert!(
        env.crypto().bls12_381().g1_is_in_subgroup(&point),
        "invalid commitment point"
    );
    point
}

fn digest_point(env: &Env, point: &BytesN<96>) -> BytesN<32> {
    let bytes: Bytes = point.clone().into();
    env.crypto().sha256(&bytes).into()
}

/// Fiat–Shamir challenge bound to the commitment, nonce, invoice and payer.
fn challenge(
    env: &Env,
    commitment: &BytesN<96>,
    nonce_commitment: &BytesN<96>,
    invoice_id: u64,
    payer: &Address,
) -> Fr {
    let mut transcript = Bytes::from_slice(env, POK_DOMAIN);
    transcript.append(&commitment.clone().into());
    transcript.append(&nonce_commitment.clone().into());
    transcript.extend_from_array(&invoice_id.to_be_bytes());
    transcript.append(&payer.clone().to_xdr(env));
    let hash: BytesN<32> = env.crypto().sha256(&transcript).into();
    Fr::from_bytes(hash)
}

/// Verify a proof of knowledge of the opening of `commitment`:
/// `s1·G + s2·H == T + e·C`.
fn verify_opening_proof(
    env: &Env,
    commitment: &BytesN<96>,
    proof: &ConfidentialProof,
    invoice_id: u64,
    payer: &Address,
) -> bool {
    let (g, h) = generators(env);
    let c = parse_point(env, commitment);
    let t = parse_point(env, &proof.nonce_commitment);
    let e = challenge(env, commitment, &proof.nonce_commitment, invoice_id, payer);
    let s1 = Fr::from_bytes(proof.response_value.clone());
    let s2 = Fr::from_bytes(proof.response_blinding.clone());

    let lhs = (g * s1) + (h * s2);
    let rhs = t + (c * e);
    lhs.to_bytes() == rhs.to_bytes()
}

/// `v·G + r·H` for a non-negative amount `v`.
fn commit(env: &Env, value: i128, blinding: &BytesN<32>) -> G1Affine {
    assert!(value >= 0, "amount must not be negative");
    let (g, h) = generators(env);
    let v = Fr::from_u256(U256::from_u128(env, value as u128));
    let r = Fr::from_bytes(blinding.clone());
    (g * v) + (h * r)
}

fn load_payers(env: &Env, invoice_id: u64) -> Vec<Address> {
    env.storage()
        .persistent()
        .get(&payers_key(invoice_id))
        .unwrap_or(Vec::new(env))
}

fn load_audit_head(env: &Env, invoice_id: u64) -> (u32, BytesN<32>) {
    env.storage()
        .persistent()
        .get(&audit_head_key(invoice_id))
        .unwrap_or((0u32, BytesN::from_array(env, &[0u8; 32])))
}

fn audit_entry_hash(
    env: &Env,
    prev_hash: &BytesN<32>,
    seq: u32,
    action: ConfidentialAuditAction,
    actor: &Address,
    subject: &BytesN<32>,
    timestamp: u64,
) -> BytesN<32> {
    let mut preimage: Bytes = prev_hash.clone().into();
    preimage.extend_from_array(&seq.to_be_bytes());
    preimage.extend_from_array(&(action as u32).to_be_bytes());
    preimage.append(&actor.clone().to_xdr(env));
    preimage.append(&subject.clone().into());
    preimage.extend_from_array(&timestamp.to_be_bytes());
    env.crypto().sha256(&preimage).into()
}

fn append_audit(
    env: &Env,
    invoice_id: u64,
    action: ConfidentialAuditAction,
    actor: &Address,
    subject: BytesN<32>,
) {
    let (len, prev_hash) = load_audit_head(env, invoice_id);
    assert!(
        len < MAX_CONFIDENTIAL_AUDIT_ENTRIES,
        "confidential audit trail is full"
    );
    let timestamp = env.ledger().timestamp();
    let entry_hash = audit_entry_hash(env, &prev_hash, len, action, actor, &subject, timestamp);
    let entry = ConfidentialAuditEntry {
        seq: len,
        action,
        actor: actor.clone(),
        subject,
        timestamp,
        prev_hash,
        entry_hash: entry_hash.clone(),
    };
    env.storage()
        .persistent()
        .set(&audit_entry_key(invoice_id, len), &entry);
    env.storage()
        .persistent()
        .set(&audit_head_key(invoice_id), &(len + 1, entry_hash));
}

#[contractimpl]
impl SplitContract {
    /// Issue #921: Record a confidential payment for `invoice_id`.
    ///
    /// Stores only the commitment, its proof and the encrypted amount — the
    /// plaintext amount never touches the chain. A payer that already has a
    /// record replaces it (e.g. to correct an amount); the aggregate is
    /// updated homomorphically and any previously verified total is cleared.
    ///
    /// No funds move: this tracks payments settled through other channels
    /// (or later via `reveal_confidential_payment`).
    ///
    /// # Panics
    /// * `"invalid commitment point"` — commitment or nonce not a valid G1 point.
    /// * `"invalid confidential payment proof"` — proof does not verify.
    /// * `"encrypted amount ..."` — empty or oversized ciphertext.
    /// * invoice not pending, past deadline, or payer not allowed.
    pub fn record_confidential_payment(
        env: Env,
        payer: Address,
        invoice_id: u64,
        commitment: BytesN<96>,
        proof: ConfidentialProof,
        encrypted_amount: Bytes,
    ) -> ConfidentialRecord {
        crate::require_not_paused(&env);
        payer.require_auth();

        let invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(
            env.ledger().timestamp() <= invoice.deadline,
            "invoice deadline has passed"
        );
        if let Some(ref whitelist) = invoice.allowed_payers {
            assert!(whitelist.contains(&payer), "payer not allowed");
        }

        assert!(!encrypted_amount.is_empty(), "encrypted amount must not be empty");
        assert!(
            encrypted_amount.len() <= MAX_ENCRYPTED_AMOUNT_LEN,
            "encrypted amount too large"
        );
        assert!(
            verify_opening_proof(&env, &commitment, &proof, invoice_id, &payer),
            "invalid confidential payment proof"
        );

        let key = record_key(invoice_id, &payer);
        let previous: Option<ConfidentialRecord> = env.storage().persistent().get(&key);

        // Update the homomorphic aggregate: ΣC' = ΣC − C_old + C_new.
        let new_point = parse_point(&env, &commitment);
        let aggregate: Option<BytesN<96>> = env.storage().persistent().get(&aggregate_key(invoice_id));
        let mut next_aggregate = match aggregate {
            Some(bytes) => G1Affine::from_bytes(bytes) + new_point,
            None => new_point,
        };
        if let Some(ref prev) = previous {
            next_aggregate = next_aggregate + (-G1Affine::from_bytes(prev.commitment.clone()));
        } else {
            let mut payers = load_payers(&env, invoice_id);
            assert!(
                payers.len() < MAX_CONFIDENTIAL_PAYERS,
                "too many confidential payers"
            );
            payers.push_back(payer.clone());
            env.storage().persistent().set(&payers_key(invoice_id), &payers);
        }
        env.storage()
            .persistent()
            .set(&aggregate_key(invoice_id), &next_aggregate.to_bytes());
        // The old total no longer matches the aggregate.
        env.storage()
            .persistent()
            .remove(&verified_total_key(invoice_id));

        let record = ConfidentialRecord {
            invoice_id,
            payer: payer.clone(),
            commitment: commitment.clone(),
            proof,
            ciphertext_hash: env.crypto().sha256(&encrypted_amount).into(),
            encrypted_amount,
            recorded_at: env.ledger().timestamp(),
            revision: previous.as_ref().map(|p| p.revision + 1).unwrap_or(1),
        };
        env.storage().persistent().set(&key, &record);

        let action = if previous.is_some() {
            ConfidentialAuditAction::Replaced
        } else {
            ConfidentialAuditAction::Recorded
        };
        let commitment_digest = digest_point(&env, &commitment);
        append_audit(&env, invoice_id, action, &payer, commitment_digest.clone());
        events::confidential_payment_recorded(
            &env,
            invoice_id,
            &payer,
            &commitment_digest,
            record.revision,
        );
        record
    }

    /// Issue #921: Creator proves the invoice's confidential total.
    ///
    /// Checks `total·G + aggregate_blinding·H == ΣC`. On success the total is
    /// stored and an audit entry appended; individual amounts remain hidden.
    /// Returns `false` (and changes nothing) when the opening does not match.
    pub fn verify_confidential_total(
        env: Env,
        caller: Address,
        invoice_id: u64,
        total: i128,
        aggregate_blinding: BytesN<32>,
    ) -> bool {
        crate::require_not_paused(&env);
        caller.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        let is_creator =
            invoice.creator == caller || invoice.co_creators.iter().any(|c| c == caller);
        assert!(is_creator, "NotAuthorized: only creator or co-creator may verify");
        assert!(total > 0, "total must be positive");

        let aggregate: BytesN<96> = env
            .storage()
            .persistent()
            .get(&aggregate_key(invoice_id))
            .expect("no confidential payments recorded");
        if commit(&env, total, &aggregate_blinding).to_bytes() != aggregate {
            return false;
        }

        env.storage()
            .persistent()
            .set(&verified_total_key(invoice_id), &total);
        append_audit(
            &env,
            invoice_id,
            ConfidentialAuditAction::TotalVerified,
            &caller,
            digest_point(&env, &aggregate),
        );
        events::confidential_total_verified(&env, invoice_id, &caller, total);
        true
    }

    /// Issue #921: Re-verify a stored record's proof of knowledge. Confirms
    /// the payer knows a valid opening without revealing the amount.
    pub fn verify_confidential_record(env: Env, invoice_id: u64, payer: Address) -> bool {
        let record: ConfidentialRecord = env
            .storage()
            .persistent()
            .get(&record_key(invoice_id, &payer))
            .expect("no confidential record");
        verify_opening_proof(&env, &record.commitment, &record.proof, invoice_id, &payer)
            && env.crypto().sha256(&record.encrypted_amount).to_bytes() == record.ciphertext_hash
    }

    /// Issue #921: Check an opening `(value, blinding)` shared off-chain (e.g.
    /// with an auditor) against a payer's stored commitment. Read-only: the
    /// opening is never written to storage or emitted.
    pub fn verify_confidential_opening(
        env: Env,
        invoice_id: u64,
        payer: Address,
        value: i128,
        blinding: BytesN<32>,
    ) -> bool {
        let record: ConfidentialRecord = env
            .storage()
            .persistent()
            .get(&record_key(invoice_id, &payer))
            .expect("no confidential record");
        commit(&env, value, &blinding).to_bytes() == record.commitment
    }

    /// Issue #921: A payer's confidential record, if any.
    pub fn get_confidential_record(
        env: Env,
        invoice_id: u64,
        payer: Address,
    ) -> Option<ConfidentialRecord> {
        env.storage()
            .persistent()
            .get(&record_key(invoice_id, &payer))
    }

    /// Issue #921: Payers with a confidential record on the invoice.
    pub fn get_confidential_payers(env: Env, invoice_id: u64) -> Vec<Address> {
        load_payers(&env, invoice_id)
    }

    /// Issue #921: Amount-free summary of an invoice's confidential payments.
    pub fn get_confidential_summary(env: Env, invoice_id: u64) -> ConfidentialSummary {
        let (audit_length, audit_head) = load_audit_head(&env, invoice_id);
        ConfidentialSummary {
            invoice_id,
            payment_count: load_payers(&env, invoice_id).len(),
            aggregate_commitment: env.storage().persistent().get(&aggregate_key(invoice_id)),
            verified_total: env
                .storage()
                .persistent()
                .get(&verified_total_key(invoice_id)),
            audit_length,
            audit_head,
        }
    }

    /// Issue #921: Page of audit entries starting at `start` (max `MAX_AUDIT_PAGE`).
    pub fn get_confidential_audit_trail(
        env: Env,
        invoice_id: u64,
        start: u32,
        limit: u32,
    ) -> Vec<ConfidentialAuditEntry> {
        let (len, _) = load_audit_head(&env, invoice_id);
        let end = start.saturating_add(limit.min(MAX_AUDIT_PAGE)).min(len);
        let mut entries = Vec::new(&env);
        for seq in start..end {
            if let Some(entry) = env
                .storage()
                .persistent()
                .get(&audit_entry_key(invoice_id, seq))
            {
                entries.push_back(entry);
            }
        }
        entries
    }

    /// Issue #921: Recompute the audit hash chain and check it against the
    /// stored head. `false` if any entry is missing, altered or out of order.
    pub fn verify_confidential_audit_chain(env: Env, invoice_id: u64) -> bool {
        let (len, head) = load_audit_head(&env, invoice_id);
        let mut prev_hash = BytesN::from_array(&env, &[0u8; 32]);
        for seq in 0..len {
            let entry: ConfidentialAuditEntry = match env
                .storage()
                .persistent()
                .get(&audit_entry_key(invoice_id, seq))
            {
                Some(e) => e,
                None => return false,
            };
            if entry.seq != seq || entry.prev_hash != prev_hash {
                return false;
            }
            let expected = audit_entry_hash(
                &env,
                &prev_hash,
                seq,
                entry.action,
                &entry.actor,
                &entry.subject,
                entry.timestamp,
            );
            if expected != entry.entry_hash {
                return false;
            }
            prev_hash = expected;
        }
        prev_hash == head
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    /// Register the contract and pin the same generators `initialize` derives.
    fn setup_generators(env: &Env) -> Address {
        let contract_id = env.register(SplitContract, ());
        env.as_contract(&contract_id, || {
            let dst = Bytes::from_slice(env, b"StellarSplit-Pedersen-BLS12381G1-v1");
            let bls = env.crypto().bls12_381();
            let g = bls.hash_to_g1(&Bytes::from_slice(env, b"pedersen-generator-G"), &dst);
            let h = bls.hash_to_g1(&Bytes::from_slice(env, b"pedersen-generator-H"), &dst);
            env.storage().instance().set(&pedersen_g_key(), &g);
            env.storage().instance().set(&pedersen_h_key(), &h);
        });
        contract_id
    }

    fn scalar(env: &Env, v: u128) -> Fr {
        Fr::from_u256(U256::from_u128(env, v))
    }

    /// Build `(C, proof)` for amount `v` with blinding `r` and nonces `a, b`.
    fn prove(
        env: &Env,
        invoice_id: u64,
        payer: &Address,
        v: u128,
        r: u128,
        a: u128,
        b: u128,
    ) -> (BytesN<96>, ConfidentialProof) {
        let (g, h) = generators(env);
        let c = (g.clone() * scalar(env, v)) + (h.clone() * scalar(env, r));
        let t = (g * scalar(env, a)) + (h * scalar(env, b));
        let e = challenge(env, &c.to_bytes(), &t.to_bytes(), invoice_id, payer);
        let s1 = scalar(env, a) + e.clone() * scalar(env, v);
        let s2 = scalar(env, b) + e * scalar(env, r);
        (
            c.to_bytes(),
            ConfidentialProof {
                nonce_commitment: t.to_bytes(),
                response_value: s1.to_bytes(),
                response_blinding: s2.to_bytes(),
            },
        )
    }

    #[test]
    fn test_valid_opening_proof_verifies() {
        let env = Env::default();
        let contract_id = setup_generators(&env);
        let payer = Address::generate(&env);
        env.as_contract(&contract_id, || {
            let (c, proof) = prove(&env, 1, &payer, 500, 77, 11, 13);
            assert!(verify_opening_proof(&env, &c, &proof, 1, &payer));
        });
    }

    #[test]
    fn test_proof_bound_to_invoice_and_payer() {
        let env = Env::default();
        let contract_id = setup_generators(&env);
        let payer = Address::generate(&env);
        let other = Address::generate(&env);
        env.as_contract(&contract_id, || {
            let (c, proof) = prove(&env, 1, &payer, 500, 77, 11, 13);
            assert!(!verify_opening_proof(&env, &c, &proof, 2, &payer));
            assert!(!verify_opening_proof(&env, &c, &proof, 1, &other));
        });
    }

    #[test]
    fn test_tampered_response_fails() {
        let env = Env::default();
        let contract_id = setup_generators(&env);
        let payer = Address::generate(&env);
        env.as_contract(&contract_id, || {
            let (c, mut proof) = prove(&env, 1, &payer, 500, 77, 11, 13);
            proof.response_value = scalar(&env, 12345).to_bytes();
            assert!(!verify_opening_proof(&env, &c, &proof, 1, &payer));
        });
    }

    #[test]
    fn test_commitments_aggregate_homomorphically() {
        let env = Env::default();
        let contract_id = setup_generators(&env);
        env.as_contract(&contract_id, || {
            let r1 = scalar(&env, 5).to_bytes();
            let r2 = scalar(&env, 9).to_bytes();
            let sum = commit(&env, 300, &r1) + commit(&env, 200, &r2);
            let expected = commit(&env, 500, &scalar(&env, 14).to_bytes());
            assert_eq!(sum.to_bytes(), expected.to_bytes());
            let wrong = commit(&env, 499, &scalar(&env, 14).to_bytes());
            assert_ne!(sum.to_bytes(), wrong.to_bytes());
        });
    }

    #[test]
    fn test_audit_chain_append_and_verify() {
        let env = Env::default();
        let contract_id = setup_generators(&env);
        let client = SplitContractClient::new(&env, &contract_id);
        let actor = Address::generate(&env);
        env.as_contract(&contract_id, || {
            let subject = BytesN::from_array(&env, &[1u8; 32]);
            append_audit(&env, 7, ConfidentialAuditAction::Recorded, &actor, subject.clone());
            append_audit(&env, 7, ConfidentialAuditAction::Replaced, &actor, subject);
        });
        assert!(client.verify_confidential_audit_chain(&7));
        let trail = client.get_confidential_audit_trail(&7, &0, &10);
        assert_eq!(trail.len(), 2);
        assert_eq!(trail.get(1).unwrap().prev_hash, trail.get(0).unwrap().entry_hash);

        // Tamper with the first entry: the chain must no longer verify.
        env.as_contract(&contract_id, || {
            let mut entry: ConfidentialAuditEntry =
                env.storage().persistent().get(&audit_entry_key(7, 0)).unwrap();
            entry.timestamp += 1;
            env.storage().persistent().set(&audit_entry_key(7, 0), &entry);
        });
        assert!(!client.verify_confidential_audit_chain(&7));
    }

    #[test]
    fn test_empty_summary() {
        let env = Env::default();
        let contract_id = setup_generators(&env);
        let client = SplitContractClient::new(&env, &contract_id);
        let summary = client.get_confidential_summary(&3);
        assert_eq!(summary.payment_count, 0);
        assert_eq!(summary.aggregate_commitment, None);
        assert_eq!(summary.verified_total, None);
        assert_eq!(summary.audit_length, 0);
        assert!(client.verify_confidential_audit_chain(&3));
    }
}
