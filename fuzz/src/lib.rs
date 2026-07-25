//! Shared fixtures for the `split` contract fuzz targets.
//!
//! Each fuzz target builds a fresh [`soroban_sdk::Env`] + registered contract
//! + funded token per input (cheap, in-memory host simulation) and drives one
//! or more contract calls through it. Calls are wrapped in
//! `std::panic::catch_unwind` so that documented `assert!`/`panic!`
//! validation failures (see [`EXPECTED_PANIC_SUBSTRINGS`]) are treated as a
//! normal rejection of malformed input rather than a fuzzer-found crash.
//! Anything else — an arithmetic overflow panic, an `unwrap()` on `None`, an
//! index-out-of-bounds, or any panic message we haven't explicitly
//! documented — is re-raised so libFuzzer records it as a crash.

use soroban_sdk::testutils::Address as _;
use soroban_sdk::token::StellarAssetClient;
use soroban_sdk::{Address, Env, Vec};
use split::types::{InvoiceOptions, InvoiceOptions2, OverflowBehavior};
use split::{SplitContract, SplitContractClient};

/// Balance minted to every generated payer. Set to `i128::MAX` so that a
/// fuzzed payment `amount` is never rejected by the underlying token
/// contract for insufficient balance — that would just be noise unrelated
/// to the `split` contract logic under test. Any panic that *does* still
/// occur while transferring is therefore attributable to `split` itself.
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

/// All-default `InvoiceOptions`, mirroring `split::test::default_options`.
/// Fuzz targets clone this and override only the fields they're exercising.
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
        },
    }
}

/// Compute a `u64` ledger timestamp `offset_secs` away from `now`, clamped
/// to `[0, u64::MAX]` instead of wrapping — lets fuzzed signed offsets probe
/// both past and future deadlines without ever constructing a nonsensical
/// wrapped-around timestamp.
pub fn offset_timestamp(now: u64, offset_secs: i64) -> u64 {
    let wide = now as i128 + offset_secs as i128;
    wide.clamp(0, u64::MAX as i128) as u64
}

/// Substrings of `assert!`/`panic!` messages the contract raises to reject
/// malformed input or an invoice in the wrong lifecycle state. These are
/// intentional validation failures, not bugs, and are recognized by
/// [`is_expected_panic`] so fuzz targets don't treat every rejected input as
/// a crash.
///
/// Sourced directly from the `assert!`/`panic!` call sites in
/// `create_invoice` / `_create_invoice_inner` / `pay` / `_pay` / `release` /
/// `refund` and the shared pause/dispute guards they call through.
pub const EXPECTED_PANIC_SUBSTRINGS: &[&str] = &[
    // create_invoice / _create_invoice_inner
    "recipients and amounts length mismatch",
    "must have at least one recipient",
    "deadline must be in the future",
    "bonus_pool must be non-negative",
    "penalty_bps must be",
    "min_funding_bps must be",
    "tax_bps must be",
    "insurance_premium_bps must be",
    "tax_authority must be set",
    "priorities length must match recipients",
    "oracle_asset_pair required when oracle is set",
    "cannot set both price_oracle and oracle",
    "amounts must be positive",
    "compliance check failed",
    "creator not whitelisted",
    "nft gate: not a holder",
    // pay / _pay
    "invoice is not pending",
    "invoice is disputed",
    "invoice deadline has passed",
    "payment amount must be positive",
    // release
    "invoice is frozen",
    "invoice frozen by admin",
    "invoice is under active dispute",
    "minimum funding not reached",
    "FundsLockedUntil",
    "awaiting approval",
    "prerequisite not released",
    // refund
    "deadline has not passed",
    "auction in progress",
    "auction ended; settle auction",
    "no token",
    // shared pause / circuit-breaker guards
    "contract is paused",
    "ContractPaused",
    "function paused",
    "InvoiceNotFound",
];

/// True if a caught panic payload matches one of [`EXPECTED_PANIC_SUBSTRINGS`]
/// rather than representing a genuine bug (overflow, unwrap-on-None,
/// index-out-of-bounds, or any other unrecognized panic).
pub fn is_expected_panic(payload: &(dyn std::any::Any + Send)) -> bool {
    let msg: &str = if let Some(s) = payload.downcast_ref::<&str>() {
        s
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.as_str()
    } else {
        return false;
    };
    EXPECTED_PANIC_SUBSTRINGS
        .iter()
        .any(|expected| msg.contains(expected))
}

std::thread_local! {
    static CAPTURED_PANIC_MSG: std::cell::RefCell<String> = std::cell::RefCell::new(String::new());
}

/// Run `f`, swallowing documented "expected rejection" panics and
/// re-raising anything else so libFuzzer records it as a crash.
///
/// soroban-env-host catches contract panics internally and re-panics with a
/// `HostError(WasmVm, InvalidAction)` whose Display includes the full event
/// log (which contains the original panic message). We install a temporary
/// panic hook to capture that formatted message so `is_expected_panic` can
/// match against the original contract assertion text even when the payload
/// type itself is not a plain `&str` / `String`.
pub fn catch_expected<F: FnOnce() -> R + std::panic::UnwindSafe, R>(f: F) -> Option<R> {
    CAPTURED_PANIC_MSG.with(|c| c.borrow_mut().clear());

    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| {
        CAPTURED_PANIC_MSG.with(|c| *c.borrow_mut() = format!("{info}"));
    }));

    let result = std::panic::catch_unwind(f);
    std::panic::set_hook(prev_hook);

    match result {
        Ok(v) => Some(v),
        Err(payload) => {
            let full_msg = CAPTURED_PANIC_MSG.with(|c| c.borrow().clone());
            let expected = is_expected_panic(&*payload)
                || EXPECTED_PANIC_SUBSTRINGS
                    .iter()
                    .any(|s| full_msg.contains(s));
            if expected {
                None
            } else {
                std::panic::resume_unwind(payload);
            }
        }
    }
}
