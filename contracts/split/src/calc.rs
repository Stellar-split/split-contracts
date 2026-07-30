//! Arithmetic helpers for token distribution.
//!
//! Implements the **largest-remainder method** to distribute an integer `total`
//! across recipients proportionally, ensuring every stroop is accounted for
//! (i.e. `sum(result) == total` always holds).

use soroban_sdk::{Env, Vec};

/// Distribute `total` among recipients according to their `ratios` out of
/// `denom`, using the largest-remainder method to handle rounding.
///
/// # Arguments
/// * `env`    – Soroban environment (needed to allocate the result `Vec`)
/// * `total`  – total amount to distribute (stroops); must be ≥ 0
/// * `ratios` – relative weight of each recipient (must be non-empty, all ≥ 0)
/// * `denom`  – sum of all ratios (must be > 0)
///
/// # Guarantees
/// * `result.iter().sum::<i128>() == total` always
/// * recipients with larger fractional remainders receive an extra stroop
/// * pure function: no side effects, no storage access
///
/// # Panics
/// * if `ratios` is empty
/// * if `denom` is zero
pub fn distribute_with_remainder(
    env: &Env,
    total: i128,
    ratios: &Vec<i128>,
    denom: i128,
) -> Vec<i128> {
    assert!(!ratios.is_empty(), "ratios must not be empty");
    assert!(denom > 0, "denom must be positive");

    let n = ratios.len() as usize;

    // Compute floor shares and remainders.
    let mut shares = Vec::new(env);
    let mut remainders = Vec::new(env);

    for r in ratios.iter() {
        shares.push_back(total * r / denom);
        remainders.push_back(total * r % denom);
    }

    // How many extra stroops to distribute?
    let distributed: i128 = shares.iter().sum();
    let leftover = (total - distributed) as usize;

    // Build an index array sorted by remainder descending.
    // We use a fixed-size array (max 64 recipients) for no_std compatibility.
    // Contracts with more than 64 recipients would need a larger cap, but
    // 64 is a reasonable upper bound for on-chain use.
    const MAX_RECIPIENTS: usize = 64;
    assert!(n <= MAX_RECIPIENTS, "too many recipients (max 64)");

    let mut indices = [0usize; MAX_RECIPIENTS];
    for i in 0..n {
        indices[i] = i;
    }

    // Insertion sort by remainder descending (small n, simple is best).
    for i in 1..n {
        let key = indices[i];
        let key_rem = remainders.get(key as u32).unwrap_or(0);
        let mut j = i;
        while j > 0 {
            let prev = indices[j - 1];
            let prev_rem = remainders.get(prev as u32).unwrap_or(0);
            if prev_rem >= key_rem {
                break;
            }
            indices[j] = indices[j - 1];
            j -= 1;
        }
        indices[j] = key;
    }

    // Distribute leftover stroops to top-remainder recipients.
    let mut shares_mut = Vec::new(env);
    for s in shares.iter() {
        shares_mut.push_back(s);
    }

    for i in 0..leftover {
        let idx = indices[i] as u32;
        let current = shares_mut.get(idx).unwrap_or(0);
        shares_mut.set(idx, current + 1);
    }

    shares_mut
}

// ---------------------------------------------------------------------------
// Issue #561: Canonical payout ordering
// ---------------------------------------------------------------------------

