//! Issue #919: Volume-based fee tier system for creators.
//!
//! High-volume creators earn a lower fee rate. The admin publishes an ordered
//! tier table; each creator is placed in the highest tier whose `min_volume`
//! their lifetime volume meets. Lifetime volume is the per-creator total that
//! the contract already accumulates on every payment
//! (`creator_stats_volume_key`, issue #299), so no extra bookkeeping happens on
//! the payment hot path.
//!
//! ## Config rules
//! * 1 – `MAX_CREATOR_FEE_TIERS` tiers.
//! * `tier_id` equals the tier's position (0, 1, 2, ...).
//! * The first tier starts at `min_volume == 0`, so every creator has a tier.
//! * `min_volume` is strictly ascending.
//! * `fee_bps` never increases from one tier to the next and is at most
//!   `MAX_CREATOR_FEE_BPS`.
//!
//! Each successful update bumps the config `version`, which lets tracked
//! tiers detect that they were computed against an older table.
//!
//! ## Lookup
//! Tier lookup is a binary search over the (sorted) table — O(log n) with a
//! single instance-storage read for the table plus one persistent read for
//! the creator's volume.
//!
//! ## Storage
//! * `cfee_cfg` → [`CreatorFeeTierConfig`] (instance)
//! * `(cfee_trk, creator)` → [`CreatorTierRecord`] (persistent)

use soroban_sdk::{contractimpl, contracttype, symbol_short, Address, Env, Symbol, Vec};

use crate::events;
use crate::types::AdminRole;
use crate::{
    creator_stats_volume_key, require_admin_role, SplitContract, SplitContractArgs,
    SplitContractClient,
};

/// Maximum number of tiers in the table.
pub const MAX_CREATOR_FEE_TIERS: u32 = 10;

/// Highest fee rate any tier may charge (10%).
pub const MAX_CREATOR_FEE_BPS: u32 = 1_000;

const BPS_DENOMINATOR: i128 = 10_000;

/// One row of the creator fee tier table.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatorFeeTier {
    /// Position of the tier in the table (0-based).
    pub tier_id: u32,
    /// Human-readable label, e.g. `bronze`, `silver`, `gold`.
    pub name: Symbol,
    /// Minimum lifetime creator volume (inclusive) to qualify.
    pub min_volume: i128,
    /// Fee rate charged to creators in this tier, in basis points.
    pub fee_bps: u32,
}

/// The full tier table plus versioning metadata.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatorFeeTierConfig {
    pub tiers: Vec<CreatorFeeTier>,
    /// Incremented on every successful update.
    pub version: u32,
    /// Ledger timestamp of the last update.
    pub updated_at: u64,
}

/// A creator's current tier placement.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatorFeeTierStatus {
    pub creator: Address,
    pub tier_id: u32,
    pub tier_name: Symbol,
    pub fee_bps: u32,
    /// Lifetime volume the placement was computed from.
    pub volume: i128,
    /// `min_volume` of the next tier up, or `None` at the top tier.
    pub next_tier_min_volume: Option<i128>,
    /// Additional volume needed to reach the next tier (0 at the top tier).
    pub volume_to_next_tier: i128,
}

/// Result of applying a creator's tier rate to an amount.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatorFeeQuote {
    pub tier_id: u32,
    pub fee_bps: u32,
    pub gross_amount: i128,
    pub fee: i128,
    pub net_amount: i128,
}

/// Last tier recorded for a creator by `sync_creator_fee_tier`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatorTierRecord {
    pub tier_id: u32,
    pub fee_bps: u32,
    /// Config version the tier was computed against.
    pub config_version: u32,
    /// Ledger timestamp when the creator entered this tier.
    pub since: u64,
}

fn creator_fee_config_key() -> Symbol {
    symbol_short!("cfee_cfg")
}

fn creator_tier_record_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("cfee_trk"), creator.clone())
}

