//! Shared fixtures for the `split` contract fuzz targets.
//!
//! Each fuzz target builds a fresh [`soroban_sdk::Env`] + registered contract
//! + funded token per input (cheap, in-memory host simulation) and drives one
//! or more contract calls through it.  All contract calls use the `try_*`
//! client variants, which return `Result` instead of panicking.  This avoids
//! any interaction with `std::panic::catch_unwind` or panic hooks, which are
//! incompatible with the soroban host's own internal panic-to-HostError
//! escalation path.

use soroban_sdk::testutils::Address as _;
use soroban_sdk::token::StellarAssetClient;
use soroban_sdk::{Address, Env, Vec};
use split::types::{InvoiceOptions, InvoiceOptions2, OverflowBehavior};
use split::{SplitContract, SplitContractClient};

/// Balance minted to every generated payer. Set to `i128::MAX` so that a
/// fuzzed payment `amount` is never rejected by the underlying token
/// contract for insufficient balance — that would just be noise unrelated
/// to the `split` contract logic under test.
pub const PAYER_BALANCE: i128 = i128::MAX;

/// Register a fresh `SplitContract` plus a funded Stellar asset token in a
/// brand-new [`Env`]. Mirrors `split::test::setup()`.
pub fn setup(env: &Env) -> (Address, Address) {
    env.mock_all_auths();
    let contract_id = env.register(SplitContract, ());
    let token_admin = Address::generate(env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    StellarAssetClient::new(env, &token_id).mint(&token_admin, &PAYER_BALANCE);
    (contract_id, token_id)
}

pub fn client<'a>(env: &'a Env, contract_id: &Address) -> SplitContractClient<'a> {
    SplitContractClient::new(env, contract_id)
}

/// Fund `who` with [`PAYER_BALANCE`] units of `token_id`.
pub fn fund(env: &Env, token_id: &Address, who: &Address) {
    StellarAssetClient::new(env, token_id).mint(who, &PAYER_BALANCE);
}

/// All-default `InvoiceOptions`. Fuzz targets clone this and override only
/// the fields they are exercising.
pub fn default_options(env: &Env) -> InvoiceOptions {
    InvoiceOptions {
        co_creators: Vec::new(env),
        allow_early_withdrawal: false,
        bonus_pool: 0,
        bonus_max_payers: 0,
        creator_cosigner: None,
        velocity_limit: 0,
        velocity_window: 0,
        prerequisite_id: None,
        tranches: Vec::new(env),
        co_signers: Vec::new(env),
        required_signatures: 0,
        penalty_bps: None,
        penalty_deadline: None,
        min_funding_bps: None,
        release_stages: Vec::new(env),
        price_oracle: None,
        swap_tokens: Vec::new(env),
        tax_bps: None,
        tax_authority: None,
        insurance_premium_bps: None,
        smart_route: None,
        notification_contract: None,
        overflow_behavior: OverflowBehavior::Reject,
        convert_to_stream: false,
        accepted_tokens: Vec::new(env),
        forward_to: None,
        forward_invoice_id: None,
        split_rules: Vec::new(env),
        auto_resolve_rules: Vec::new(env),
        oracle_address: None,
        cross_chain_ref: None,
        allowed_payers: None,
        priorities: Vec::new(env),
        refund_grace_secs: None,
        scheduled_release_at: None,
        require_kyc: false,
        ratios: Vec::new(env),
        cosigners: None,
        cosigner_threshold: None,
        ext: InvoiceOptions2 {
            target_usd_cents: None,
            payment_token: None,
            release_delay_ledgers: None,
            metadata_hash: None,
            payment_cooldown_secs: None,
            max_payments_per_window: None,
            payment_window_secs: None,
            oracle: None,
            oracle_asset_pair_base: None,
            oracle_asset_pair_quote: None,
            min_payer_rep: None,
            payment_open_at: None,
            payment_close_at: None,
        },
    }
}

/// Compute a `u64` ledger timestamp `offset_secs` away from `now`, clamped
/// to `[0, u64::MAX]` — lets fuzzed signed offsets probe both past and future
/// deadlines without ever constructing a nonsensical wrapped-around timestamp.
pub fn offset_timestamp(now: u64, offset_secs: i64) -> u64 {
    let wide = now as i128 + offset_secs as i128;
    wide.clamp(0, u64::MAX as i128) as u64
}
