//! Memory-bound proof of work (issue #1178).
//!
//! A small, deterministic memory-hard puzzle: fill a caller-provided buffer
//! with a hash chain seeded from `(challenge, nonce)`, then perform
//! data-dependent reads across it and require the final value to have enough
//! leading zero bits. The mixing function is a simple non-cryptographic
//! 64-bit mixer, so this is a rate-limiting building block, NOT a vetted
//! cryptographic PoW. Heap free: the memory buffer is supplied by the caller.
#![allow(dead_code)]

/// Errors returned by proof-of-work helpers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowError {
    /// The working buffer is too small for the required memory cost.
    BufferTooSmall,
    /// Difficulty is outside 1..=64.
    InvalidDifficulty,
    /// The nonce search limit was reached without a solution.
    NoSolution,
}

/// 64-bit mixer (splitmix64 finalizer).
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Compute the memory-bound digest for `(challenge, nonce)` using `memory`
/// as scratch space, with `rounds` data-dependent read rounds.
pub fn compute(challenge: u64, nonce: u64, memory: &mut [u64], rounds: u32) -> Result<u64, PowError> {
    if memory.is_empty() {
        return Err(PowError::BufferTooSmall);
    }
    let len = memory.len() as u64;
    let mut state = mix(challenge ^ mix(nonce));
    for slot in memory.iter_mut() {
        state = mix(state);
        *slot = state;
    }
    for _ in 0..rounds {
        let idx = (state % len) as usize;
        state = mix(state ^ memory[idx]);
        memory[idx] = state;
    }
    Ok(state)
}

/// True when `digest` has at least `difficulty` leading zero bits.
pub fn meets_difficulty(digest: u64, difficulty: u32) -> bool {
    difficulty >= 1 && difficulty <= 64 && digest.leading_zeros() >= difficulty
}

/// Verify a claimed solution (one full memory computation).
pub fn verify(
    challenge: u64,
    nonce: u64,
    difficulty: u32,
    memory: &mut [u64],
    rounds: u32,
) -> Result<bool, PowError> {
    if difficulty == 0 || difficulty > 64 {
        return Err(PowError::InvalidDifficulty);
    }
    let digest = compute(challenge, nonce, memory, rounds)?;
    Ok(meets_difficulty(digest, difficulty))
}

/// Search nonces `0..max_attempts` for a solution; returns the first nonce.
pub fn solve(
    challenge: u64,
    difficulty: u32,
    memory: &mut [u64],
    rounds: u32,
    max_attempts: u64,
) -> Result<u64, PowError> {
    if difficulty == 0 || difficulty > 64 {
        return Err(PowError::InvalidDifficulty);
    }
    let mut nonce = 0u64;
    while nonce < max_attempts {
        let digest = compute(challenge, nonce, memory, rounds)?;
        if meets_difficulty(digest, difficulty) {
            return Ok(nonce);
        }
        nonce += 1;
    }
    Err(PowError::NoSolution)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_is_deterministic_and_nonce_sensitive() {
        let mut a = [0u64; 64];
        let mut b = [0u64; 64];
        let d1 = compute(1, 2, &mut a, 32).unwrap();
        let d2 = compute(1, 2, &mut b, 32).unwrap();
        assert_eq!(d1, d2);
        assert_ne!(d1, compute(1, 3, &mut a, 32).unwrap());
    }

    #[test]
    fn solve_then_verify() {
        let mut mem = [0u64; 32];
        let nonce = solve(42, 4, &mut mem, 16, 10_000).unwrap();
        assert_eq!(verify(42, nonce, 4, &mut mem, 16), Ok(true));
    }

    #[test]
    fn rejects_bad_inputs() {
        let mut empty: [u64; 0] = [];
        assert_eq!(compute(1, 1, &mut empty, 1), Err(PowError::BufferTooSmall));
        let mut mem = [0u64; 8];
        assert_eq!(verify(1, 1, 0, &mut mem, 1), Err(PowError::InvalidDifficulty));
        assert_eq!(solve(1, 65, &mut mem, 1, 5), Err(PowError::InvalidDifficulty));
        assert_eq!(solve(1, 64, &mut mem, 1, 3), Err(PowError::NoSolution));
    }

    #[test]
    fn difficulty_check() {
        assert!(meets_difficulty(0x00FF_FFFF_FFFF_FFFF, 8));
        assert!(!meets_difficulty(0x00FF_FFFF_FFFF_FFFF, 9));
        assert!(!meets_difficulty(0, 0));
    }
}
