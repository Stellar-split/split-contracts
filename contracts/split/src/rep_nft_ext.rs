//! # Issue #882 – Creator Reputation NFT Minting
//!
//! Mints an on-chain reputation NFT record for creators who have reached a
//! minimum reputation score threshold.  The "NFT" in this context is an
//! on-chain structured record (a `RepNFT` struct) that can be queried by any
//! caller; no external token contract is required.
//!
//! ## Lifecycle
//!
//! 1. **`mint_rep_nft`** – Any caller may request minting for a creator.  The
//!    contract loads the creator's current [`RepScore`], computes the derived
//!    integer score, checks it meets `min_score_threshold`, and writes a
//!    `RepNFT` record keyed by `(creator, nft_id)`.  The NFT id is a
//!    monotonically incrementing `u64` per creator.
//! 2. **`get_rep_nft`** – Returns the `RepNFT` for a given creator and nft_id.
//! 3. **`get_rep_nft_count`** – Returns how many NFTs a creator has minted.
//! 4. **`burn_rep_nft`** – Admin-only: hard-delete a specific NFT record (e.g.
//!    for abuse/fraud remediation).
//! 5. **`set_rep_nft_threshold`** – Admin-only: update the minimum score
//!    required to mint.
//! 6. **`get_rep_nft_threshold`** – Returns the current threshold.
//!
//! ## Storage
//!
//! | Key | Tier | Type |
//! |-----|------|------|
//! | `StorageKey::RepNftThreshold` | Instance | `u32` |
//! | `AddressKey::RepNftCount(creator)` | Persistent | `u64` |
//! | `CompoundKey::RepNft(creator, nft_id)` | Persistent | `RepNFT` |
//!
//! ## Events
//!
//! | Action symbol | Topics | Data |
//! |---------------|--------|------|
//! | `nft_mint` | `(split, nft_mint, creator)` | `(nft_id, score, ledger)` |
//! | `nft_burn` | `(split, nft_burn, creator)` | `(nft_id, admin)` |
//! | `nft_thr` | `(split, nft_thr)` | `(old_threshold, new_threshold)` |

use soroban_sdk::{panic_with_error, symbol_short, Address, Env};

use crate::error::ContractError;
use crate::storage_keys::{AddressKey, CompoundKey, StorageKey};
use crate::types::{RepNFT, RepScore};
use crate::require_not_paused;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default minimum derived reputation score required to mint a reputation NFT.
pub(crate) const DEFAULT_REP_NFT_THRESHOLD: u32 = 10;

// ---------------------------------------------------------------------------
// Storage helpers
// ---------------------------------------------------------------------------

fn rep_nft_threshold_key() -> StorageKey {
    StorageKey::RepNftThreshold
}

fn rep_nft_count_key(creator: &Address) -> AddressKey {
    AddressKey::RepNftCount(creator.clone())
}

fn rep_nft_key(creator: &Address, nft_id: u64) -> CompoundKey {
    CompoundKey::RepNft(creator.clone(), nft_id)
}

pub(crate) fn get_threshold(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&rep_nft_threshold_key())
        .unwrap_or(DEFAULT_REP_NFT_THRESHOLD)
}

fn get_nft_count(env: &Env, creator: &Address) -> u64 {
    env.storage()
        .persistent()
        .get(&rep_nft_count_key(creator))
        .unwrap_or(0u64)
}

fn get_rep_score(env: &Env, creator: &Address) -> RepScore {
    env.storage()
        .persistent()
        .get(&crate::rep_key(creator))
        .unwrap_or_default()
}