/// Sort recipients by their Stellar address byte representation (lexicographic
/// order over `Address`'s inner `BytesN<32>`).
///
/// This guarantees deterministic, canonical payout ordering regardless of the
/// order in which recipients were supplied at invoice creation.
///
/// # Arguments
/// * `env`        – Soroban environment
/// * `recipients` – mutable list of recipient addresses to sort in-place
pub fn sort_recipients(env: &Env, recipients: &mut Vec<Address>) {
    let n = recipients.len() as usize;
    if n <= 1 {
        return;
    }

    // Build byte representations for comparison.
    let mut bytes_vec: Vec<(BytesN<32>, usize)> = Vec::new(env);
    for i in 0..n {
        let addr = recipients.get(i as u32).unwrap();
        let bytes = addr.to_bytes();
        bytes_vec.push_back((bytes, i));
    }

    // Insertion sort by byte representation (lexicographic).
    for i in 1..n {
        let key = bytes_vec.get(i).unwrap();
        let mut j = i;
        while j > 0 {
            let prev = bytes_vec.get(j - 1).unwrap();
            if prev.0 <= key.0 {
                break;
            }
            bytes_vec.set(j, bytes_vec.get(j - 1).unwrap());
            j -= 1;
        }
        bytes_vec.set(j, key.clone());
    }

    // Reorder recipients according to sorted indices.
    let mut sorted = Vec::new(env);
    for i in 0..n {
        let (_, original_idx) = bytes_vec.get(i).unwrap();
        sorted.push_back(recipients.get(original_idx as u32).unwrap());
    }
    *recipients = sorted;
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::Env;

    fn make_ratios(env: &Env, vals: &[i128]) -> Vec<i128> {
        let mut v = Vec::new(env);
        for &x in vals {
            v.push_back(x);
        }
        v
    }

    /// Assert sum equals total and return shares.
    fn assert_exact(env: &Env, total: i128, ratios: &[i128], denom: i128) -> Vec<i128> {
        let r_vec = make_ratios(env, ratios);
        let result = distribute_with_remainder(env, total, &r_vec, denom);
        let sum: i128 = result.iter().sum();
        assert_eq!(
            sum, total,
            "sum {sum} != total {total} for ratios {ratios:?} / {denom}"
        );
        result
    }

    #[test]
    fn test_even_split() {
        let env = Env::default();
        let r = assert_exact(&env, 300, &[1, 1, 1], 3);
        assert_eq!(r.get(0), Some(100));
        assert_eq!(r.get(1), Some(100));
        assert_eq!(r.get(2), Some(100));
    }

    #[test]
    fn test_uneven_split_even_amount() {
        let env = Env::default();
        let r = assert_exact(&env, 100, &[1, 1], 2);
        assert_eq!(r.get(0), Some(50));
        assert_eq!(r.get(1), Some(50));
    }

    #[test]
    fn test_uneven_split_odd_amount() {
        let env = Env::default();
        // 101 split 1:1 → sum must be 101, diff ≤ 1
        let r = assert_exact(&env, 101, &[1, 1], 2);
        let a = r.get(0).unwrap();
        let b = r.get(1).unwrap();
        assert!((a - b).abs() <= 1);
    }

    #[test]
    fn test_three_way_remainder() {
        let env = Env::default();
        // 10 / 3: floors [3,3,3] leftover=1 → one recipient gets 4
        let r = assert_exact(&env, 10, &[1, 1, 1], 3);
        let mut vals = [r.get(0).unwrap(), r.get(1).unwrap(), r.get(2).unwrap()];
        vals.sort();
        assert_eq!(vals, [3, 3, 4]);
    }

    #[test]
    fn test_weighted_split() {
        let env = Env::default();
        // 1000 split 1:2:3 (denom=6)
        // floors: 166, 333, 500 → sum=999, leftover=1
        // remainders: 1000*1%6=4, 1000*2%6=2, 1000*3%6=0
        // index 0 has highest remainder → gets the extra stroop
        let r = assert_exact(&env, 1000, &[1, 2, 3], 6);
        assert_eq!(r.get(0), Some(167));
        assert_eq!(r.get(1), Some(333));
        assert_eq!(r.get(2), Some(500));
    }

    #[test]
    fn test_single_recipient() {
        let env = Env::default();
        let r = assert_exact(&env, 999, &[1], 1);
        assert_eq!(r.get(0), Some(999));
    }

    #[test]
    fn test_zero_total() {
        let env = Env::default();
        let r = assert_exact(&env, 0, &[1, 2, 3], 6);
        assert_eq!(r.get(0), Some(0));
        assert_eq!(r.get(1), Some(0));
        assert_eq!(r.get(2), Some(0));
    }

    #[test]
    fn test_large_amounts() {
        let env = Env::default();
        // 1_000_000_007 stroops across 7 equal recipients
        assert_exact(&env, 1_000_000_007, &[1, 1, 1, 1, 1, 1, 1], 7);
    }

    #[test]
    fn test_large_ratio_values() {
        let env = Env::default();
        // Real invoice: 1_000_000_000 stroops split 100_000:200_000:300_000
        assert_exact(&env, 1_000_000_000, &[100_000, 200_000, 300_000], 600_000);
    }

    /// Property-based style test: exhaustively verify sum == total for many inputs.
    #[test]
    fn test_property_sum_equals_total() {
        let env = Env::default();
        let cases: &[(i128, &[i128], i128)] = &[
            (1, &[1], 1),
            (2, &[1, 1], 2),
            (3, &[1, 1, 1], 3),
            (7, &[1, 2, 4], 7),
            (99, &[3, 5, 7, 11], 26),
            (1000, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10], 55),
            (1_000_000_000, &[1, 3, 5, 7], 16),
            (13, &[2, 3], 5),
            (10_000_007, &[1, 1, 1, 1, 1, 1, 1], 7),
            (1_000_000_000_000i128, &[333_333, 333_333, 333_334], 1_000_000),
        ];

        for &(total, ratios, denom) in cases {
            assert_exact(&env, total, ratios, denom);
        }
    }
}