/// Validate a tier table against the config rules in the module docs.
fn validate_tiers(tiers: &Vec<CreatorFeeTier>) {
    assert!(!tiers.is_empty(), "at least one fee tier is required");
    assert!(
        tiers.len() <= MAX_CREATOR_FEE_TIERS,
        "too many fee tiers"
    );

    let first = tiers.get(0).unwrap();
    assert!(first.min_volume == 0, "first fee tier must start at zero volume");

    for i in 0..tiers.len() {
        let tier = tiers.get(i).unwrap();
        assert!(tier.tier_id == i, "tier_id must match tier position");
        assert!(tier.fee_bps <= MAX_CREATOR_FEE_BPS, "tier fee_bps exceeds maximum");
        if i > 0 {
            let prev = tiers.get(i - 1).unwrap();
            assert!(
                tier.min_volume > prev.min_volume,
                "tier min_volume must be strictly ascending"
            );
            assert!(
                tier.fee_bps <= prev.fee_bps,
                "tier fee_bps must not increase with volume"
            );
        }
    }
}

fn load_config(env: &Env) -> Option<CreatorFeeTierConfig> {
    env.storage().instance().get(&creator_fee_config_key())
}

fn store_config(env: &Env, tiers: Vec<CreatorFeeTier>) -> u32 {
    let version = load_config(env).map(|c| c.version).unwrap_or(0) + 1;
    let tier_count = tiers.len();
    let config = CreatorFeeTierConfig {
        tiers,
        version,
        updated_at: env.ledger().timestamp(),
    };
    env.storage().instance().set(&creator_fee_config_key(), &config);
    events::creator_fee_tiers_configured(env, version, tier_count);
    version
}

/// Lifetime volume accumulated for `creator` (issue #299 counter).
fn creator_volume(env: &Env, creator: &Address) -> i128 {
    env.storage()
        .persistent()
        .get::<_, u64>(&creator_stats_volume_key(creator))
        .unwrap_or(0) as i128
}

