//! Issue #877: decentralized invoice rating oracle.
//!
//! Any address that has paid an invoice may rate it once on a 1–5 scale.
//! Ratings are aggregated (sum + count) so the contract can return the
//! running average without storing every individual vote. An optional
//! `comment` of ≤ 280 bytes may accompany a rating.
//!
//! Oracle nodes (registered by the admin) may also push an external
//! "oracle rating" for any invoice — this is stored separately from
//! payer ratings and is meant to represent an off-chain signal such as a
//! credit-score feed. Oracle ratings replace the previous oracle value
//! for an invoice on every push.
//!
//! ## Entry points
//! - `register_rating_oracle(admin, oracle)` — admin adds an oracle node.
//! - `remove_rating_oracle(admin, oracle)` — admin removes an oracle node.
//! - `rate_invoice(payer, invoice_id, score, comment)` — payer rates 1–5.
//! - `push_oracle_rating(oracle, invoice_id, score)` — oracle node pushes.
//! - `get_invoice_rating(invoice_id)` — returns `InvoiceRatingSummary`.
//! - `get_oracle_rating(invoice_id)` — returns `Option<u32>` oracle score.
//! - `is_rating_oracle(oracle)` — check oracle registration.
//!
//! ## Events
//! - `(split, ora_add)` — oracle registered
//! - `(split, ora_rem)` — oracle removed
//! - `(split, inv_rat, invoice_id)` — payer rated
//! - `(split, ora_rat, invoice_id)` — oracle pushed rating
//!
//! ## Storage
//! Uses a local `RatingOracleKey` enum in **persistent** storage and the
//! oracle-nodes list in **instance** storage.

use super::*;
use soroban_sdk::{contractimpl, contracttype, symbol_short, Address, Env, String, Vec};

/// Maximum score value.
pub const MAX_RATING_SCORE: u32 = 5;
/// Minimum score value.
pub const MIN_RATING_SCORE: u32 = 1;
/// Maximum comment length in bytes.
pub const MAX_RATING_COMMENT_LEN: u32 = 280;

/// Aggregate rating data stored per invoice.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct InvoiceRatingSummary {
    /// Total of all scores submitted by payers.
    pub score_sum: u64,
    /// Number of payer ratings submitted.
    pub count: u32,
    /// List of rater addresses (to prevent double-voting).
    pub raters: Vec<Address>,
}

