//! Issue #1174: constant-time comparison helpers (first slice).
//!
//! Small best-effort helpers that avoid data-dependent early exits when
//! comparing secrets such as MACs or hashes, plus a counter of guarded
//! comparisons. These are NOT audited and the compiler may still optimise
//! them; no side-channel resistance guarantee is claimed. Soroban contract
//! code runs in a deterministic metered VM, which also limits what timing
//! signals are observable on-chain.

use core::hint::black_box;

/// Compares two byte slices without early exit on the first mismatch.
/// Length differences return `false` (length is treated as public).
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    black_box(diff) == 0
}

/// Branch-free select: returns `a` if `choice` is true, else `b`.
pub fn ct_select_u64(choice: bool, a: u64, b: u64) -> u64 {
    let mask = (choice as u64).wrapping_neg();
    (a & mask) | (b & !mask)
}

/// Counts guarded comparisons performed and how many matched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GuardStats {
    /// Total comparisons.
    pub comparisons: u32,
    /// Comparisons that matched.
    pub matches: u32,
}

impl GuardStats {
    /// Performs `ct_eq` and records the result.
    pub fn compare(&mut self, a: &[u8], b: &[u8]) -> bool {
        let eq = ct_eq(a, b);
        self.comparisons = self.comparisons.saturating_add(1);
        if eq {
            self.matches = self.matches.saturating_add(1);
        }
        eq
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eq_and_select() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
        assert!(ct_eq(b"", b""));
        assert_eq!(ct_select_u64(true, 5, 9), 5);
        assert_eq!(ct_select_u64(false, 5, 9), 9);
    }

    #[test]
    fn stats_tracked() {
        let mut s = GuardStats::default();
        assert!(s.compare(&[1, 2], &[1, 2]));
        assert!(!s.compare(&[1, 2], &[1, 3]));
        assert_eq!((s.comparisons, s.matches), (2, 1));
    }
}