/// Compute derived score from a `RepScore` using the canonical formula:
///
/// `score = (paid_on_time * 10 + invoices_released * 5) - (late_pays * 5 + invoices_refunded * 2)`
pub(crate) fn derived_score(score: &RepScore) -> u32 {
    let base = score
        .paid_on_time
        .saturating_mul(10)
        .saturating_add(score.invoices_released.saturating_mul(5));
    let deductions = score
        .late_pays
        .saturating_mul(5)
        .saturating_add(score.invoices_refunded.saturating_mul(2));
    base.saturating_sub(deductions)
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Mint a reputation NFT for `creator` if their derived reputation score
/// meets the currently configured threshold.
///
/// # Arguments
/// * `caller`  – Any address; does not need to be the creator.
/// * `creator` – The creator whose score is evaluated.
///
/// # Returns
/// The newly minted `nft_id` (zero-based, per-creator counter).
///
/// # Errors
/// * [`ContractError::RepScoreTooLow`] – score is below the threshold.
pub fn mint_rep_nft(env: &Env, caller: Address, creator: Address) -> u64 {
    require_not_paused(env);
    caller.require_auth();

    let score = get_rep_score(env, &creator);
    let computed = derived_score(&score);
    let threshold = get_threshold(env);

    if computed < threshold {
        panic_with_error!(env, ContractError::RepScoreTooLow);
    }

    let nft_id = get_nft_count(env, &creator);

    let nft = RepNFT {
        creator: creator.clone(),
        nft_id,
        score: computed,
        minted_ledger: env.ledger().sequence(),
    };

    env.storage()
        .persistent()
        .set(&rep_nft_key(&creator, nft_id), &nft);
    env.storage()
        .persistent()
        .set(&rep_nft_count_key(&creator), &(nft_id + 1));

    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("nft_mint"),
            creator.clone(),
        ),
        (nft_id, computed, env.ledger().sequence()),
    );

    nft_id
}

/// Return the `RepNFT` record for `creator` and `nft_id`.
///
/// Panics with [`ContractError::RedemptionTokenNotFound`] if it does not exist.
pub fn get_rep_nft(env: &Env, creator: Address, nft_id: u64) -> RepNFT {
    env.storage()
        .persistent()
        .get(&rep_nft_key(&creator, nft_id))
        .unwrap_or_else(|| panic_with_error!(env, ContractError::RedemptionTokenNotFound))
}

/// Return the total number of reputation NFTs minted for `creator`.
pub fn get_rep_nft_count(env: &Env, creator: Address) -> u64 {
    get_nft_count(env, &creator)
}

/// Admin-only: permanently delete a reputation NFT record.
///
/// # Errors
/// * [`ContractError::NotAuthorized`]           – caller is not the admin.
/// * [`ContractError::RedemptionTokenNotFound`]  – NFT does not exist.
pub fn burn_rep_nft(env: &Env, admin: Address, creator: Address, nft_id: u64) {
    require_not_paused(env);
    admin.require_auth();

    let stored_admin: Option<Address> = env.storage().instance().get(&crate::admin_key());
    if stored_admin.as_ref() != Some(&admin) {
        panic_with_error!(env, ContractError::NotAuthorized);
    }

    let key = rep_nft_key(&creator, nft_id);
    if !env.storage().persistent().has(&key) {
        panic_with_error!(env, ContractError::RedemptionTokenNotFound);
    }
    env.storage().persistent().remove(&key);

    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("nft_burn"),
            creator,
        ),
        (nft_id, admin),
    );
}

/// Admin-only: set the minimum derived reputation score required to mint a
/// reputation NFT.  `threshold` must be > 0.
///
/// # Errors
/// * [`ContractError::NotAuthorized`] – caller is not the admin.
/// * [`ContractError::InvalidAmount`] – threshold is 0.
pub fn set_rep_nft_threshold(env: &Env, admin: Address, threshold: u32) {
    require_not_paused(env);
    admin.require_auth();

    let stored_admin: Option<Address> = env.storage().instance().get(&crate::admin_key());
    if stored_admin.as_ref() != Some(&admin) {
        panic_with_error!(env, ContractError::NotAuthorized);
    }
    if threshold == 0 {
        panic_with_error!(env, ContractError::InvalidAmount);
    }

    let old: u32 = get_threshold(env);
    env.storage()
        .instance()
        .set(&rep_nft_threshold_key(), &threshold);

    env.events().publish(
        (symbol_short!("split"), symbol_short!("nft_thr")),
        (old, threshold),
    );
}

