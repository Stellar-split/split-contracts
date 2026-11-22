//! Issue #1169: hash-commitment verifier (first slice, NOT zero-knowledge).
//!
//! This is a salted SHA-256 commit/reveal check plus a verification-key hash
//! allow-list and proof counters. A reveal discloses the secret, so it does
//! NOT preserve privacy and there is no circuit or ZK proof system here.
//! Not covered: real ZK proofs, circuits, on-chain verifier keys, storage.

use soroban_sdk::{Bytes, BytesN, Env};

/// Errors for the verifier helpers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ZkError {
    /// Verification key hash not in the registry.
    UnknownKey,
}

/// Counters of verification attempts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProofStats {
    /// Successful verifications.
    pub verified: u32,
    /// Failed verifications.
    pub rejected: u32,
}

impl ProofStats {
    /// Records one outcome (saturating).
    pub fn record(&mut self, ok: bool) {
        if ok {
            self.verified = self.verified.saturating_add(1);
        } else {
            self.rejected = self.rejected.saturating_add(1);
        }
    }
}

/// Computes `sha256(salt || secret)`.
pub fn commit(env: &Env, secret: &Bytes, salt: &BytesN<32>) -> BytesN<32> {
    let mut data = Bytes::from_slice(env, &salt.to_array());
    data.append(secret);
    env.crypto().sha256(&data).into()
}

/// Checks that revealing `secret`/`salt` matches `commitment`.
pub fn verify_reveal(env: &Env, commitment: &BytesN<32>, secret: &Bytes, salt: &BytesN<32>) -> bool {
    commit(env, secret, salt) == *commitment
}

/// Checks a key hash against an allow-list of registered key hashes.
pub fn require_registered_key(
    registry: &[BytesN<32>],
    key_hash: &BytesN<32>,
) -> Result<(), ZkError> {
    if registry.iter().any(|k| k == key_hash) {
        Ok(())
    } else {
        Err(ZkError::UnknownKey)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_and_stats() {
        let env = Env::default();
        let salt = BytesN::from_array(&env, &[7u8; 32]);
        let secret = Bytes::from_slice(&env, b"hello");
        let c = commit(&env, &secret, &salt);
        let mut stats = ProofStats::default();
        stats.record(verify_reveal(&env, &c, &secret, &salt));
        stats.record(verify_reveal(&env, &c, &Bytes::from_slice(&env, b"nope"), &salt));
        assert_eq!(stats, ProofStats { verified: 1, rejected: 1 });
    }

    #[test]
    fn key_registry() {
        let env = Env::default();
        let k = BytesN::from_array(&env, &[1u8; 32]);
        let other = BytesN::from_array(&env, &[2u8; 32]);
        let reg = [k.clone()];
        assert!(require_registered_key(&reg, &k).is_ok());
        assert_eq!(require_registered_key(&reg, &other), Err(ZkError::UnknownKey));
    }
}
