//! Issue #857: Dynamic fee adjustment based on market conditions.
//!
//! An admin configures a fee curve; an authorised reporter (or the admin)
//! periodically reports market conditions — payment volume for the period and
//! a volatility measure in bps. The contract derives a target fee:
//!
//! ```text
//! deviation_bps = (volume - target_volume) * 10_000 / target_volume   (clamped to ±100_000)
//! target        = base_bps
//!               + deviation_bps  * volume_sensitivity_bps     / 10_000
//!               + volatility_bps * volatility_sensitivity_bps / 10_000
//! target        = clamp(target, min_bps, max_bps)
//! ```
//!
//! and moves the live platform fee toward it by at most `max_step_bps` per
//! report, so fees cannot jump abruptly. The result is written to the same
//! storage slot read by `get_platform_fee_bps`, so it takes effect immediately.

use crate::error::ContractError;
use crate::types::{DynamicFeeConfig, MarketConditions};
use crate::{
    admin_key, events, platform_fee_bps_key, require_admin, require_not_paused, SplitContract,
    SplitContractArgs, SplitContractClient,
};
use soroban_sdk::{contractimpl, panic_with_error, symbol_short, Address, Env, Symbol};

/// Deviation is capped at ±10× target so extreme reports stay bounded.
const MAX_DEVIATION_BPS: i128 = 100_000;

fn config_key() -> Symbol {
    symbol_short!("dyn_fee")
}

fn conditions_key() -> Symbol {
    symbol_short!("mkt_cond")
}

fn load_config(env: &Env) -> Option<DynamicFeeConfig> {
    env.storage().instance().get(&config_key())
}

fn current_fee(env: &Env) -> u32 {
    env.storage().instance().get(&platform_fee_bps_key()).unwrap_or(0)
}

fn require_is_admin(env: &Env, admin: &Address) {
    if require_admin(env) != *admin {
        panic_with_error!(env, ContractError::NotAuthorized);
    }
}

fn validate_config(env: &Env, cfg: &DynamicFeeConfig) {
    let valid = cfg.min_bps <= cfg.base_bps
        && cfg.base_bps <= cfg.max_bps
        && cfg.max_bps <= 10_000
        && cfg.target_volume > 0;
    if !valid {
        panic_with_error!(env, ContractError::InvalidFeeConfig);
    }
}

/// Fee the curve targets for the given conditions, before step limiting.
fn target_fee(cfg: &DynamicFeeConfig, volume: i128, volatility_bps: u32) -> u32 {
    let deviation = ((volume - cfg.target_volume).saturating_mul(10_000) / cfg.target_volume)
        .clamp(-MAX_DEVIATION_BPS, MAX_DEVIATION_BPS);
    let volume_adj = deviation * cfg.volume_sensitivity_bps as i128 / 10_000;
    let volatility_adj = volatility_bps as i128 * cfg.volatility_sensitivity_bps as i128 / 10_000;
    let target = cfg.base_bps as i128 + volume_adj + volatility_adj;
    target.clamp(cfg.min_bps as i128, cfg.max_bps as i128) as u32
}

/// Move `current` toward `target` by at most `max_step` (0 = unlimited).
fn step_toward(current: u32, target: u32, max_step: u32) -> u32 {
    if max_step == 0 {
        return target;
    }
    if target > current {
        current.saturating_add(max_step).min(target)
    } else {
        current.saturating_sub(max_step).max(target)
    }
}

fn next_fee(env: &Env, cfg: &DynamicFeeConfig, volume: i128, volatility_bps: u32) -> u32 {
    let target = target_fee(cfg, volume, volatility_bps);
    // Keep the live fee inside the configured band even if it was set
    // out-of-band before dynamic fees were enabled.
    let current = current_fee(env).clamp(cfg.min_bps, cfg.max_bps);
    step_toward(current, target, cfg.max_step_bps)
}

#[contractimpl]
impl SplitContract {
    /// Admin: install or replace the dynamic fee curve. The live fee is reset
    /// to `base_bps`.
    pub fn configure_dynamic_fee(env: Env, admin: Address, config: DynamicFeeConfig) {
        require_is_admin(&env, &admin);
        validate_config(&env, &config);

        let old = current_fee(&env);
        env.storage().instance().set(&config_key(), &config);
        env.storage().instance().set(&platform_fee_bps_key(), &config.base_bps);

        events::dynamic_fee_configured(&env, config.base_bps, config.min_bps, config.max_bps);
        events::dynamic_fee_adjusted(&env, old, config.base_bps, 0, 0);
    }

    /// Admin: stop dynamic adjustment. The current fee stays in force.
    pub fn disable_dynamic_fee(env: Env, admin: Address) {
        require_is_admin(&env, &admin);
        env.storage().instance().remove(&config_key());
        env.storage().instance().remove(&conditions_key());
    }

    /// Report current market conditions and apply the resulting fee.
    /// Callable by the configured reporter or the admin. Returns the new fee.
    pub fn report_market_conditions(
        env: Env,
        reporter: Address,
        volume: i128,
        volatility_bps: u32,
    ) -> u32 {
        require_not_paused(&env);
        reporter.require_auth();
        let cfg = load_config(&env)
            .unwrap_or_else(|| panic_with_error!(&env, ContractError::DynamicFeeNotConfigured));
        let admin: Option<Address> = env.storage().instance().get(&admin_key());
        if reporter != cfg.reporter && Some(reporter.clone()) != admin {
            panic_with_error!(&env, ContractError::NotAuthorized);
        }
        if volume < 0 || volatility_bps > 10_000 {
            panic_with_error!(&env, ContractError::InvalidAmount);
        }

        let old = current_fee(&env);
        let new_fee = next_fee(&env, &cfg, volume, volatility_bps);
        env.storage().instance().set(&platform_fee_bps_key(), &new_fee);
        env.storage().instance().set(
            &conditions_key(),
            &MarketConditions {
                volume,
                volatility_bps,
                reported_at: env.ledger().timestamp(),
                applied_fee_bps: new_fee,
            },
        );

        events::dynamic_fee_adjusted(&env, old, new_fee, volume, volatility_bps);
        new_fee
    }

    /// Fee that `report_market_conditions` would apply for these inputs,
    /// without changing state.
    pub fn preview_dynamic_fee(env: Env, volume: i128, volatility_bps: u32) -> u32 {
        let cfg = load_config(&env)
            .unwrap_or_else(|| panic_with_error!(&env, ContractError::DynamicFeeNotConfigured));
        next_fee(&env, &cfg, volume.max(0), volatility_bps.min(10_000))
    }

    pub fn get_dynamic_fee_config(env: Env) -> Option<DynamicFeeConfig> {
        load_config(&env)
    }

    pub fn get_market_conditions(env: Env) -> Option<MarketConditions> {
        env.storage().instance().get(&conditions_key())
    }
}