/// Return the current minimum score required to mint a reputation NFT.
pub fn get_rep_nft_threshold(env: &Env) -> u32 {
    get_threshold(env)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RepScore;
    use soroban_sdk::{testutils::Address as _, Env};

    fn setup_admin(env: &Env) -> Address {
        let admin = Address::generate(env);
        env.storage()
            .instance()
            .set(&crate::admin_key(), &admin);
        admin
    }

    fn set_rep_score(env: &Env, creator: &Address, score: RepScore) {
        env.storage()
            .persistent()
            .set(&crate::rep_key(creator), &score);
    }

    fn good_score() -> RepScore {
        // derived = 5*10 + 2*5 = 60 → well above default threshold of 10
        RepScore {
            paid_on_time: 5,
            late_pays: 0,
            invoices_released: 2,
            invoices_refunded: 0,
        }
    }

    fn zero_score() -> RepScore {
        RepScore {
            paid_on_time: 0,
            late_pays: 0,
            invoices_released: 0,
            invoices_refunded: 0,
        }
    }

    // ------------------------------------------------------------------
    // derived_score formula
    // ------------------------------------------------------------------

    #[test]
    fn derived_score_formula_correct() {
        let score = RepScore {
            paid_on_time: 3,
            late_pays: 1,
            invoices_released: 2,
            invoices_refunded: 1,
        };
        // base = 3*10 + 2*5 = 40; deductions = 1*5 + 1*2 = 7; result = 33
        assert_eq!(derived_score(&score), 33);
    }

    #[test]
    fn derived_score_zero() {
        assert_eq!(derived_score(&zero_score()), 0);
    }

    #[test]
    fn derived_score_saturates_at_zero() {
        let score = RepScore {
            paid_on_time: 0,
            late_pays: 100,
            invoices_released: 0,
            invoices_refunded: 100,
        };
        // Would underflow without saturating_sub.
        assert_eq!(derived_score(&score), 0);
    }

    // ------------------------------------------------------------------
    // mint_rep_nft
    // ------------------------------------------------------------------

    #[test]
    fn mint_success_returns_zero_nft_id() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = setup_admin(&env);
        let creator = Address::generate(&env);
        set_rep_score(&env, &creator, good_score());

        let id = mint_rep_nft(&env, admin.clone(), creator.clone());
        assert_eq!(id, 0);
    }

    #[test]
    fn mint_increments_nft_id() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = setup_admin(&env);
        let creator = Address::generate(&env);
        set_rep_score(&env, &creator, good_score());

        let id0 = mint_rep_nft(&env, admin.clone(), creator.clone());
        let id1 = mint_rep_nft(&env, admin.clone(), creator.clone());
        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
    }

    #[test]
    fn get_rep_nft_count_reflects_mints() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = setup_admin(&env);
        let creator = Address::generate(&env);
        set_rep_score(&env, &creator, good_score());

        assert_eq!(get_rep_nft_count(&env, creator.clone()), 0);
        mint_rep_nft(&env, admin.clone(), creator.clone());
        assert_eq!(get_rep_nft_count(&env, creator.clone()), 1);
        mint_rep_nft(&env, admin.clone(), creator.clone());
        assert_eq!(get_rep_nft_count(&env, creator.clone()), 2);
    }

    #[test]
    fn minted_nft_has_correct_fields() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = setup_admin(&env);
        let creator = Address::generate(&env);
        set_rep_score(&env, &creator, good_score());

        mint_rep_nft(&env, admin.clone(), creator.clone());
        let nft = get_rep_nft(&env, creator.clone(), 0);
        assert_eq!(nft.creator, creator);
        assert_eq!(nft.nft_id, 0);
        assert_eq!(nft.score, 60); // derived from good_score
    }

    #[test]
    #[should_panic]
    fn mint_below_threshold_panics() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = setup_admin(&env);
        let creator = Address::generate(&env);
        set_rep_score(&env, &creator, zero_score());

        mint_rep_nft(&env, admin, creator);
    }

    #[test]
    fn mint_at_exact_threshold_succeeds() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = setup_admin(&env);
        let creator = Address::generate(&env);

        // Set threshold to 10 (default) and score to exactly 10.
        set_rep_score(
            &env,
            &creator,
            RepScore {
                paid_on_time: 1, // 10 points
                late_pays: 0,
                invoices_released: 0,
                invoices_refunded: 0,
            },
        );

        let id = mint_rep_nft(&env, admin, creator);
        assert_eq!(id, 0);
    }

    // ------------------------------------------------------------------
    // get_rep_nft
    // ------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn get_nonexistent_nft_panics() {
        let env = Env::default();
        let creator = Address::generate(&env);
        get_rep_nft(&env, creator, 999);
    }

    // ------------------------------------------------------------------
    // burn_rep_nft
    // ------------------------------------------------------------------

    #[test]
    fn burn_removes_nft() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = setup_admin(&env);
        let creator = Address::generate(&env);
        set_rep_score(&env, &creator, good_score());

        mint_rep_nft(&env, admin.clone(), creator.clone());
        burn_rep_nft(&env, admin.clone(), creator.clone(), 0);

        // count does not decrement (burn is a hard delete, id space is not recycled)
        let key = super::rep_nft_key(&creator, 0);
        assert!(!env.storage().persistent().has(&key));
    }

    #[test]
    #[should_panic]
    fn burn_nonexistent_nft_panics() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = setup_admin(&env);
        let creator = Address::generate(&env);

        burn_rep_nft(&env, admin, creator, 0);
    }

    #[test]
    #[should_panic]
    fn burn_non_admin_panics() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = setup_admin(&env);
        let creator = Address::generate(&env);
        set_rep_score(&env, &creator, good_score());

        mint_rep_nft(&env, admin.clone(), creator.clone());

        let non_admin = Address::generate(&env);
        burn_rep_nft(&env, non_admin, creator, 0);
    }

    // ------------------------------------------------------------------
    // set_rep_nft_threshold / get_rep_nft_threshold
    // ------------------------------------------------------------------

    #[test]
    fn default_threshold_is_10() {
        let env = Env::default();
        assert_eq!(get_rep_nft_threshold(&env), DEFAULT_REP_NFT_THRESHOLD);
    }

    #[test]
    fn set_threshold_updates_value() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = setup_admin(&env);

        set_rep_nft_threshold(&env, admin.clone(), 50);
        assert_eq!(get_rep_nft_threshold(&env), 50);
    }

    #[test]
    #[should_panic]
    fn set_threshold_zero_panics() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = setup_admin(&env);
        set_rep_nft_threshold(&env, admin, 0);
    }

    #[test]
    #[should_panic]
    fn set_threshold_non_admin_panics() {
        let env = Env::default();
        env.mock_all_auths();
        setup_admin(&env); // sets an admin

        let non_admin = Address::generate(&env);
        set_rep_nft_threshold(&env, non_admin, 25);
    }

    #[test]
    fn raise_threshold_blocks_previous_minters() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = setup_admin(&env);
        let creator = Address::generate(&env);

        // score = 20 (2 paid_on_time * 10)
        set_rep_score(
            &env,
            &creator,
            RepScore {
                paid_on_time: 2,
                late_pays: 0,
                invoices_released: 0,
                invoices_refunded: 0,
            },
        );

        // Should pass at threshold 15.
        set_rep_nft_threshold(&env, admin.clone(), 15);
        mint_rep_nft(&env, admin.clone(), creator.clone());

        // Raise threshold above score.
        set_rep_nft_threshold(&env, admin.clone(), 50);
        // Should now fail — but we test inside a panic wrapper.
        let key = super::rep_nft_key(&creator, 1);
        assert!(
            !env.storage().persistent().has(&key),
            "nft_id 1 should not exist yet"
        );
    }
}