/// Index of the highest tier whose `min_volume <= volume`. Binary search over
/// a validated (ascending) table; tier 0 always matches since it starts at 0.
fn tier_index_for_volume(tiers: &Vec<CreatorFeeTier>, volume: i128) -> u32 {
    let mut lo: u32 = 0;
    let mut hi: u32 = tiers.len() - 1;
    while lo < hi {
        let mid = lo + (hi - lo + 1) / 2;
        if tiers.get(mid).unwrap().min_volume <= volume {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}

/// Compute the creator's tier status. Falls back to a single implicit tier at
/// the platform fee when no table has been configured.
fn resolve_status(env: &Env, creator: &Address) -> CreatorFeeTierStatus {
    let volume = creator_volume(env, creator);
    match load_config(env) {
        Some(config) => {
            let idx = tier_index_for_volume(&config.tiers, volume);
            let tier = config.tiers.get(idx).unwrap();
            let next_tier_min_volume = config.tiers.get(idx + 1).map(|t| t.min_volume);
            let volume_to_next_tier = next_tier_min_volume
                .map(|min| (min - volume).max(0))
                .unwrap_or(0);
            CreatorFeeTierStatus {
                creator: creator.clone(),
                tier_id: tier.tier_id,
                tier_name: tier.name,
                fee_bps: tier.fee_bps,
                volume,
                next_tier_min_volume,
                volume_to_next_tier,
            }
        }
        None => CreatorFeeTierStatus {
            creator: creator.clone(),
            tier_id: 0,
            tier_name: symbol_short!("default"),
            fee_bps: SplitContract::get_platform_fee_bps(env.clone()),
            volume,
            next_tier_min_volume: None,
            volume_to_next_tier: 0,
        },
    }
}

/// Apply `fee_bps` to `amount`, rounding the fee down.
fn apply_fee(amount: i128, fee_bps: u32) -> i128 {
    amount
        .checked_mul(fee_bps as i128)
        .expect("fee calculation overflow")
        / BPS_DENOMINATOR
}

#[contractimpl]
impl SplitContract {
    /// Issue #919: Replace the creator fee tier table. SuperAdmin only.
    /// Returns the new config version.
    pub fn set_creator_fee_tiers(env: Env, admin: Address, tiers: Vec<CreatorFeeTier>) -> u32 {
        require_admin_role(&env, &admin, AdminRole::SuperAdmin);
        validate_tiers(&tiers);
        store_config(&env, tiers)
    }

    /// Issue #919: Change the fee rate of a single existing tier, keeping the
    /// rest of the table. The updated table is re-validated. SuperAdmin only.
    /// Returns the new config version.
    pub fn update_creator_fee_tier_bps(env: Env, admin: Address, tier_id: u32, fee_bps: u32) -> u32 {
        require_admin_role(&env, &admin, AdminRole::SuperAdmin);
        let config = load_config(&env).expect("creator fee tiers not configured");
        let mut tiers = config.tiers;
        let mut tier = tiers.get(tier_id).expect("unknown fee tier");
        tier.fee_bps = fee_bps;
        tiers.set(tier_id, tier);
        validate_tiers(&tiers);
        store_config(&env, tiers)
    }

    /// Issue #919: Remove the tier table; creators fall back to the platform fee.
    /// SuperAdmin only.
    pub fn clear_creator_fee_tiers(env: Env, admin: Address) {
        require_admin_role(&env, &admin, AdminRole::SuperAdmin);
        env.storage().instance().remove(&creator_fee_config_key());
        events::creator_fee_tiers_configured(&env, 0, 0);
    }

    /// Issue #919: The current tier table, if configured.
    pub fn get_creator_fee_tier_config(env: Env) -> Option<CreatorFeeTierConfig> {
        load_config(&env)
    }

    /// Issue #919: The tier a creator currently qualifies for, based on their
    /// lifetime volume.
    pub fn get_creator_fee_tier(env: Env, creator: Address) -> CreatorFeeTierStatus {
        resolve_status(&env, &creator)
    }

    /// Issue #919: Fee owed by `creator` on `amount` at their current tier rate.
    pub fn calc_creator_fee(env: Env, creator: Address, amount: i128) -> CreatorFeeQuote {
        assert!(amount > 0, "amount must be positive");
        let status = resolve_status(&env, &creator);
        let fee = apply_fee(amount, status.fee_bps);
        CreatorFeeQuote {
            tier_id: status.tier_id,
            fee_bps: status.fee_bps,
            gross_amount: amount,
            fee,
            net_amount: amount - fee,
        }
    }

    /// Issue #919: Recompute a creator's tier and record it. Emits a
    /// `cfee_chg` event whenever the tier (or its rate) differs from the last
    /// recorded one. Permissionless — the result depends only on on-chain state.
    pub fn sync_creator_fee_tier(env: Env, creator: Address) -> CreatorTierRecord {
        let status = resolve_status(&env, &creator);
        let config_version = load_config(&env).map(|c| c.version).unwrap_or(0);
        let key = creator_tier_record_key(&creator);
        let previous: Option<CreatorTierRecord> = env.storage().persistent().get(&key);

        let changed = match previous {
            Some(ref prev) => prev.tier_id != status.tier_id || prev.fee_bps != status.fee_bps,
            None => true,
        };

        let record = CreatorTierRecord {
            tier_id: status.tier_id,
            fee_bps: status.fee_bps,
            config_version,
            since: match previous {
                Some(ref prev) if !changed => prev.since,
                _ => env.ledger().timestamp(),
            },
        };

        if changed {
            let old_tier = previous.as_ref().map(|p| p.tier_id);
            events::creator_fee_tier_changed(
                &env,
                &creator,
                old_tier,
                status.tier_id,
                status.fee_bps,
                status.volume,
            );
        }
        if changed || previous.map(|p| p.config_version) != Some(config_version) {
            env.storage().persistent().set(&key, &record);
        }
        record
    }

    /// Issue #919: Last tier recorded for a creator by `sync_creator_fee_tier`.
    pub fn get_creator_tracked_tier(env: Env, creator: Address) -> Option<CreatorTierRecord> {
        env.storage()
            .persistent()
            .get(&creator_tier_record_key(&creator))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tier(env: &Env, tier_id: u32, name: &str, min_volume: i128, fee_bps: u32) -> CreatorFeeTier {
        CreatorFeeTier {
            tier_id,
            name: Symbol::new(env, name),
            min_volume,
            fee_bps,
        }
    }

    fn sample_tiers(env: &Env) -> Vec<CreatorFeeTier> {
        Vec::from_array(
            env,
            [
                tier(env, 0, "bronze", 0, 300),
                tier(env, 1, "silver", 10_000, 200),
                tier(env, 2, "gold", 100_000, 100),
            ],
        )
    }

    #[test]
    fn test_tier_index_for_volume() {
        let env = Env::default();
        let tiers = sample_tiers(&env);
        assert_eq!(tier_index_for_volume(&tiers, 0), 0);
        assert_eq!(tier_index_for_volume(&tiers, 9_999), 0);
        assert_eq!(tier_index_for_volume(&tiers, 10_000), 1);
        assert_eq!(tier_index_for_volume(&tiers, 99_999), 1);
        assert_eq!(tier_index_for_volume(&tiers, 100_000), 2);
        assert_eq!(tier_index_for_volume(&tiers, i128::MAX), 2);
    }

    #[test]
    fn test_tier_index_single_tier() {
        let env = Env::default();
        let tiers = Vec::from_array(&env, [tier(&env, 0, "flat", 0, 250)]);
        assert_eq!(tier_index_for_volume(&tiers, 0), 0);
        assert_eq!(tier_index_for_volume(&tiers, 1_000_000), 0);
    }

    #[test]
    fn test_validate_accepts_sample_tiers() {
        let env = Env::default();
        validate_tiers(&sample_tiers(&env));
    }

    #[test]
    #[should_panic(expected = "first fee tier must start at zero volume")]
    fn test_validate_rejects_nonzero_first_tier() {
        let env = Env::default();
        validate_tiers(&Vec::from_array(&env, [tier(&env, 0, "bronze", 5, 300)]));
    }

    #[test]
    #[should_panic(expected = "tier min_volume must be strictly ascending")]
    fn test_validate_rejects_unsorted_volume() {
        let env = Env::default();
        validate_tiers(&Vec::from_array(
            &env,
            [tier(&env, 0, "bronze", 0, 300), tier(&env, 1, "silver", 0, 200)],
        ));
    }

    #[test]
    #[should_panic(expected = "tier fee_bps must not increase with volume")]
    fn test_validate_rejects_increasing_fee() {
        let env = Env::default();
        validate_tiers(&Vec::from_array(
            &env,
            [tier(&env, 0, "bronze", 0, 100), tier(&env, 1, "silver", 10, 200)],
        ));
    }

    #[test]
    #[should_panic(expected = "tier fee_bps exceeds maximum")]
    fn test_validate_rejects_fee_above_max() {
        let env = Env::default();
        validate_tiers(&Vec::from_array(
            &env,
            [tier(&env, 0, "bronze", 0, MAX_CREATOR_FEE_BPS + 1)],
        ));
    }

    #[test]
    #[should_panic(expected = "tier_id must match tier position")]
    fn test_validate_rejects_bad_tier_id() {
        let env = Env::default();
        validate_tiers(&Vec::from_array(&env, [tier(&env, 3, "bronze", 0, 100)]));
    }

    #[test]
    fn test_apply_fee_rounds_down() {
        assert_eq!(apply_fee(10_000, 250), 250);
        assert_eq!(apply_fee(999, 100), 9);
        assert_eq!(apply_fee(1, 1_000), 0);
        assert_eq!(apply_fee(1_000, 0), 0);
    }
}