/// Persistent storage keys for the rating oracle module.
#[contracttype]
#[derive(Clone)]
pub enum RatingOracleKey {
    /// `InvoiceRatingSummary` aggregate for an invoice.
    RatingSummary(u64),
    /// `u32` oracle score for an invoice (latest push wins).
    OracleScore(u64),
    /// `Vec<Address>` registered oracle nodes (instance storage).
    OracleNodes,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn load_summary(env: &Env, invoice_id: u64) -> InvoiceRatingSummary {
    env.storage()
        .persistent()
        .get(&RatingOracleKey::RatingSummary(invoice_id))
        .unwrap_or_else(|| InvoiceRatingSummary {
            score_sum: 0,
            count: 0,
            raters: Vec::new(env),
        })
}

fn load_oracle_nodes(env: &Env) -> Vec<Address> {
    env.storage()
        .instance()
        .get(&RatingOracleKey::OracleNodes)
        .unwrap_or_else(|| Vec::new(env))
}

fn save_oracle_nodes(env: &Env, list: &Vec<Address>) {
    env.storage()
        .instance()
        .set(&RatingOracleKey::OracleNodes, list);
}

fn require_admin(env: &Env, caller: &Address) {
    let admin: Address = env
        .storage()
        .instance()
        .get(&symbol_short!("admin"))
        .expect("contract not initialised");
    assert!(*caller == admin, "only admin can manage oracle nodes");
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

#[contractimpl]
impl SplitContract {
    /// Register `oracle` as a rating oracle node (admin only).
    pub fn register_rating_oracle(env: Env, admin: Address, oracle: Address) {
        require_not_paused(&env);
        admin.require_auth();
        require_admin(&env, &admin);

        let mut nodes = load_oracle_nodes(&env);
        assert!(!nodes.contains(oracle.clone()), "oracle already registered");
        nodes.push_back(oracle.clone());
        save_oracle_nodes(&env, &nodes);

        env.events().publish(
            (symbol_short!("split"), symbol_short!("ora_add")),
            oracle,
        );
    }

    /// Remove `oracle` from the oracle node set (admin only).
    pub fn remove_rating_oracle(env: Env, admin: Address, oracle: Address) {
        require_not_paused(&env);
        admin.require_auth();
        require_admin(&env, &admin);

        let nodes = load_oracle_nodes(&env);
        let mut new_nodes = Vec::new(&env);
        let mut found = false;
        for n in nodes.iter() {
            if n == oracle {
                found = true;
            } else {
                new_nodes.push_back(n);
            }
        }
        assert!(found, "oracle not registered");
        save_oracle_nodes(&env, &new_nodes);

        env.events().publish(
            (symbol_short!("split"), symbol_short!("ora_rem")),
            oracle,
        );
    }

    /// Returns `true` if `oracle` is a registered rating oracle node.
    pub fn is_rating_oracle(env: Env, oracle: Address) -> bool {
        load_oracle_nodes(&env).contains(oracle)
    }

    /// Rate an invoice as a payer.
    ///
    /// - `score` must be in `[1, 5]`.
    /// - The caller must have paid the invoice at least once.
    /// - Each payer may rate the invoice only once.
    /// - `comment` (if provided) must be ≤ 280 bytes.
    pub fn rate_invoice(
        env: Env,
        payer: Address,
        invoice_id: u64,
        score: u32,
        comment: Option<String>,
    ) {
        require_not_paused(&env);
        payer.require_auth();

        assert!(
            score >= MIN_RATING_SCORE && score <= MAX_RATING_SCORE,
            "score must be between 1 and 5"
        );
        if let Some(ref c) = comment {
            assert!(c.len() <= MAX_RATING_COMMENT_LEN, "comment too long");
        }

        let invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.payments.iter().any(|p| p.payer == payer),
            "only payers can rate an invoice"
        );

        let mut summary = load_summary(&env, invoice_id);
        assert!(
            !summary.raters.contains(payer.clone()),
            "payer has already rated this invoice"
        );

        summary.score_sum += score as u64;
        summary.count += 1;
        summary.raters.push_back(payer.clone());

        env.storage()
            .persistent()
            .set(&RatingOracleKey::RatingSummary(invoice_id), &summary);

        env.events().publish(
            (symbol_short!("split"), symbol_short!("inv_rat"), invoice_id),
            (payer, score, comment),
        );
    }

    /// Push an oracle rating for an invoice (oracle node only).
    ///
    /// The latest push overwrites any prior oracle score. `score` must be in
    /// `[1, 5]`.
    pub fn push_oracle_rating(env: Env, oracle: Address, invoice_id: u64, score: u32) {
        require_not_paused(&env);
        oracle.require_auth();

        assert!(
            score >= MIN_RATING_SCORE && score <= MAX_RATING_SCORE,
            "score must be between 1 and 5"
        );

        let nodes = load_oracle_nodes(&env);
        assert!(nodes.contains(oracle.clone()), "caller is not an oracle node");

        // Verify the invoice exists.
        load_invoice(&env, invoice_id);

        env.storage()
            .persistent()
            .set(&RatingOracleKey::OracleScore(invoice_id), &score);

        env.events().publish(
            (symbol_short!("split"), symbol_short!("ora_rat"), invoice_id),
            (oracle, score),
        );
    }

    /// Returns the aggregate payer rating summary for `invoice_id`.
    pub fn get_invoice_rating(env: Env, invoice_id: u64) -> InvoiceRatingSummary {
        load_summary(&env, invoice_id)
    }

    /// Returns the latest oracle-pushed score for `invoice_id`, or `None`
    /// if no oracle has rated it yet.
    pub fn get_oracle_rating(env: Env, invoice_id: u64) -> Option<u32> {
        env.storage()
            .persistent()
            .get(&RatingOracleKey::OracleScore(invoice_id))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ext_test_util::{fixture, mint, new_invoice, pay};
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::Address;

    #[test]
    fn payer_can_rate_invoice() {
        let f = fixture();
        let (id, _, _) = new_invoice(&f, 100);
        let p = Address::generate(&f.env);
        mint(&f.env, &f.token, &p, 50);
        pay(&f, &p, id, 50);
        f.c.rate_invoice(&p, &id, &4u32, &None);
        let summary = f.c.get_invoice_rating(&id);
        assert_eq!(summary.count, 1);
        assert_eq!(summary.score_sum, 4);
    }

    #[test]
    #[should_panic(expected = "only payers can rate an invoice")]
    fn non_payer_cannot_rate() {
        let f = fixture();
        let (id, _, _) = new_invoice(&f, 100);
        let stranger = Address::generate(&f.env);
        f.c.rate_invoice(&stranger, &id, &3u32, &None);
    }

    #[test]
    #[should_panic(expected = "payer has already rated this invoice")]
    fn double_rating_rejected() {
        let f = fixture();
        let (id, _, _) = new_invoice(&f, 100);
        let p = Address::generate(&f.env);
        mint(&f.env, &f.token, &p, 50);
        pay(&f, &p, id, 50);
        f.c.rate_invoice(&p, &id, &5u32, &None);
        f.c.rate_invoice(&p, &id, &3u32, &None);
    }

    #[test]
    #[should_panic(expected = "score must be between 1 and 5")]
    fn invalid_score_rejected() {
        let f = fixture();
        let (id, _, _) = new_invoice(&f, 100);
        let p = Address::generate(&f.env);
        mint(&f.env, &f.token, &p, 50);
        pay(&f, &p, id, 50);
        f.c.rate_invoice(&p, &id, &0u32, &None);
    }

    #[test]
    fn oracle_can_push_rating() {
        let f = fixture();
        let (id, _, _) = new_invoice(&f, 100);
        let oracle = Address::generate(&f.env);
        f.c.register_rating_oracle(&f.admin, &oracle);
        f.c.push_oracle_rating(&oracle, &id, &4u32);
        assert_eq!(f.c.get_oracle_rating(&id), Some(4u32));
    }

    #[test]
    #[should_panic(expected = "caller is not an oracle node")]
    fn non_oracle_cannot_push_rating() {
        let f = fixture();
        let (id, _, _) = new_invoice(&f, 100);
        let stranger = Address::generate(&f.env);
        f.c.push_oracle_rating(&stranger, &id, &3u32);
    }

    #[test]
    fn oracle_rating_defaults_to_none() {
        let f = fixture();
        let (id, _, _) = new_invoice(&f, 100);
        assert_eq!(f.c.get_oracle_rating(&id), None);
    }

    #[test]
    fn oracle_rating_is_overwritten_on_repush() {
        let f = fixture();
        let (id, _, _) = new_invoice(&f, 100);
        let oracle = Address::generate(&f.env);
        f.c.register_rating_oracle(&f.admin, &oracle);
        f.c.push_oracle_rating(&oracle, &id, &2u32);
        f.c.push_oracle_rating(&oracle, &id, &5u32);
        assert_eq!(f.c.get_oracle_rating(&id), Some(5u32));
    }

    #[test]
    fn register_and_remove_oracle() {
        let f = fixture();
        let oracle = Address::generate(&f.env);
        f.c.register_rating_oracle(&f.admin, &oracle);
        assert!(f.c.is_rating_oracle(&oracle));
        f.c.remove_rating_oracle(&f.admin, &oracle);
        assert!(!f.c.is_rating_oracle(&oracle));
    }

    #[test]
    fn multiple_payers_aggregate_correctly() {
        let f = fixture();
        let (id, _, _) = new_invoice(&f, 100);
        for score in [3u32, 4, 5] {
            let p = Address::generate(&f.env);
            mint(&f.env, &f.token, &p, 10);
            pay(&f, &p, id, 10);
            f.c.rate_invoice(&p, &id, &score, &None);
        }
        let summary = f.c.get_invoice_rating(&id);
        assert_eq!(summary.count, 3);
        assert_eq!(summary.score_sum, 12); // 3+4+5
    }
}
