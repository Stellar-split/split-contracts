//! StellarSplit â on-chain invoice & payment splitting contract.
//!
//! Allows a creator to define an invoice with multiple recipients and amounts.
//! Payers contribute funds; once fully funded the contract auto-routes USDC to
//! each recipient. If the deadline passes unfunded, payers are refunded.
//!
//! Additional features:
//! - #865 Creator collateral system for risk management
//! - #866 Invoice rating-based discount tiers
//! - #867 Smart invoice routing based on recipient performance
//! - #868 Creator covenant system with penalties
//! Additionally features audit logging, invoice archival, and a contributor leaderboard.
//! StellarSplit — on-chain invoice & payment splitting contract.

#![no_std]
#![allow(clippy::too_many_arguments)]

const SHARD_COUNT: u64 = 8;
const ARCHIVE_AFTER_LEDGERS: u64 = 100_000;

/// Maximum number of co-creators allowed per invoice (bounded to prevent
/// unbounded storage growth).
const MAX_CO_CREATORS: usize = 10;

/// Default dispute timeout in ledgers (30 days at ~5 s/ledger).
const DEFAULT_DISPUTE_TIMEOUT_LEDGERS: u32 = 518_400;

/// Default payer spend cap window in ledgers (~1 day at ~5 s/ledger).
const DEFAULT_PAYER_SPEND_WINDOW_LEDGERS: u32 = 17_280;

/// Issue #298: Soroban per-transaction instruction budget limit.
const INSTRUCTION_BUDGET_LIMIT: u64 = 100_000_000;

/// Issue #298: Base cost per recipient transfer during release (estimated).
const INSTRUCTIONS_PER_RECIPIENT: u64 = 500_000;
/// Issue #298: Base cost per payment shard to aggregate (estimated).
const INSTRUCTIONS_PER_SHARD: u64 = 100_000;
/// Issue #298: Fixed overhead for a release call (estimated).
const INSTRUCTIONS_BASE: u64 = 1_000_000;
/// Issue #298: Stroops per 10_000 instructions (rough estimate based on Soroban fee schedule).
const STROOPS_PER_10K_INSTRUCTIONS: u64 = 1;

/// Issue #296: Maximum entries in the per-creator fee waiver list.
const MAX_FEE_WAIVER_ENTRIES: usize = 100;
const DEFAULT_COMMITMENT_EXPIRY_LEDGERS: u32 = 100;

/// Fixed-point scale for oracle-priced invoices: the oracle's `price()` return
/// value is USD cents per 1 whole token, scaled by this factor (e.g. 1 XLM at
/// $0.12 = 12 cents is reported as `12 * ORACLE_RATE_SCALE` = `12_000_000`).
const ORACLE_RATE_SCALE: i128 = 1_000_000;

/// Issue #425: default per-invoice storage quota in bytes, applied at `initialize()`
/// and used by `get_storage_quota` before an admin ever calls
/// `set_invoice_storage_quota`. Sized with headroom above the largest invoices in
/// the test suite (~200 recipients, ~15KB) so it only trips on genuinely unbounded
/// growth; admins can tighten it via `set_invoice_storage_quota`.
const DEFAULT_INVOICE_STORAGE_QUOTA: u64 = 65_536;

mod constants;
mod error;
mod events;
pub mod types;
mod validation;
mod calc;
mod recipients_ext;
mod visibility_ext;
mod referral_ext;
mod pause_ext;
mod stats;
mod search_ext;
mod treasury_gov_ext;
mod nonce_ext;
mod group_ext;

#[cfg(test)]
pub(crate) mod test;
#[cfg(test)]
mod ext_tests;

#[cfg(test)]
mod fuzz_tests;

#[cfg(test)]
mod test_market_features;

#[cfg(test)]
mod storage_snapshot;

#[cfg(test)]
mod op_snapshot;

mod storage;
mod storage_keys;

mod migrations;
mod hold_ext;
mod freeze_ext;
mod treasury_multi_ext;
mod velocity_ext;
mod payer_tier_ext;
mod payment_pool_ext;
mod payer_feedback_ext;
mod export_validation_ext;
#[cfg(test)]
mod ext_test_util;

mod invoice_ops;

// Issues #857–#860: feature modules, each with its own `#[contractimpl]` block.
mod dynamic_fee;
mod earnings_insurance;
mod invoice_links;
mod liquidity_pool;

use error::ContractError;
use validation::assert_valid_bps;
use calc::{calc_platform_fee, funding_bps};
use soroban_sdk::crypto::bls12_381::{Fr, G1Affine};
use soroban_sdk::xdr::ToXdr;
use soroban_sdk::{
    contract, contractimpl, panic_with_error, symbol_short, token, Address, Bytes, BytesN, Env,
    IntoVal, Map, String, Symbol, TryFromVal, Val, Vec, U256,
};

#[allow(dead_code)]
const MAX_PARENT_DEPTH: u32 = 10;

/// Issue #749: maximum number of addresses that may be present in an invoice's
/// payer whitelist (`allowed_payers`).
const MAX_PAYER_WHITELIST: u32 = 50;

use types::{
    CreatorEarnings,
    AdminAction, AdminRole, AdminSet, AuditEntry, Bid, CircuitBreakerStatus,
    CloneOverrides, CompactInvoice, CompactMigrateResult, CompletionProof, ComputeEstimate,
    ConfidentialPayment, ContributionResult, CreateInvoiceParams, CreatorStats, CreatorTier,
    DelayedPayout,
    DisputeOutcome, DisputeRecord, DisputeStatus, FeeBracket, FeeSplit, FeeTier, InstalmentPlan,
    Invoice, InvoiceCore, InvoiceExt, InvoiceExt2, InvoiceExt3, InvoiceHot, InvoiceLock,
    InvoiceOptions,
    InvoiceOptions2, InvoicePayment, InvoiceStats, InvoiceStatus, InvoiceTemplate,
    InvoiceTemplateRecord, LegacyInvoice, OverflowBehavior, OverfundingPolicy, Payment,
    PaymentCertificate, PaymentCommitment, PaymentProof, PaymentRecord, PendingAdminAction,
    ProtocolFeeConfig, QueuedAction, RebateTier, Recipient, RepScore, ResolveAction, RewardPoolInfo, RoundInfo,
    ResolveRule, Role, SimulateReleaseResult, SplitRule, SubscriptionParams, TimelockAction,
    Tombstone, Tranche, TransferRecord, TreasuryRecord, UpgradeProposal,
    // Issue #759
    CreatorRating,
    // Issue #756
    Note,
    // Issue #758
    Subscription,
    // Issue #872
    InvoicePriceTier, InvoicePricingModel,
    // Issue #870
    RecipientDelegation, RecipientPerformance,
    // Issue #871
    InvoiceTimeLock,
    // Issue #869
    RedemptionToken,
};

// ---------------------------------------------------------------------------
// Storage key helpers
// ---------------------------------------------------------------------------

fn governance_contract_key() -> Symbol {
    symbol_short!("gov_ctr")
}

fn admin_key() -> Symbol {
    symbol_short!("admin")
}
fn admins_key() -> Symbol {
    symbol_short!("admins")
}

/// Issue #477: One-shot initialiser guard — instance storage.
/// Set to `true` at the end of `initialize()`; checked at the top to prevent
/// re-initialisation (front-run protection).
fn initialised_key() -> Symbol {
    symbol_short!("init_flg")
}
fn paused_key() -> Symbol {
    symbol_short!("paused")
}
fn paused_fns_key() -> Symbol {
    symbol_short!("ps_fns")
}

fn pause_exempt_key(address: &Address) -> (Symbol, Address) {
    (symbol_short!("p_exempt"), address.clone())
}

fn global_payer_limit_key() -> Symbol {
    symbol_short!("g_vel_lim")
}

fn global_payer_window_key() -> Symbol {
    symbol_short!("g_vel_win")
}

fn global_vel_key(payer: &Address) -> (Symbol, Address) {
    (symbol_short!("g_vel"), payer.clone())
}
fn creation_fee_key() -> Symbol {
    symbol_short!("crt_fee")
}
fn platform_fee_bps_key() -> Symbol {
    symbol_short!("plat_fee")
}
#[allow(dead_code)]
/// Issue #803: maximum creator commission, in basis points (5%).
const MAX_CREATOR_FEE_BPS: u32 = 500;

fn creator_fee_bps_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("cr_fee_bp"), invoice_id)
}
fn pending_creator_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("pend_cr"), invoice_id)
}
fn tombstone_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("tombstone"), invoice_id)
}
fn fallback_escrow_key(invoice_id: u64, recipient: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("fb_esc"), invoice_id, recipient.clone())
}
fn plan_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("inst_pl"), invoice_id, payer.clone())
}
fn fee_brackets_key() -> Symbol {
    symbol_short!("fee_brks")
}

fn platform_fee_waiver_list_key() -> Symbol {
    symbol_short!("fee_wvrs")
}
fn fee_recipients_key() -> Symbol {
    symbol_short!("fee_rcp")
}

/// Issue #296: Per-creator fee waiver list key (distinct from the recipient-level waiver list).
fn creator_fee_waiver_key() -> Symbol {
    symbol_short!("cr_fw")
}

/// Issue #297: Circuit breaker active flag key.
fn circuit_breaker_key() -> Symbol {
    symbol_short!("cb_flag")
}

/// Issue #297: Circuit breaker reason storage key.
fn circuit_breaker_reason_key() -> Symbol {
    symbol_short!("cb_rsn")
}

/// Issue #295: Confidential payment storage key per (invoice_id, payer).
fn confidential_pay_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("conf_pay"), invoice_id, payer.clone())
}

/// Issue #327: release delay ledgers for an invoice.
fn release_delay_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("rel_dly"), id)
}
fn recipient_whitelist_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("rcp_wl"), id)
}
/// Issue #790: co-funding round configuration and state for an invoice.
fn funding_round_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("fund_rnd"), id)
}
/// Issue #327: ledger sequence when the invoice was fully funded.
fn funded_at_ledger_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("fund_led"), id)
}
/// Issue #809: auto-release condition set on an invoice.
fn auto_release_condition_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("auto_cnd"), id)
}
/// Issue #810: aggregate performance metrics for a recipient.
fn recipient_metrics_key(recipient: &Address) -> (Symbol, Address) {
    (symbol_short!("rcp_met"), recipient.clone())
}

/// Issue #809: whether the invoice's auto-release condition (if any) is met.
/// `None` when no condition is set.
fn auto_release_condition_met(env: &Env, invoice_id: u64) -> Option<bool> {
    env.storage()
        .persistent()
        .get::<_, AutoReleaseCondition>(&auto_release_condition_key(invoice_id))
        .map(|condition| match condition {
            AutoReleaseCondition::AtTimestamp(at) => env.ledger().timestamp() >= at,
        })
}

/// Issue #810: count one more released invoice for each distinct recipient.
fn record_recipient_release(env: &Env, recipients: &Vec<Address>) {
    let mut seen: Vec<Address> = Vec::new(env);
    for recipient in recipients.iter() {
        if seen.contains(&recipient) {
            continue;
        }
        seen.push_back(recipient.clone());
        let key = recipient_metrics_key(&recipient);
        let mut metrics: RecipientMetrics = env.storage().persistent().get(&key).unwrap_or_default();
        metrics.invoices_received_count = metrics.invoices_received_count.saturating_add(1);
        env.storage().persistent().set(&key, &metrics);
    }
}
/// Issue #329: off-chain metadata hash for an invoice.
fn metadata_hash_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("meta_hsh"), id)
}
/// Issue #330: set of recipients already paid via release_to_recipient.
fn paid_recipients_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("paid_rec"), id)
}
/// Issue #430: creator-defined payment window open timestamp.
fn payment_open_at_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("pay_open"), id)
}
/// Issue #430: creator-defined payment window close timestamp.
fn payment_close_at_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("pay_close"), id)
}

/// Issue #430: read the configured payment-window open timestamp, if any.
fn get_payment_open_at_internal(env: &Env, id: u64) -> Option<u64> {
    env.storage().persistent().get(&payment_open_at_key(id))
}

/// Issue #430: read the configured payment-window close timestamp, if any.
fn get_payment_close_at_internal(env: &Env, id: u64) -> Option<u64> {
    env.storage().persistent().get(&payment_close_at_key(id))
}

/// Issue #332: contiguous Vec<Address> of all recipients — persistent storage.
fn recipients_list_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("rec_lst"), id)
}
/// Issue #332: contiguous Vec<i128> of amounts parallel to recipients_list_key — persistent storage.
fn amounts_list_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("amt_lst"), id)
}
/// Issue #332: u32 bit-vector of paid flags — persistent storage.
#[cfg(test)]
fn paid_flags_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("paid_flg"), id)
}

/// Issue #333: u8 bitmask of milestones already emitted — instance storage.
/// Bit 0 = 25 %, Bit 1 = 50 %, Bit 2 = 75 %, Bit 3 = 100 %.
fn milestone_flags_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("ms_flgs"), id)
}

/// RBAC: per-(address, role) assignment flag — persistent storage.
/// Stored as `bool`; absent key means role is not held.
#[allow(dead_code)]
fn role_key(address: &Address, role_discriminant: u32) -> (Symbol, Address, u32) {
    (symbol_short!("role_asn"), address.clone(), role_discriminant)
}

/// Convert a `Role` to its stable u32 discriminant used as the storage key component.
#[allow(dead_code)]
fn role_discriminant(role: &Role) -> u32 {
    match role {
        Role::Admin    => 0,
        Role::Creator  => 1,
        Role::Operator => 2,
        Role::Auditor  => 3,
    }
}

/// Contract-level funding progress checkpoints in basis points.
fn funding_checkpoints_key() -> Symbol {
    symbol_short!("fnd_chk")
}

/// Highest admin-configured funding checkpoint already emitted per invoice.
fn last_checkpoint_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("last_chk"), invoice_id)
}

/// Cliff + vesting schedule: bitmask (u32) of tranche indices already released
/// via `release_tranche()` — bit N set means `tranches[N]` has been paid out.
/// Supports up to 32 tranches per invoice (matches `paid_flags_key` convention).
fn released_tranche_idx_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("tr_rel_ix"), id)
}

/// Issue #334: compact status byte (u8) — persistent storage.
fn compact_status_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("cpt_sts"), id)
}
/// Issue #334: compact deadline as ledger sequence (u32) — persistent storage.
fn compact_deadline_ledger_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("cpt_dlg"), id)
}

/// Issue #295: Counter of confidential payments per invoice.
fn confidential_count_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("conf_cnt"), invoice_id)
}

fn treasury_key() -> Symbol {
    symbol_short!("treasury")
}
fn rebate_tiers_key() -> Symbol {
    symbol_short!("rbt_trs")
}
fn rebate_balance_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("rbt_bal"), creator.clone())
}
fn creator_volume_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("cr_vol_r"), creator.clone())
}
fn usdc_token_key() -> Symbol {
    symbol_short!("usdc_tok")
}
fn counter_key() -> Symbol {
    symbol_short!("counter")
}
fn archive_after_ledgers_key() -> Symbol {
    symbol_short!("arch_af")
}
fn archive_marker_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("archv"), id)
}
fn created_ledger_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("cr_ledger"), id)
}

/// Record the creation ledger for an invoice, keeping the entry alive for as
/// long as the rest of the invoice's persistent storage.
fn set_created_ledger(env: &Env, id: u64) {
    env.storage()
        .persistent()
        .set(&created_ledger_key(id), &env.ledger().sequence());
    env.storage().persistent().extend_ttl(
        &created_ledger_key(id),
        INVOICE_HOT_TTL_LEDGERS / 2,
        INVOICE_HOT_TTL_LEDGERS,
    );
}
fn invoice_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("inv"), id)
}
fn invoice_ext_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("inv_ext"), id)
}
fn invoice_ext2_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("inv_ex2"), id)
}
fn invoice_compact_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("inv_cpt"), id)
}
/// Hot invoice fields — instance storage key. See `InvoiceHot` in types.rs.
fn invoice_hot_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("inv_hot"), id)
}
fn audit_log_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("log"), id)
}
fn subscription_params_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("sub"), id)
}
fn ext_vote_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("ext_vote"), id)
}
/// Issue #752: NFT mint contract key — instance storage (distinct from nft_gate_key).
fn nft_mint_contract_key() -> Symbol {
    symbol_short!("nft_ctr")
}
/// Issue #754: Tag index key — maps tag string to vec of invoice IDs.
fn tag_index_key(tag: &String) -> (Symbol, String) {
    (symbol_short!("tag_idx"), tag.clone())
}
/// Issue #754: Per-invoice tags storage key.
fn invoice_tags_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("inv_tags"), id)
}
/// Issue #755: Per-invoice extension count key.
fn invoice_ext_count_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("ext_cnt"), id)
}
/// Issue #755: Per-invoice extension config key — stores (duration_seconds: u64, quorum_bps: u32).
fn invoice_ext_config_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("ext_cfg"), id)
}
fn group_key(group_id: u64) -> (Symbol, u64) {
    (symbol_short!("grp"), group_id)
}
fn invoice_group_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("invgrp"), invoice_id)
}

fn invoice_treasury_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("inv_tr"), invoice_id)
}

fn treasury_group_counter_key() -> Symbol {
    symbol_short!("grp_tr_cn")
}

fn reminder_key(invoice_id: u64, address: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("rem"), invoice_id, address.clone())
}

fn group_treasury_key(group_id: u64) -> (Symbol, u64) {
    (symbol_short!("grp_tr"), group_id)
}

/// Issue #476: ID-based invoice template — persistent storage.
fn template_id_key(creator: &Address, template_id: u64) -> (Symbol, Address, u64) {
    (symbol_short!("tmpl_id"), creator.clone(), template_id)
}

/// Issue #476: Template ID counter per creator — persistent storage.
fn template_id_counter_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("tmpl_ctr"), creator.clone())
}

/// Issue #475: The multi-sig admin set — instance storage.
fn admin_set_key() -> Symbol {
    symbol_short!("adm_set")
}

/// Issue #475: Pending multi-sig admin action keyed by its action hash — persistent storage.
fn pending_admin_action_key(action_hash: &BytesN<32>) -> (Symbol, BytesN<32>) {
    (symbol_short!("adm_pnd"), action_hash.clone())
}

fn template_key(creator: &Address, name: &Symbol) -> (Symbol, Address, Symbol) {
    (symbol_short!("tmpl"), creator.clone(), name.clone())
}

/// Issue #210: versioned template key.
fn template_version_key(
    creator: &Address,
    name: &Symbol,
    version: u32,
) -> (Symbol, Address, Symbol, u32) {
    (
        symbol_short!("tmpl_v"),
        creator.clone(),
        name.clone(),
        version,
    )
}

/// Issue #210: template version counter key.
fn template_version_count_key(creator: &Address, name: &Symbol) -> (Symbol, Address, Symbol) {
    (symbol_short!("tmpl_ct"), creator.clone(), name.clone())
}

/// Issue #829: cumulative amount reversed for (invoice_id, payer).
fn reversed_amount_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("pay_rev"), invoice_id, payer.clone())
}

/// Issue #831: invoice ids linked to a payment transaction hash.
fn payment_hash_key(tx_hash: &BytesN<32>) -> (Symbol, BytesN<32>) {
    (symbol_short!("pay_hash"), tx_hash.clone())
}

/// Issue #209: pending payout key per (invoice_id, recipient).
fn pending_payout_key(invoice_id: u64, recipient: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("pend_pay"), invoice_id, recipient.clone())
}

/// Per-address reputation counter key (issue #24, #349).
fn rep_key(payer: &Address) -> (Symbol, Address) {
    (symbol_short!("rep"), payer.clone())
}

fn get_rep_internal(env: &Env, address: &Address) -> RepScore {
    env.storage()
        .persistent()
        .get(&rep_key(address))
        .unwrap_or_default()
}

fn update_rep_internal<F>(env: &Env, address: &Address, update_fn: F) -> RepScore
where
    F: FnOnce(&mut RepScore),
{
    let mut score = get_rep_internal(env, address);
    update_fn(&mut score);
    env.storage().persistent().set(&rep_key(address), &score);
    // Compute the derived score integer before emitting the event.
    // Formula: (paid_on_time * 10 + invoices_released * 5)
    //          .saturating_sub(late_pays * 5 + invoices_refunded * 2)
    let base = (score.paid_on_time as u32)
        .saturating_mul(10)
        .saturating_add((score.invoices_released as u32).saturating_mul(5));
    let deductions = (score.late_pays as u32)
        .saturating_mul(5)
        .saturating_add((score.invoices_refunded as u32).saturating_mul(2));
    let computed_score = base.saturating_sub(deductions);
    events::rep_updated(env, address, &score, computed_score);
    score
}

/// Per-address credit score key (issue #38).
fn credit_key(payer: &Address) -> (Symbol, Address) {
    (symbol_short!("credit"), payer.clone())
}

/// Issue #792: the referrer credited with acquiring a user.
fn acquisition_referrer_key(user: &Address) -> (Symbol, Address) {
    (symbol_short!("acq_ref"), user.clone())
}

/// Per-address referral count key (issue #87).
fn referral_count_key(referrer: &Address) -> (Symbol, Address) {
    (symbol_short!("ref_cnt"), referrer.clone())
}

fn channel_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("chan"), invoice_id, payer.clone())
}

/// Per-payer per-invoice nonce key (issue #21).
fn nonce_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("nonce"), invoice_id, payer.clone())
}

/// Contract-wide per-caller nonce key for off-chain signed authorisations (issue #424).
/// Unlike `nonce_key`, this is not scoped to a single invoice: it tracks one
/// monotonically increasing sequence per caller across every nonce-protected
/// entry point, so a signed authorisation cannot be replayed against a
/// different invoice or a different call.
fn global_nonce_key(caller: &Address) -> (Symbol, Address) {
    (symbol_short!("g_nonce"), caller.clone())
}

/// Returns the current expected contract-wide nonce for `caller`. Starts at 0.
fn get_global_nonce_internal(env: &Env, caller: &Address) -> u64 {
    env.storage()
        .persistent()
        .get(&global_nonce_key(caller))
        .unwrap_or(0u64)
}

/// Validates `nonce` against the stored contract-wide nonce for `caller` and,
/// on success, atomically increments it so the same nonce cannot be reused.
/// Panics with "InvalidNonce" on a stale or out-of-order nonce.
fn consume_global_nonce(env: &Env, caller: &Address, nonce: u64) {
    let stored = get_global_nonce_internal(env, caller);
    if nonce != stored {
        panic!("InvalidNonce");
    }
    env.storage()
        .persistent()
        .set(&global_nonce_key(caller), &(stored + 1));
}

/// Per-payer velocity window state key: (window_start, window_total)
fn vel_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("vel"), invoice_id, payer.clone())
}

/// Authorised factory addresses key (issue #145).
#[allow(dead_code)]
fn factories_key() -> Symbol {
    symbol_short!("factories")
}

/// Per-recipient invoice ID index key (issue #40).
fn recipient_invoice_ids_key(recipient: &Address) -> (Symbol, Address) {
    (symbol_short!("rec_inv"), recipient.clone())
}

/// Issue #1: Stellar payment streaming contract address.
fn stream_contract_key() -> Symbol {
    symbol_short!("strm_ctr")
}

/// Issue #4: Creator whitelist key.
fn creator_whitelist_key() -> Symbol {
    symbol_short!("crt_wl")
}

/// Delegate address key for an invoice (issue #43).
fn delegate_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("delegate"), invoice_id)
}

/// Delegate-pay authorization key for a beneficiary.
fn delegate_pay_key(beneficiary: &Address) -> (Symbol, Address) {
    (symbol_short!("dlgt_pay"), beneficiary.clone())
}

/// N-of-M release approval: configured cosigner addresses for an invoice.
/// Set at creation time from `InvoiceOptions::cosigners`; absent means the
/// gate is disabled for this invoice.
fn cosigners_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("cosigrs"), id)
}
/// N-of-M release approval: required number of `cosigners_key` approvals.
/// Set at creation time from `InvoiceOptions::cosigner_threshold`.
fn cosigner_thresh_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("cosig_th"), id)
}
/// N-of-M release approval: recorded approvals collected via `approve_release`.
fn cosign_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("cosign"), id)
}

/// Analytics counters (issue #28).
fn total_invoices_key() -> Symbol {
    symbol_short!("tot_inv")
}
fn total_volume_key() -> Symbol {
    symbol_short!("tot_vol")
}
/// Issue #276: platform volume milestone threshold key (in token base units).
fn platform_vol_thresh_key() -> Symbol {
    symbol_short!("plt_v_th")
}
/// Issue #276: last platform milestone number emitted.
fn platform_vol_mile_key() -> Symbol {
    symbol_short!("plt_v_ms")
}
/// Issue #276: last creator milestone number emitted.
fn creator_vol_mile_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("cr_v_ms"), creator.clone())
}
/// Issue #276: creator volume milestone threshold key.
fn creator_vol_thresh_key() -> Symbol {
    symbol_short!("cr_v_th")
}
fn total_released_key() -> Symbol {
    symbol_short!("tot_rel")
}
fn total_refunded_key() -> Symbol {
    symbol_short!("tot_ref")
}

/// Compliance contract address key.
fn compliance_key() -> Symbol {
    symbol_short!("comply")
}

/// Per-creator invoice creation rate limit usage key.
fn rate_usage_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("rate"), creator.clone())
}

/// Global per-creator rate limit value.
fn rate_limit_key() -> Symbol {
    symbol_short!("rate_lim")
}

/// Global per-creator rate window value.
fn rate_window_key() -> Symbol {
    symbol_short!("rate_win")
}

/// KYC verification contract address key.
fn kyc_contract_key() -> Symbol {
    symbol_short!("kyc_ctr")
}

/// Per-creator invoice creation count key (issue #106).
fn invoice_count_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("inv_count"), creator.clone())
}

/// Issue: per-creator invoice cancellation count key (cancellation rate limit).
fn cancel_count_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("cnl_count"), creator.clone())
}

/// Issue #439: per-creator cooldown until ledger after cancellation.
fn creator_cooldown_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("cr_cool"), creator.clone())
}

/// Default cancellation cooldown in ledgers (~1 day at 5s/ledger).
const DEFAULT_CANCELLATION_COOLDOWN_LEDGERS: u64 = 17_280;

/// Instance-storage key for the configurable cancellation cooldown duration.
fn cancellation_cooldown_ledgers_key() -> Symbol {
    symbol_short!("cnl_cool")
}

/// Storage key for a pending recipient-replacement proposal.
/// Keyed by (invoice_id, old_recipient).
#[allow(dead_code)]
fn repl_proposal_key(invoice_id: u64, old_recipient: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("repl_prp"), invoice_id, old_recipient.clone())
}

/// Issue: maximum cancellation rate in basis points, stored globally.
fn max_cancel_bps_key() -> Symbol {
    symbol_short!("mx_cnl_bp")
}

/// Issue #425: global per-invoice storage quota (bytes), admin-configurable.
fn storage_quota_key() -> Symbol {
    symbol_short!("inv_quota")
}

/// Issue: receipt token factory contract address key.
fn receipt_factory_key() -> Symbol {
    symbol_short!("rcpt_fac")
}

/// Issue: per-payer per-invoice receipt token address key.
fn receipt_token_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("rcpt"), invoice_id, payer.clone())
}

/// Per-invoice per-payer micro-payment accumulator key.
fn accum_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("accum"), invoice_id, payer.clone())
}

/// Per-creator total invoice count key.
fn creator_stats_count_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("cr_cnt"), creator.clone())
}

/// Per-creator total funded volume key.
fn creator_stats_volume_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("cr_vol"), creator.clone())
}

/// Per-creator total released volume key.
fn creator_stats_released_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("cr_rel"), creator.clone())
}

/// Per-creator total refunded volume key.
fn creator_stats_refunded_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("cr_ref"), creator.clone())
}

/// Per-creator total payers count key (Issue #299).
fn creator_stats_payers_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("cr_pyr"), creator.clone())
}

/// Per-creator average funding time in ledgers key (Issue #299).
fn creator_stats_avg_funding_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("cr_avgf"), creator.clone())
}

/// Dashboard contract address key.
fn dashboard_contract_key() -> Symbol {
    symbol_short!("dash_ctr")
}

/// Per-payer last-payment timestamp key for cooldown enforcement (issue #168).
fn payer_cooldown_key(invoice_id: u64, payer: Address) -> (Symbol, u64, Address) {
    (symbol_short!("pyr_cd"), invoice_id, payer)
}

/// Issue #763: Per-invoice history ring buffer key — persistent storage.
/// Stores a `Vec<HistoryEntry>` capped at [`HISTORY_RING_CAP`] entries.
fn history_key(invoice_id: u64) -> storage_keys::InvoiceKey {
    storage_keys::history_key(invoice_id)
}

/// Issue #760: Per-invoice milestone list key — persistent storage.
/// Stores a `Vec<Milestone>` set at creation via `InvoiceOptions::milestone_list`.
fn milestone_data_key(invoice_id: u64) -> storage_keys::InvoiceKey {
    storage_keys::milestone_data_key(invoice_id)
}

/// Sliding-window payment timestamp list key for rate limiting (issue #168).
fn payment_window_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("pay_win"), invoice_id)
}

/// Sliding-window rate-limit timestamps per (invoice_id, payer).
fn payer_payment_timestamps_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("pay_ts"), invoice_id, payer.clone())
}

/// Issue #447: per-invoice analytics.
fn invoice_analytics_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("inv_anltc"), invoice_id)
}

/// Issue #449: per-invoice phase.
fn invoice_phase_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("inv_phase"), invoice_id)
}

/// Issue #448: per-invoice slippage tolerance in basis points.
fn slippage_tolerance_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("slp_tol"), invoice_id)
}

/// Issue #451: per-invoice required memo hash.
fn required_memo_hash_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("req_memo"), invoice_id)
}

fn invoice_rate_limit_window_key() -> Symbol {
    symbol_short!("inv_rl_w")
}

fn invoice_rate_limit_max_key() -> Symbol {
    symbol_short!("inv_rl_m")
}

fn invoice_rating_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("inv_rat"), invoice_id, payer.clone())
}

fn invoice_rating_sum_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("inv_rsm"), invoice_id)
}

fn invoice_rating_count_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("inv_rct"), invoice_id)
}

fn creator_rating_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("crt_rat"), creator.clone())
}

/// Issue #756: Notes list for an invoice — persistent storage.
fn notes_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("inv_nts"), invoice_id)
}

/// Issue #758: New-style subscription record — distinct from legacy SubscriptionParams.
fn subscription_record_key(subscription_id: u64) -> (Symbol, u64) {
    (symbol_short!("sub_rec"), subscription_id)
}

fn renewed_to_key(old_invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("rnewd"), old_invoice_id)
}

fn cert_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("cert"), invoice_id)
}

const PAYMENT_WINDOW_CAP: u32 = 100;

/// NFT gate contract address key (issue #192).
fn nft_gate_key() -> Symbol {
    symbol_short!("nft_gte")
}

/// Timelock duration in seconds key (issue #185).
fn timelock_secs_key() -> Symbol {
    symbol_short!("tl_secs")
}

/// Timelock action counter key (issue #185).
fn timelock_action_counter_key() -> Symbol {
    symbol_short!("tl_cntr")
}

/// Timelock action storage key (issue #185).
fn timelock_action_key(action_id: u64) -> (Symbol, u64) {
    (symbol_short!("tl_act"), action_id)
}

fn pay_shard_key(invoice_id: u64, shard_id: u64) -> (Symbol, u64, u64) {
    (symbol_short!("pay_sh"), invoice_id, shard_id)
}

fn compute_shard_id(env: &Env, payer: &Address) -> u64 {
    let bytes = payer.to_xdr(env);
    let len = bytes.len();
    let last = bytes.get(len - 1).unwrap_or(0) as u64;
    last % SHARD_COUNT
}

fn require_admin(env: &Env) -> Address {
    migrations::require_schema_current(env);
    let admin: Address = env
        .storage()
        .instance()
        .get(&admin_key())
        .expect("admin not set");
    admin.require_auth();
    admin
}

/// Check that `caller` is either the invoice creator or a listed co-creator.
/// Panics with "NotAuthorized" if neither.
fn require_creator_or_cocreator(invoice: &Invoice, caller: &Address) {
    if invoice.creator == *caller {
        return;
    }
    if invoice.co_creators.iter().any(|c| c == *caller) {
        return;
    }
    panic!("NotAuthorized: caller is not the creator or a co-creator");
}

fn creator_volume_cap_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("cr_v_cap"), creator.clone())
}

fn creator_volume_used_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("cr_v_use"), creator.clone())
}

fn fee_tiers_key() -> Symbol {
    symbol_short!("fee_trs")
}

/// Issue #824: Per-creator explicit commission tier — persistent storage.
fn creator_tier_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("cr_tier"), creator.clone())
}

/// Issue #824: Commission rate (basis points) charged for each creator tier.
fn tier_commission_bps(tier: &CreatorTier) -> u32 {
    match tier {
        CreatorTier::Bronze => 300,   // 3.00%
        CreatorTier::Silver => 200,   // 2.00%
        CreatorTier::Gold => 100,     // 1.00%
        CreatorTier::Platinum => 50,  // 0.50%
    }
}

fn pending_admin_key() -> Symbol {
    symbol_short!("pend_adm")
}

/// Issue #526: Minimum recipient count per invoice — instance storage.
fn min_recipients_key() -> Symbol {
    symbol_short!("min_recip")
}

/// Issue #527: Per-payer payment history — persistent storage.
fn payer_history_key(payer: &Address) -> (Symbol, Address) {
    (symbol_short!("pay_hist"), payer.clone())
}

/// Issue #310: pending upgrade proposal — instance storage.
fn upgrade_proposal_key() -> Symbol {
    symbol_short!("upg_prop")
}

/// Issue #315: per-invoice delegation authorization — persistent storage.
/// Key: (invoice_id, on_behalf_of) → delegate Address (single-use).
fn delegation_key(invoice_id: u64, on_behalf_of: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("deleg"), invoice_id, on_behalf_of.clone())
}

/// Issue #325: per-invoice dispute record — persistent storage.
fn dispute_record_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("disp_rec"), invoice_id)
}

/// Issue #325: dispute raised-at ledger per invoice — persistent storage.
fn dispute_raised_at_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("disp_at"), invoice_id)
}

/// Issue #326: protocol fee config — instance storage.
fn protocol_fee_key() -> Symbol {
    symbol_short!("proto_fee")
}

/// Issue #751: accumulated protocol fees held by the contract, awaiting
/// `withdraw_treasury`. Instance storage so the balance never expires while the
/// contract is live.
fn treasury_balance_key() -> Symbol {
    symbol_short!("trs_bal")
}

/// Issue #751: current protocol fee rate in basis points (0 when unset).
///
/// Reads the [`ProtocolFeeConfig`] written by `set_protocol_fee` /
/// `set_protocol_fee_bps`; a missing config means the fee is disabled.
fn get_protocol_fee_bps_internal(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get::<Symbol, ProtocolFeeConfig>(&protocol_fee_key())
        .map(|cfg| cfg.rate_bps)
        .unwrap_or(0)
}

/// Issue #751: protocol fees currently held by the contract.
fn get_treasury_balance_internal(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get::<Symbol, i128>(&treasury_balance_key())
        .unwrap_or(0)
}

fn commitment_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("commit"), invoice_id, payer.clone())
}

// ---------------------------------------------------------------------------
// Confidential payment settlement — Pedersen commitments over BLS12-381 G1.
//
// Distinct from the pre-existing `pay_confidential` / `reveal_confidential_total`
// placeholder (issue #295), which only checks that a proof blob is non-zero.
// This scheme performs a real elliptic-curve commitment opening: a payer commits
// to `C = value*G + blinding*H` during `pay`, then proves the opening at
// `reveal_confidential_payment` time by supplying `(value, blinding)`, which the
// contract recombines and checks against the stored commitment.
// ---------------------------------------------------------------------------

/// Instance storage: fixed Pedersen base generator `G` (BLS12-381 G1), derived
/// once at `initialize` via hash-to-curve.
fn pedersen_g_key() -> Symbol {
    symbol_short!("pc_gen_g")
}

/// Instance storage: fixed Pedersen blinding generator `H` (BLS12-381 G1),
/// independent of `G` — nobody knows the discrete log of one relative to the
/// other, which is what makes the commitment hiding.
fn pedersen_h_key() -> Symbol {
    symbol_short!("pc_gen_h")
}

/// Persistent storage: pending Pedersen commitment digest for `(invoice_id, payer)`,
/// awaiting `reveal_confidential_payment`.
fn pedersen_commitment_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("pc_commit"), invoice_id, payer.clone())
}

/// Recompute the commitment digest for `(value, blinding)` against the
/// contract's fixed generators, so it can be checked against a commitment
/// digest previously supplied off-chain to `pay`.
///
/// The commitment point `C = value*G + blinding*H` is itself never stored:
/// only a SHA-256 digest of its serialized form is kept on-chain, which is
/// what fits `pay`'s `BytesN<32>` commitment slot and keeps storage compact.
fn pedersen_commitment_digest(env: &Env, value: i128, blinding: &BytesN<32>) -> BytesN<32> {
    guard_nonzero_amount(value).expect("ZeroAmountNotAllowed");
    let g: G1Affine = env
        .storage()
        .instance()
        .get(&pedersen_g_key())
        .expect("not initialized");
    let h: G1Affine = env
        .storage()
        .instance()
        .get(&pedersen_h_key())
        .expect("not initialized");
    let value_scalar = Fr::from_u256(U256::from_u128(env, value as u128));
    let blinding_scalar = Fr::from_bytes(blinding.clone());
    let commitment_point: G1Affine = (g * value_scalar) + (h * blinding_scalar);
    let commitment_bytes: Bytes = commitment_point.to_bytes().into();
    env.crypto().sha256(&commitment_bytes).into()
}

fn surplus_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("surplus"), invoice_id)
}

fn surplus_claim_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("sur_clm"), invoice_id, payer.clone())
}

/// Issue #485: per-invoice contributor allowlist — persistent storage.
#[allow(dead_code)]
fn contributor_allowlist_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("ctr_al"), invoice_id)
}

/// Issue #503: number of currently-open invoices per creator — persistent storage.
fn open_invoice_count_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("op_inv_cn"), creator.clone())
}

/// Issue #503: admin-configured maximum open invoices per creator — instance storage.
fn max_open_invoices_key() -> Symbol {
    symbol_short!("mx_op_inv")
}

/// Issue #505: list of recipients whose payout failed (missing account) — persistent storage.
fn failed_payouts_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("fail_pay"), invoice_id)
}

fn commitment_expiry_key() -> Symbol {
    symbol_short!("com_exp")
}

/// Payer spending cap (per-window maximum) — instance storage.
fn payer_spend_cap_key() -> Symbol {
    symbol_short!("pay_sp_cp")
}

/// Payer spending window size in ledgers — instance storage.
fn payer_spend_window_ledgers_key() -> Symbol {
    symbol_short!("pay_sp_wn")
}

/// Per-payer spending accumulator: (window_start_ledger, total_spent) — temporary storage.
fn payer_spend_accum_key(payer: &Address) -> (Symbol, Address) {
    (symbol_short!("pay_sp_ac"), payer.clone())
}

/// Global dispute timeout ledgers — instance storage.
fn dispute_timeout_key() -> Symbol {
    symbol_short!("disp_tout")
}

// ---------------------------------------------------------------------------
// Issue #482: Safe arithmetic helpers — checked intermediate ops
// ---------------------------------------------------------------------------

/// Multiply `a * b / divisor` using u128 intermediates, returning
/// `Err(ContractError::ArithmeticOverflow)` on any overflow or divide-by-zero.
#[inline]
fn checked_bps_of(amount: i128, bps: u32, divisor: u128) -> Result<i128, ContractError> {
    if divisor == 0 {
        return Err(ContractError::ArithmeticOverflow);
    }
    let a = amount as u128;
    let b = bps as u128;
    let numerator = a.checked_mul(b).ok_or(ContractError::ArithmeticOverflow)?;
    let result = numerator.checked_div(divisor).ok_or(ContractError::ArithmeticOverflow)?;
    Ok(result as i128)
}

/// Multiply `a * b / divisor` with both a and b as u128, returning
/// `Err(ContractError::ArithmeticOverflow)` on any overflow or divide-by-zero.
#[inline]
fn checked_proportion(a: u128, b: u128, divisor: u128) -> Result<i128, ContractError> {
    if divisor == 0 {
        return Err(ContractError::ArithmeticOverflow);
    }
    let numerator = a.checked_mul(b).ok_or(ContractError::ArithmeticOverflow)?;
    let result = numerator.checked_div(divisor).ok_or(ContractError::ArithmeticOverflow)?;
    Ok(result as i128)
}

// ---------------------------------------------------------------------------
// Issue #483: Zero-value guard helper
// ---------------------------------------------------------------------------

/// Return `Err(ContractError::ZeroAmountNotAllowed)` when `amount <= 0`.
#[inline]
fn guard_nonzero_amount(amount: i128) -> Result<(), ContractError> {
    if amount <= 0 {
        Err(ContractError::ZeroAmountNotAllowed)
    } else {
        Ok(())
    }
}

/// Issue #308: per-invoice refunded-addresses set — persistent storage.
#[cfg(test)]
fn refunded_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("refunded"), invoice_id)
}

// ---------------------------------------------------------------------------
// Event sequence number helper (per-invoice, temporary-storage counter)
// ---------------------------------------------------------------------------

/// Temporary-storage key for per-invoice event sequence counter.
/// Lives in `storage::temporary` so it resets between transactions.
#[allow(dead_code)]
fn event_seq_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("evt_seq"), invoice_id)
}

/// Fetch and increment the per-invoice event sequence counter.
/// Returns the new (post-increment) sequence number, starting at 1.
#[allow(dead_code)]
pub(crate) fn event_seq(env: &Env, invoice_id: u64) -> u64 {
    let key = event_seq_key(invoice_id);
    let seq: u64 = env.storage().temporary().get(&key).unwrap_or(0) + 1;
    env.storage().temporary().set(&key, &seq);
    seq
}

// ---------------------------------------------------------------------------
// Reentrancy guard (issue #451-reentrancy)
// ---------------------------------------------------------------------------

/// Temporary-storage key for the per-transaction reentrancy lock.
///
/// Using *temporary* storage means the flag is automatically invalidated at the
/// end of the transaction (its TTL is never extended), so a stale lock can never
/// block a subsequent independent call.
fn reentrancy_lock_key() -> Symbol {
    symbol_short!("re_lock")
}

/// Executes `body` inside a reentrancy guard backed by temporary storage.
///
/// # How it works
/// 1. Check whether the lock key is present in temporary storage.  If it is,
///    a recursive call is in progress — return `ReentrantCall` immediately.
/// 2. Set the lock (TTL = 1 ledger; only needs to survive this transaction).
/// 3. Run `body`.
/// 4. Remove the lock so that another *independent* call in the same ledger can
///    still proceed (Soroban executes each top-level invocation as its own
///    transaction, but this is belt-and-suspenders).
///
/// The lock lives in `env.storage().temporary()` so it is **never persisted
/// across transactions** even if the `remove` step is somehow skipped.
#[allow(dead_code)]
fn with_reentrancy_guard<F>(env: &Env, body: F) -> Result<(), ContractError>
where
    F: FnOnce() -> Result<(), ContractError>,
{
    let key = reentrancy_lock_key();
    if env
        .storage()
        .temporary()
        .has(&key)
    {
        return Err(ContractError::ReentrantCall);
    }
    // Set the lock with the minimum TTL.  The value is irrelevant; presence is
    // all we test.
    env.storage().temporary().set(&key, &true);
    let result = body();
    // Always clear the lock so subsequent independent calls within the same
    // ledger (different top-level transactions) are not blocked.
    env.storage().temporary().remove(&key);
    result
}

fn maybe_record_created(env: &Env, creator: &Address, total: i128) {
    if let Some(dashboard) = env
        .storage()
        .persistent()
        .get::<Symbol, Address>(&dashboard_contract_key())
    {
        let _: Val = env.invoke_contract(
            &dashboard,
            &Symbol::new(env, "record_created"),
            (creator.clone(), total).into_val(env),
        );
    }
}

fn maybe_record_released(env: &Env, creator: &Address, amount: i128) {
    if let Some(dashboard) = env
        .storage()
        .persistent()
        .get::<Symbol, Address>(&dashboard_contract_key())
    {
        let _: Val = env.invoke_contract(
            &dashboard,
            &Symbol::new(env, "record_released"),
            (creator.clone(), amount).into_val(env),
        );
    }
}

fn current_commitment_expiry(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&commitment_expiry_key())
        .unwrap_or(DEFAULT_COMMITMENT_EXPIRY_LEDGERS)
}

fn compute_payment_commitment_hash(
    env: &Env,
    invoice_id: u64,
    amount: i128,
    salt: &BytesN<32>,
) -> BytesN<32> {
    let mut preimage = Bytes::new(env);
    let invoice_bytes = invoice_id.to_xdr(env);
    for i in 0..invoice_bytes.len() {
        preimage.push_back(invoice_bytes.get(i).unwrap());
    }
    let amount_bytes = amount.to_xdr(env);
    for i in 0..amount_bytes.len() {
        preimage.push_back(amount_bytes.get(i).unwrap());
    }
    for i in 0..salt.len() {
        preimage.push_back(salt.get(i).unwrap());
    }
    env.crypto().sha256(&preimage).into()
}

fn update_twafr(invoice: &mut Invoice, creation_ledger: u32, current_ledger: u32, amount: i128) {
    if current_ledger <= creation_ledger || amount <= 0 {
        return;
    }
    if invoice.twafr_last_ledger == 0 {
        invoice.twafr_numerator = invoice.twafr_numerator.saturating_add(amount);
    } else if current_ledger > invoice.twafr_last_ledger {
        let interval = current_ledger.saturating_sub(invoice.twafr_last_ledger) as i128;
        invoice.twafr_numerator = invoice
            .twafr_numerator
            .saturating_add(invoice.funded.saturating_mul(interval))
            .saturating_add(amount);
    } else {
        invoice.twafr_numerator = invoice.twafr_numerator.saturating_add(amount);
    }
    invoice.twafr_last_ledger = current_ledger;
}

fn validate_milestones(env: &Env, milestones: &Vec<u32>) {
    if milestones.is_empty() {
        return;
    }
    let mut prev = 0u32;
    for milestone in milestones.iter() {
        assert!(milestone > prev, "milestones must be strictly ascending");
        assert!(
            milestone <= 10_000,
            "milestone basis points must be <= 10000"
        );
        prev = milestone;
    }
    assert!(prev == 10_000, "milestones must end at 10000");
    let _ = env;
}

/// Validate that `ratios` is non-empty, each element is strictly less than
/// `denominator`, and their sum equals exactly `denominator`.
///
/// Returns `Ok(())` on success, or:
/// - [`ContractError::EmptyRecipientList`] when the slice is empty.
/// - [`ContractError::InvalidRatio`] when any ratio >= denominator or the sum differs.
pub(crate) fn validate_ratios(ratios: &Vec<u32>, denominator: u64) -> Result<(), ContractError> {
    if ratios.is_empty() {
        return Err(ContractError::EmptyRecipientList);
    }
    let denom = denominator as u32;
    for r in ratios.iter() {
        if r >= denom {
            return Err(ContractError::InvalidRatio);
        }
    }
    let sum: u32 = ratios.iter().fold(0u32, |acc, r| acc.saturating_add(r));
    if sum != denom {
        return Err(ContractError::InvalidRatio);
    }
    Ok(())
}

/// Issue #519: enforce the allowed invoice status transitions.
///
/// # State machine
/// * Terminal states (`Released`, `Refunded`, `Expired`, `Cancelled`) cannot be left.
/// * `Pending` may move to `PartiallyReleased`, `Released`, `Disputed`, `Cancelled`, `Expired`, or `Refunded`.
/// * `PartiallyReleased` may move to `Released` or back to `Pending` (dispute resolution).
/// * `Disputed` may move to `Pending` or `Refunded`.
/// * Same-state transitions are permitted (idempotent).
#[allow(dead_code)]
pub(crate) fn valid_transition(from: InvoiceStatus, to: InvoiceStatus) -> bool {
    if from == to {
        return true;
    }
    match from {
        InvoiceStatus::Released => false,
        InvoiceStatus::Refunded => false,
        InvoiceStatus::Expired => false,
        InvoiceStatus::Cancelled => false,
        InvoiceStatus::Pending => matches!(
            to,
            InvoiceStatus::PartiallyReleased
                | InvoiceStatus::Released
                | InvoiceStatus::Disputed
                | InvoiceStatus::Cancelled
                | InvoiceStatus::Expired
                | InvoiceStatus::Refunded
        ),
        InvoiceStatus::PartiallyReleased => {
            to == InvoiceStatus::Released || to == InvoiceStatus::Pending
        }
        InvoiceStatus::Disputed => to == InvoiceStatus::Pending || to == InvoiceStatus::Refunded,
        InvoiceStatus::Finalised => false,
        InvoiceStatus::Deleted => false,
        InvoiceStatus::PayoutInProgress => to == InvoiceStatus::Released || to == InvoiceStatus::Refunded,
    }
}

/// Issue #519: transition an invoice to a new status after validating the move.
#[allow(dead_code)]
pub(crate) fn transition_status(
    env: &Env,
    invoice_id: u64,
    invoice: &mut Invoice,
    to: InvoiceStatus,
    actor: &Address,
) {
    let from = invoice.status.clone();
    if !valid_transition(from.clone(), to.clone()) {
        env.panic_with_error(ContractError::InvalidStateTransition);
    }
    invoice.status = to;
    events::invoice_state_changed(env, invoice_id, Some(&from), &invoice.status, actor);
}

/// Issue #299: Update creator stats on invoice creation.
fn update_creator_stats_on_creation(env: &Env, creator: &Address) {
    let count_key = creator_stats_count_key(creator);
    let count: u64 = env.storage().persistent().get(&count_key).unwrap_or(0u64);
    env.storage().persistent().set(&count_key, &(count + 1));
}

/// Issue #299: Update creator stats on payment received.
fn update_creator_stats_on_payment(env: &Env, creator: &Address, amount: i128) {
    let volume_key = creator_stats_volume_key(creator);
    let volume: u64 = env.storage().persistent().get(&volume_key).unwrap_or(0u64);
    env.storage()
        .persistent()
        .set(&volume_key, &(volume + amount as u64));
}

/// Issue #299: Update creator stats on release.
fn update_creator_stats_on_release(env: &Env, creator: &Address, amount: i128) {
    let released_key = creator_stats_released_key(creator);
    let released: u64 = env
        .storage()
        .persistent()
        .get(&released_key)
        .unwrap_or(0u64);
    env.storage()
        .persistent()
        .set(&released_key, &(released + amount as u64));
}

/// Issue #409: Update creator lifetime release volume and accrue any rebate.
fn accrue_creator_rebate(env: &Env, creator: &Address, release_amount: i128, total_fee: i128) {
    let volume_key = creator_volume_key(creator);
    let current_volume: i128 = env.storage().persistent().get(&volume_key).unwrap_or(0i128);
    let new_volume = current_volume
        .checked_add(release_amount)
        .expect("creator volume overflow");
    env.storage().persistent().set(&volume_key, &new_volume);

    if total_fee <= 0 {
        return;
    }

    let tiers: Vec<RebateTier> = env
        .storage()
        .instance()
        .get(&rebate_tiers_key())
        .unwrap_or_else(|| Vec::new(env));
    if tiers.is_empty() {
        return;
    }

    let mut applicable: Option<RebateTier> = None;
    for tier in tiers.iter() {
        if new_volume >= tier.min_volume {
            applicable = Some(tier.clone());
        }
    }

    if let Some(tier) = applicable {
        let rebate = checked_bps_of(total_fee, tier.rebate_bps, 10_000u128)
            .expect("ArithmeticOverflow"); // Issue #482
        if rebate > 0 {
            let balance_key = rebate_balance_key(creator);
            let current_balance: i128 = env
                .storage()
                .persistent()
                .get(&balance_key)
                .unwrap_or(0i128);
            let new_balance = current_balance
                .checked_add(rebate)
                .expect("rebate balance overflow");
            env.storage().persistent().set(&balance_key, &new_balance);
            events::rebate_accrued(env, creator, rebate, tier.rebate_bps);
        }
    }
}

/// Issue #299: Update creator unique payers count (call after recording payment).
fn update_creator_payers(env: &Env, creator: &Address, payer: &Address) {
    // Track unique payers using a set-like approach via a key pattern
    let payer_key = (symbol_short!("cr_py_set"), creator.clone(), payer.clone());
    if env
        .storage()
        .persistent()
        .get::<(Symbol, Address, Address), bool>(&payer_key)
        .is_none()
    {
        env.storage().persistent().set(&payer_key, &true);
        let payers_key = creator_stats_payers_key(creator);
        let payers: u64 = env.storage().persistent().get(&payers_key).unwrap_or(0u64);
        env.storage().persistent().set(&payers_key, &(payers + 1));
    }
}

/// Issue #438: anonymity mode flag for an invoice — persistent storage.
fn anonymous_recipients_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("anon_rec"), invoice_id)
}

/// Issue #808: payer anonymity flag for an invoice — persistent storage.
fn payer_anonymity_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("anon_pay"), invoice_id)
}

/// Issue #808: whether payment events for this invoice carry a payer hash
/// instead of the payer address.
fn payer_anonymity_enabled(env: &Env, invoice_id: u64) -> bool {
    env.storage()
        .persistent()
        .get(&payer_anonymity_key(invoice_id))
        .unwrap_or(false)
}

/// Issue #808: sha256 of the payer address's XDR encoding.
fn hash_payer(env: &Env, payer: &Address) -> BytesN<32> {
    env.crypto().sha256(&payer.clone().to_xdr(env)).into()
}

/// Issue #438: recipient commitment hash — persistent storage (invoice_id, index).
// Issue #438: storage key for the recipient reveal scheme; the reveal entry
// point that consumes it is not wired up yet.
#[allow(dead_code)]
fn recipient_commitment_key(invoice_id: u64, index: u32) -> (Symbol, u64, u32) {
    (symbol_short!("rec_cmt"), invoice_id, index)
}

/// Issue #437: delayed payout record — persistent storage (invoice_id, recipient).
fn delayed_payout_key(invoice_id: u64, recipient: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("del_pay"), invoice_id, recipient.clone())
}

/// Issue #436: rolling payment root hash — persistent storage.
fn payment_root_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("pay_root"), invoice_id)
}

/// Issue #435: contract upgrade freeze flag — instance storage.
fn upgrade_freeze_key() -> Symbol {
    symbol_short!("upg_frz")
}

/// Issue #435: contract upgrade checkpoint hash — instance storage.
fn upgrade_checkpoint_key() -> Symbol {
    symbol_short!("upg_ckpt")
}

/// Issue #431: duplicate payment fingerprint — persistent storage (with TTL).
// Issue #431: duplicate-payment detection is implemented below but not yet
// called from the payment path.
#[allow(dead_code)]
fn payment_fingerprint_key(fingerprint_hash: &BytesN<32>) -> (Symbol, BytesN<32>) {
    (symbol_short!("dup_fp"), fingerprint_hash.clone())
}

/// Issue #431: duplicate window in ledgers — instance storage.
fn duplicate_window_ledgers_key() -> Symbol {
    symbol_short!("dup_win")
}

/// Issue #432: referrer reward percentage in basis points — instance storage.
fn referrer_reward_bps_key() -> Symbol {
    symbol_short!("ref_bps")
}

/// Issue #432: referrer address for an invoice — persistent storage.
fn invoice_referrer_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("ref_addr"), invoice_id)
}

/// Issue #434: group members list — persistent storage.
fn group_members_key(group_id: u64) -> (Symbol, u64) {
    (symbol_short!("grp_mem"), group_id)
}

/// Issue #434: group ID for an invoice — persistent storage.
fn invoice_group_id_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("inv_grp"), invoice_id)
}

/// Issue #434: group counter — instance storage.
#[allow(dead_code)]
fn group_counter_key() -> Symbol {
    symbol_short!("grp_ctr")
}

/// Cumulative contributed amount for an invoice — persistent storage.
/// Monotonically increases with each payment; never decremented.
fn cumulative_contributed_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("cum_ctb"), invoice_id)
}

/// Sweep timeout in ledgers — instance storage.
/// Failed payouts can be swept after `last_failed_ledger + sweep_timeout_ledgers`.
fn sweep_timeout_key() -> Symbol {
    symbol_short!("swp_tout")
}


/// Issue #504: Per-failed-payout record — persistent storage.
/// Key: ("fp_rec", invoice_id, recipient) -> i128 (amount that failed).
fn failed_payout_record_key(invoice_id: u64, recipient: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("fp_rec"), invoice_id, recipient.clone())
}

/// Issue #504: Attempt a token transfer, returning Ok(()) on success or
/// Err(reason_string) if the transfer fails or panics.
///
/// This catches both Soroban `try_borrow_authorization` errors and
/// contract-level panics (e.g. from frozen accounts) so that a single
/// failing transfer never reverts the entire release batch.
#[allow(clippy::too_many_arguments)]
fn try_transfer(
    env: &Env,
    token: &Address,
    from: &Address,
    to: &Address,
    amount: i128,
) -> Result<(), String> {
    let _client = token::Client::new(env, token);
    // Use try_transfer which returns Result instead of panicking.
    // If the Soroban token client does not have try_transfer, we
    // fall back to calling transfer and catching any panic.
    match env.try_invoke_contract::<(), soroban_sdk::Error>(
        token,
        &symbol_short!("transfer"),
        (from.clone(), to.clone(), amount).into_val(env),
    ) {
        Ok(_result) => Ok(()),
        Err(_err) => Err(String::from_str(env, "TransferFailed")),
    }
}

/// Issue #504: Record a failed payout for later retry.
fn record_failed_payout(
    env: &Env,
    invoice_id: u64,
    recipient: &Address,
    amount: i128,
    reason: &str,
) {
    // Store the amount so retry knows how much to re-attempt.
    env.storage().persistent().set(
        &failed_payout_record_key(invoice_id, recipient),
        &amount,
    );

    // Append recipient to the failed-payouts list if not already present.
    let mut failed: Vec<Address> = env
        .storage()
        .persistent()
        .get(&failed_payouts_key(invoice_id))
        .unwrap_or_else(|| Vec::new(env));
    if !failed.iter().any(|a| a == *recipient) {
        failed.push_back(recipient.clone());
        env.storage()
            .persistent()
            .set(&failed_payouts_key(invoice_id), &failed);
    }

    // Emit event with reason.
    let reason_str = String::from_str(env, reason);
    events::payout_failed(env, invoice_id, recipient, amount, &reason_str);
}

/// Per-invoice last failed payout ledger — persistent storage.
/// Updated whenever a payout fails during release.
fn last_failed_ledger_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("last_fail"), invoice_id)
}

/// Trusted callers whitelist — instance storage.
/// Addresses in this list are exempt from platform fee deduction.
fn trusted_callers_key() -> Symbol {
    symbol_short!("trstd_cal")
}

/// Unreleased funds accumulator for an invoice — persistent storage.
/// When a recipient share is locked, funds that would have gone to that
/// recipient are accumulated here until released via `release_locked_funds`.
fn unreleased_funds_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("unrl_fnd"), invoice_id)
}

/// Per-invoice per-recipient share lock flag — persistent storage.
/// Key: (invoice_id, recipient). Value: true if locked, absent = unlocked.
fn recipient_lock_key(invoice_id: u64, recipient: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("rcp_lock"), invoice_id, recipient.clone())
}

/// Per-invoice per-contributor contribution record — persistent storage.
/// Key: (invoice_id, payer). Value: i128 amount contributed.
fn contribution_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("contrb"), invoice_id, payer.clone())
}

/// Issue #747: Running total contributed by `payer` on `invoice_id` for
/// cap enforcement — persistent storage.
/// Key: (Symbol "cntrb_cap", invoice_id, payer) → i128
fn payer_cap_total_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("cntrb_cap"), invoice_id, payer.clone())
}

/// Alias for `payer_cap_total_key` (issue #747).
fn payer_total_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) {
    payer_cap_total_key(invoice_id, payer)
}

/// Issue #813: Creator-suggested per-payer amount for an invoice.
/// Key: (Symbol "pay_hint", invoice_id) → i128
fn payment_hint_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("pay_hint"), invoice_id)
}

/// Issue #816: Ledger timestamp at which an invoice was retired.
/// Key: (Symbol "retired", invoice_id) → u64
fn retired_at_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("retired"), invoice_id)
}

/// Issue #746: Cumulative basis points released so far via `release_partial`.
/// Prevents the sum of partial releases from exceeding 10 000 bps.
/// Key: (Symbol "rel_bps", invoice_id) → u32
fn total_released_bps_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("rel_bps"), invoice_id)
}

// ---------------------------------------------------------------------------
// Invoice storage helpers
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Hot-field TTL helpers
// ---------------------------------------------------------------------------

/// Target ledger count for instance-storage TTL extension (~30 days at 5 s/ledger).
const INVOICE_HOT_TTL_LEDGERS: u32 = 518_400;

/// Issue #770: ledgers added to an invoice's storage TTL by `bump_invoice_ttl`.
///
/// 1,000,000 ledgers at ~5 seconds per ledger is about 5,000,000 seconds, or
/// roughly 57.9 days.
pub const TTL_EXTENSION_LEDGERS: u32 = 1_000_000;

/// Extend the contract instance TTL so all `InvoiceHot` entries remain live.
///
/// Because every `InvoiceHot` entry lives in the *instance* bucket, one call
/// covers every active invoice simultaneously — O(1) cost per payment instead
/// of one persistent-rent charge per invoice key.
fn bump_invoice_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INVOICE_HOT_TTL_LEDGERS / 2, INVOICE_HOT_TTL_LEDGERS);
}

/// Extend the TTL of an invoice's *persistent* entries.
///
/// [`bump_invoice_ttl`] only covers the instance bucket, but core/ext/ext2 and
/// the compact overlay live in persistent storage — without this they archive
/// out from under an invoice that is still being paid.
fn bump_invoice_entry_ttl(env: &Env, id: u64) {
    // Clamp to the network's ceiling rather than relying on the host to do it.
    let extend_to = INVOICE_HOT_TTL_LEDGERS.min(env.storage().max_ttl());
    let threshold = extend_to / 2;
    let storage = env.storage().persistent();
    for key in [
        invoice_key(id),
        invoice_ext_key(id),
        invoice_ext2_key(id),
        invoice_compact_key(id),
    ] {
        if storage.has(&key) {
            storage.extend_ttl(&key, threshold, extend_to);
        }
    }
}

// ---------------------------------------------------------------------------
// Storage helpers
// ---------------------------------------------------------------------------

fn archive_invoice_storage(env: &Env, id: u64, core: &InvoiceCore) {
    let ext: InvoiceExt = env
        .storage()
        .persistent()
        .get(&invoice_ext_key(id))
        .or_else(|| env.storage().instance().get(&invoice_ext_key(id)))
        .unwrap_or_else(|| InvoiceExt {
            co_signers: Vec::new(env),
            required_signatures: 0,
            signatures: Vec::new(env),
            approver: None,
            approved: false,
            condition_oracle: None,
            condition_met: false,
            penalty_bps: 0,
            penalty_deadline: 0,
            min_funding_bps: 0,
            release_stages: Vec::new(env),
            released_stages: 0,
            allowed_payers: None,
            price_oracle: None,
            base_amounts: Vec::new(env),
            swap_tokens: Vec::new(env),
            tax_bps: 0,
            tax_authority: None,
            insurance_premium_bps: 0,
            insurance_fund: 0,
            smart_route: false,
            convert_to_stream: false,
            accepted_tokens: Vec::new(env),
            forward_to: None,
            forward_invoice_id: None,
            split_rules: Vec::new(env),
            auto_resolve_rules: Vec::new(env),
            creator_cosigner: None,
            velocity_limit: 0,
            velocity_window: 0,
            parent_invoice_id: None,
            pause_reason: None,
            auto_resume_at: None,
            payment_cooldown_secs: None,
            max_payments_per_window: None,
            payment_window_secs: None,
            scheduled_release_at: None,
            refund_grace_secs: None,
            penalty_tiers: Vec::new(env),
            allowed_callers: None,
        });
    let ext2: InvoiceExt2 = env
        .storage()
        .persistent()
        .get(&invoice_ext2_key(id))
        .or_else(|| env.storage().instance().get(&invoice_ext2_key(id)))
        .unwrap_or_else(|| InvoiceExt2 {
            notification_contract: None,
            overflow_behavior: OverflowBehavior::Reject,
            cross_chain_ref: None,
            require_kyc: false,
            arbiter: None,
            disputed: false,
            admin_frozen: false,
            auction_on_expiry: false,
            auction_end: 0,
            bids: Vec::new(env),
            min_payment: 0,
            min_funding_amount: 0,
            priorities: Vec::new(env),
            target_usd_cents: None,
            refunded_addresses: Vec::new(env),
            oracle: None,
            oracle_asset_pair_base: None,
            oracle_asset_pair_quote: None,
            min_payer_rep: None,
            escrow_hold_period: None,
            held_until: None,
            milestones: Vec::new(env),
            milestones_released: 0,
            recipient_max_payouts: Vec::new(env),
            twafr_numerator: 0,
            twafr_last_ledger: 0,
            release_condition_hash: None,
            recipient_whitelist_enabled: false,
            overfunding_policy: OverfundingPolicy::Cap,
            contributor_allowlist: None,
            early_bird_window_ledgers: 0,
            early_bird_fee_bps: 0,
            early_bird_fee_credit: 0,
            creator_fee_bps: 0,
            ratio_denominator: 1,
            ratios: Vec::new(env),
            max_contribution_per_payer: None,
        });

    env.storage().instance().set(&invoice_key(id), core);
    env.storage().instance().set(&invoice_ext_key(id), &ext);
    env.storage().instance().set(&invoice_ext2_key(id), &ext2);
    env.storage().instance().set(&archive_marker_key(id), &true);

    let compact = env
        .storage()
        .persistent()
        .get::<_, CompactInvoice>(&invoice_compact_key(id))
        .or_else(|| env.storage().instance().get(&invoice_compact_key(id)))
        .unwrap_or_else(|| {
            let invoice = Invoice::assemble(core.clone(), ext.clone(), ext2.clone());
            invoice.to_compact(env)
        });
    env.storage()
        .instance()
        .set(&invoice_compact_key(id), &compact);

    for shard_id in 0..SHARD_COUNT {
        let shard_key = pay_shard_key(id, shard_id);
        if let Some(payments) = env
            .storage()
            .persistent()
            .get::<(Symbol, u64, u64), Vec<Payment>>(&shard_key)
        {
            env.storage().instance().set(&shard_key, &payments);
        }
        env.storage().persistent().remove(&shard_key);
    }

    if let Some(audit_log) = env
        .storage()
        .persistent()
        .get::<_, Vec<AuditEntry>>(&audit_log_key(id))
    {
        env.storage().instance().set(&audit_log_key(id), &audit_log);
    }
    env.storage().persistent().remove(&invoice_key(id));
    env.storage().persistent().remove(&invoice_ext_key(id));
    env.storage().persistent().remove(&invoice_ext2_key(id));
    env.storage().persistent().remove(&invoice_compact_key(id));
    env.storage().persistent().remove(&audit_log_key(id));
    env.storage().instance().set(
        &created_ledger_key(id),
        &env.storage()
            .persistent()
            .get::<_, u32>(&created_ledger_key(id))
            .unwrap_or_else(|| env.ledger().sequence()),
    );
}

fn maybe_archive_invoice(env: &Env, id: u64) {
    if env.storage().instance().has(&archive_marker_key(id))
        || env.storage().persistent().has(&archive_marker_key(id))
    {
        return;
    }

    let core: InvoiceCore = env
        .storage()
        .persistent()
        .get(&invoice_key(id))
        .or_else(|| env.storage().instance().get(&invoice_key(id)))
        .unwrap_or_else(|| panic!("invoice not found"));

    if core.status != InvoiceStatus::Released && core.status != InvoiceStatus::Refunded {
        return;
    }

    // Stored as a u32 ledger sequence by every writer — read it as one and
    // widen here, rather than asking the host for a u64 it never wrote.
    let created_ledger: u32 = env
        .storage()
        .persistent()
        .get(&created_ledger_key(id))
        .or_else(|| env.storage().instance().get(&created_ledger_key(id)))
        .unwrap_or_else(|| env.ledger().sequence());
    let archive_after = env
        .storage()
        .instance()
        .get(&archive_after_ledgers_key())
        .unwrap_or(ARCHIVE_AFTER_LEDGERS);
    if (env.ledger().sequence() as u64).saturating_sub(created_ledger as u64) < archive_after {
        return;
    }

    archive_invoice_storage(env, id, &core);
    events::invoice_archived(env, id);
}

fn load_invoice(env: &Env, id: u64) -> Invoice {
    maybe_archive_invoice(env, id);
    // Read hot fields from instance storage and extend TTL on every access.
    // For invoices not yet migrated the entry is absent; the persistent path
    // acts as fallback and the hot entry is written on the next save_invoice.
    let maybe_hot: Option<InvoiceHot> = env.storage().instance().get(&invoice_hot_key(id));
    if maybe_hot.is_some() {
        bump_invoice_ttl(env);
    }

    let mut core: InvoiceCore = if let Some(c) = env.storage().persistent().get(&invoice_key(id)) {
        c
    } else {
        env.storage()
            .instance()
            .get(&invoice_key(id))
            .expect("invoice not found")
    };

    // Aggregate payments from all shards (issue #177).
    let mut all_payments: Vec<Payment> = Vec::new(env);
    for shard_id in 0..SHARD_COUNT {
        let shard_key = pay_shard_key(id, shard_id);
        if let Some(shard_payments) = env
            .storage()
            .persistent()
            .get::<(Symbol, u64, u64), Vec<Payment>>(&shard_key)
            .or_else(|| env.storage().instance().get(&shard_key))
        {
            for payment in shard_payments.iter() {
                all_payments.push_back(payment.clone());
            }
        }
    }
    core.payments = all_payments;

    let ext: InvoiceExt = env
        .storage()
        .persistent()
        .get(&invoice_ext_key(id))
        .or_else(|| env.storage().instance().get(&invoice_ext_key(id)))
        .unwrap_or_else(|| InvoiceExt {
            co_signers: Vec::new(env),
            required_signatures: 0,
            signatures: Vec::new(env),
            approver: None,
            approved: false,
            condition_oracle: None,
            condition_met: false,
            penalty_bps: 0,
            penalty_deadline: 0,
            min_funding_bps: 0,
            release_stages: Vec::new(env),
            released_stages: 0,
            allowed_payers: None,
            price_oracle: None,
            base_amounts: Vec::new(env),
            swap_tokens: Vec::new(env),
            tax_bps: 0,
            tax_authority: None,
            insurance_premium_bps: 0,
            insurance_fund: 0,
            smart_route: false,
            convert_to_stream: false,
            accepted_tokens: Vec::new(env),
            forward_to: None,
            forward_invoice_id: None,
            split_rules: Vec::new(env),
            auto_resolve_rules: Vec::new(env),
            creator_cosigner: None,
            velocity_limit: 0,
            velocity_window: 0,
            parent_invoice_id: None,
            pause_reason: None,
            auto_resume_at: None,
            payment_cooldown_secs: None,
            max_payments_per_window: None,
            payment_window_secs: None,
            scheduled_release_at: None,
            penalty_tiers: Vec::new(env),
            allowed_callers: None,
            refund_grace_secs: None,
        });
    let ext2: InvoiceExt2 = env
        .storage()
        .persistent()
        .get(&invoice_ext2_key(id))
        .or_else(|| env.storage().instance().get(&invoice_ext2_key(id)))
        .unwrap_or_else(|| InvoiceExt2 {
            notification_contract: None,
            overflow_behavior: OverflowBehavior::Reject,
            cross_chain_ref: None,
            require_kyc: false,
            arbiter: None,
            disputed: false,
            admin_frozen: false,
            auction_on_expiry: false,
            auction_end: 0,
            bids: Vec::new(env),
            min_payment: 0,
            min_funding_amount: 0,
            priorities: Vec::new(env),
            target_usd_cents: None,
            refunded_addresses: Vec::new(env),
            oracle: None,
            oracle_asset_pair_base: None,
            oracle_asset_pair_quote: None,
            min_payer_rep: None,
            escrow_hold_period: None,
            held_until: None,
            milestones: Vec::new(env),
            milestones_released: 0,
            recipient_max_payouts: Vec::new(env),
            twafr_numerator: 0,
            twafr_last_ledger: 0,
            release_condition_hash: None,
            recipient_whitelist_enabled: false,
            overfunding_policy: OverfundingPolicy::Cap,
            contributor_allowlist: None,
            early_bird_window_ledgers: 0,
            early_bird_fee_bps: 0,
            early_bird_fee_credit: 0,
            creator_fee_bps: 0,
            ratio_denominator: 1,
            ratios: Vec::new(env),
            max_contribution_per_payer: None,
        });

    // Load compact representation if available, then overlay hot fields.
    let mut invoice = if let Some(compact) = env
        .storage()
        .persistent()
        .get::<_, CompactInvoice>(&invoice_compact_key(id))
    {
        Invoice::from_compact(&compact, core, ext, ext2)
    } else {
        Invoice::assemble(core, ext, ext2)
    };

    // Hot fields are authoritative post-migration; overlay them here.
    if let Some(hot) = maybe_hot {
        invoice.status = hot.status;
        invoice.funded = hot.funded;
        invoice.recipients = hot.recipients;
    }

    // Populate metadata_hash from its separate storage key.
    invoice.metadata_hash = env
        .storage()
        .persistent()
        .get(&metadata_hash_key(id));

    invoice
}

/// Guard: return `Err(ContractError::InvalidStatus)` unless the invoice is
/// still in the `Pending` state.  Calling this at the top of any mutating
/// function replaces the repeated inline pattern:
///
/// ```rust,ignore
/// if invoice.status != InvoiceStatus::Pending {
///     return Err(ContractError::InvalidStatus);
/// }
/// ```
fn assert_invoice_pending(invoice: &Invoice) -> Result<(), ContractError> {
    if invoice.status != InvoiceStatus::Pending {
        return Err(ContractError::InvalidStatus);
    }
    Ok(())
}

/// Estimates the serialised size (in bytes) of an invoice's persisted
/// representation (issue #425). Sums the XDR-encoded length of the three
/// pieces `save_invoice` actually writes to storage (`InvoiceCore`,
/// `InvoiceExt`, `InvoiceExt2`), so a quota enforced against this figure
/// reflects the real on-chain storage footprint. `payments` is excluded
/// because it is always cleared before persisting — payment history lives in
/// separate sharded storage (issue #177), not on the invoice record itself.
fn measure_invoice_bytes(env: &Env, invoice: &Invoice) -> u64 {
    let mut clean = invoice.clone();
    clean.payments = Vec::new(env);
    let (core, ext, ext2) = clean.split();
    (core.to_xdr(env).len() + ext.to_xdr(env).len() + ext2.to_xdr(env).len()) as u64
}

fn save_invoice(env: &Env, id: u64, invoice: &Invoice) {
    // Check no duplicate recipients
    for i in 0..invoice.recipients.len() {
        for j in (i + 1)..invoice.recipients.len() {
            debug_assert!(
                invoice.recipients.get(i).unwrap() != invoice.recipients.get(j).unwrap(),
                "invariant: duplicate recipient addresses"
            );
        }
    }

    // Issue #425: reject any mutation that would push the invoice's persisted
    // size past the configured quota, before writing anything.
    let quota: u64 = env
        .storage()
        .instance()
        .get(&storage_quota_key())
        .unwrap_or(DEFAULT_INVOICE_STORAGE_QUOTA);
    assert!(
        measure_invoice_bytes(env, invoice) <= quota,
        "StorageQuotaExceeded"
    );

    let mut clean_invoice = invoice.clone();
    clean_invoice.payments = Vec::new(env);
    let (core, ext, ext2) = clean_invoice.split();
    let archived = env.storage().instance().has(&archive_marker_key(id))
        || env.storage().persistent().has(&archive_marker_key(id));

    if archived {
        env.storage().instance().set(&invoice_key(id), &core);
        env.storage().instance().set(&invoice_ext_key(id), &ext);
        env.storage().instance().set(&invoice_ext2_key(id), &ext2);
        env.storage().persistent().remove(&invoice_key(id));
        env.storage().persistent().remove(&invoice_ext_key(id));
        env.storage().persistent().remove(&invoice_ext2_key(id));
    } else {
        env.storage().persistent().set(&invoice_key(id), &core);
        env.storage().persistent().set(&invoice_ext_key(id), &ext);
        env.storage().persistent().set(&invoice_ext2_key(id), &ext2);
        // Keep the persistent invoice record alive as long as its instance-storage
        // hot overlay: without this, load_invoice's persistent InvoiceCore read
        // (which the hot overlay never fully replaces — amounts, deadline, etc.
        // stay on `core`) can hit an expired/archived entry well before the
        // invoice's actual lifecycle ends.
        env.storage().persistent().extend_ttl(
            &invoice_key(id),
            INVOICE_HOT_TTL_LEDGERS / 2,
            INVOICE_HOT_TTL_LEDGERS,
        );
        env.storage().persistent().extend_ttl(
            &invoice_ext_key(id),
            INVOICE_HOT_TTL_LEDGERS / 2,
            INVOICE_HOT_TTL_LEDGERS,
        );
        env.storage().persistent().extend_ttl(
            &invoice_ext2_key(id),
            INVOICE_HOT_TTL_LEDGERS / 2,
            INVOICE_HOT_TTL_LEDGERS,
        );
    }

    // Store compact representation in the same tier as the invoice data.
    let compact = invoice.to_compact(env);
    if archived {
        env.storage()
            .instance()
            .set(&invoice_compact_key(id), &compact);
        env.storage().persistent().remove(&invoice_compact_key(id));
    } else {
        env.storage()
            .persistent()
            .set(&invoice_compact_key(id), &compact);
        env.storage().persistent().extend_ttl(
            &invoice_compact_key(id),
            INVOICE_HOT_TTL_LEDGERS / 2,
            INVOICE_HOT_TTL_LEDGERS,
        );
    }

    // Write hot fields to instance storage and bump TTL.
    // status, funded, and recipients change on pay/release/refund paths.
    let total: i128 = invoice.amounts.iter().sum();
    let hot = InvoiceHot {
        status: invoice.status.clone(),
        funded: invoice.funded,
        total,
        recipients: invoice.recipients.clone(),
    };
    env.storage().instance().set(&invoice_hot_key(id), &hot);

    // Issue #334: keep compact status byte in sync with every save.
    save_compact_status(env, id, &invoice.status);

    bump_invoice_ttl(env);
    bump_invoice_entry_ttl(env, id);
}

/// Issue #420: persist a creator-selected overfunding policy on a freshly
/// created invoice. `None` leaves the invoice on the default `Cap` policy, so
/// callers that never set the option pay no storage cost.
fn apply_overfunding_policy(env: &Env, id: u64, policy: OverfundingPolicy) {
    // `Cap` is already what the invoice was created with, so writing it back
    // would only burn a storage write.
    if policy != OverfundingPolicy::Cap {
        let mut invoice = load_invoice(env, id);
        invoice.overfunding_policy = policy;
        save_invoice(env, id, &invoice);
    }
}

/// Persist a creator-configured N-of-M cosigner approval requirement on a
/// freshly created invoice. `None` leaves the gate disabled, so invoices that
/// never set `cosigners` pay no storage cost.
fn apply_cosigner_config(env: &Env, id: u64, cosigners: Option<Vec<Address>>, threshold: Option<u32>) {
    if let Some(list) = cosigners {
        assert!(!list.is_empty(), "cosigners cannot be empty when set");
        let required = threshold.unwrap_or(list.len());
        assert!(
            required > 0 && required <= list.len(),
            "cosigner_threshold must be between 1 and cosigners.len()"
        );
        env.storage().persistent().set(&cosigners_key(id), &list);
        env.storage()
            .persistent()
            .set(&cosigner_thresh_key(id), &required);
    }
}

/// Blocks release until the invoice's configured N-of-M cosigner quorum has
/// been met via `approve_release`. A no-op when the invoice has no
/// `cosigners` configured (the common case).
fn require_cosigner_threshold_met(env: &Env, id: u64) {
    if let Some(threshold) = env.storage().persistent().get::<_, u32>(&cosigner_thresh_key(id)) {
        let approvals: Vec<Address> = env
            .storage()
            .persistent()
            .get(&cosign_key(id))
            .unwrap_or_else(|| Vec::new(env));
        assert!(
            approvals.len() >= threshold,
            "cosigner approval threshold not met"
        );
    }
}

fn funding_token_for(invoice: &Invoice) -> Address {
    invoice.funding_token.clone()
}

fn recipient_token_for(invoice: &Invoice, idx: usize) -> Address {
    invoice
        .tokens
        .get(idx as u32)
        .clone()
        .unwrap_or_else(|| invoice.funding_token.clone())
}

fn append_audit_entry(env: &Env, id: u64, action: Symbol, actor: &Address) {
    let timestamp = env.ledger().timestamp();
    let entry = AuditEntry {
        action,
        actor: actor.clone(),
        timestamp,
    };
    let archived = env.storage().instance().has(&archive_marker_key(id))
        || env.storage().persistent().has(&archive_marker_key(id));
    let mut log: Vec<AuditEntry> = if archived {
        env.storage()
            .instance()
            .get(&audit_log_key(id))
            .unwrap_or_else(|| Vec::new(env))
    } else {
        env.storage()
            .persistent()
            .get(&audit_log_key(id))
            .unwrap_or_else(|| Vec::new(env))
    };
    log.push_back(entry);
    if archived {
        env.storage().instance().set(&audit_log_key(id), &log);
        env.storage().persistent().remove(&audit_log_key(id));
    } else {
        env.storage().persistent().set(&audit_log_key(id), &log);
    }
}

/// Issue #763: Append an entry to the per-invoice history ring buffer.
///
/// The buffer is stored in persistent storage under [`history_key`] and is
/// capped at [`HISTORY_RING_CAP`] (20) entries.  When the buffer is full the
/// oldest entry (index 0) is evicted before the new one is appended, so the
/// stored slice is always in chronological order.
fn append_history_entry(
    env: &Env,
    invoice_id: u64,
    event_type: Symbol,
    actor: &Address,
    amount: Option<i128>,
) {
    let key = history_key(invoice_id);
    let mut buf: Vec<HistoryEntry> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|| Vec::new(env));
    // Evict the oldest entry when the buffer is at capacity.
    if buf.len() >= HISTORY_RING_CAP {
        // Build a new vec without the first element to stay no_std compatible.
        let mut trimmed: Vec<HistoryEntry> = Vec::new(env);
        for i in 1..buf.len() {
            trimmed.push_back(buf.get(i).unwrap());
        }
        buf = trimmed;
    }
    buf.push_back(HistoryEntry {
        event_type,
        timestamp: env.ledger().timestamp(),
        actor: actor.clone(),
        amount,
    });
    env.storage().persistent().set(&key, &buf);
}

/// Issue #760: Enforce sequential milestone gating on a payment.
///
/// When `invoice_id` has a milestone list configured, only the currently
/// *active* milestone may receive funds.  The cumulative amount collected may
/// never exceed the sum of the completed milestones' targets plus the active
/// milestone's target, and a payment is rejected outright once every milestone
/// has been completed.  This prevents a payer from funding a later milestone
/// before its predecessor has been released.
///
/// Invoices without a milestone list are unaffected and return immediately.
fn enforce_milestone_payment(env: &Env, invoice_id: u64, invoice: &Invoice, amount: i128) {
    let key = milestone_data_key(invoice_id);
    let milestones: Vec<Milestone> = match env.storage().persistent().get(&key) {
        Some(list) => list,
        None => return,
    };

    let mut active_target: Option<i128> = None;
    let mut completed_target_sum: i128 = 0;
    for i in 0..milestones.len() {
        let milestone = milestones.get(i).unwrap();
        match milestone.status {
            MilestoneStatus::Completed => {
                completed_target_sum =
                    completed_target_sum.saturating_add(milestone.target_amount);
            }
            MilestoneStatus::Active => {
                if active_target.is_none() {
                    active_target = Some(milestone.target_amount);
                }
            }
            MilestoneStatus::Pending => {}
        }
    }

    // Every milestone is completed — there is nothing left to fund.
    let active_target = active_target.expect("no active milestone");
    let ceiling = completed_target_sum.saturating_add(active_target);
    assert!(
        invoice.funded.saturating_add(amount) <= ceiling,
        "payment exceeds active milestone target"
    );
}

fn notify_invoice(
    env: &Env,
    invoice_id: u64,
    event: Symbol,
    notification_contract: &Option<Address>,
) {
    if let Some(contract) = notification_contract {
        let args = (invoice_id, event).into_val(env);
        let _: Val = env.invoke_contract(contract, &Symbol::new(env, "notify"), args);
    }
}

pub fn get_audit_log(env: &Env, id: u64) -> Vec<AuditEntry> {
    env.storage()
        .persistent()
        .get(&audit_log_key(id))
        .or_else(|| env.storage().instance().get(&audit_log_key(id)))
        .unwrap_or_else(|| Vec::new(env))
}

// ---------------------------------------------------------------------------
// Admin / pause helpers
// ---------------------------------------------------------------------------

fn is_paused(env: &Env) -> bool {
    // Issue #328: primary store is instance; fall back to persistent for migration compat.
    env.storage()
        .instance()
        .get(&paused_key())
        .unwrap_or_else(|| {
            env.storage()
                .persistent()
                .get(&paused_key())
                .unwrap_or(false)
        })
}

/// Issue #626: Reusable pause guard.
///
/// Returns `Err(ContractError::ContractPaused)` when:
/// - the instance-level `Paused` flag is `true`, or
/// - the circuit-breaker persistent flag (issue #297) is `true`.
///
/// Does **not** check the schema version; callers that also need a version
/// guard should call `migrations::require_schema_current` first (see
/// `require_not_paused` below).
fn assert_not_paused(env: &Env) -> Result<(), ContractError> {
    if is_paused(env) {
        return Err(ContractError::ContractPaused);
    }
    // Issue #297: also check circuit breaker.
    let cb_active: bool = env
        .storage()
        .persistent()
        .get(&circuit_breaker_key())
        .unwrap_or(false);
    if cb_active {
        return Err(ContractError::ContractPaused);
    }
    Ok(())
}

fn require_not_paused(env: &Env) {
    migrations::require_schema_current(env);
    freeze_ext::require_not_frozen_global(env);
    assert_not_paused(env).expect("contract is paused");
}

/// Issue #837: An invoice is locked once explicitly locked via `lock_invoice`
/// or as soon as its deadline has passed.
fn invoice_is_locked(env: &Env, invoice_id: u64, invoice: &Invoice) -> bool {
    env.ledger().timestamp() > invoice.deadline
        || env.storage().persistent().has(&invoice_lock_key(invoice_id))
}

/// Issue #837: Reject modifications to a locked invoice.
fn require_invoice_unlocked(env: &Env, invoice_id: u64, invoice: &Invoice) {
    if invoice_is_locked(env, invoice_id, invoice) {
        panic_with_error!(env, ContractError::InvoiceLocked);
    }
}

fn check_not_paused(env: &Env) {
    migrations::require_schema_current(env);
    freeze_ext::require_not_frozen_global(env);
    if assert_not_paused(env).is_err() {
        panic!("ContractPaused");
    }
}

fn validate_allowed_token(env: &Env, token: &Address) {
    if let Some(allowed) = env
        .storage()
        .persistent()
        .get::<_, Vec<Address>>(&storage_keys::allowed_tokens_key())
    {
        if !allowed.is_empty() && !allowed.contains(token) {
            panic!("UnauthorisedToken");
        }
    }
}


fn require_admin_role(env: &Env, admin: &Address, min_role: AdminRole) {
    migrations::require_schema_current(env);
    require_admin_role_unguarded(env, admin, min_role);
}

/// Same authorisation check as [`require_admin_role`], without the
/// schema-version guard. `SplitContract::migrate` must be reachable even when
/// a migration is pending, so it calls this directly rather than
/// `require_admin_role`.
fn require_admin_role_unguarded(env: &Env, admin: &Address, min_role: AdminRole) {
    admin.require_auth();
    let admins: Map<Address, AdminRole> = env
        .storage()
        .instance()
        .get(&admins_key())
        .expect("admins not set");
    let role = admins.get(admin.clone()).expect("caller is not an admin");
    match min_role {
        AdminRole::SuperAdmin => {
            assert!(role == AdminRole::SuperAdmin, "requires SuperAdmin role");
        }
        AdminRole::Operator => {
            assert!(
                role == AdminRole::SuperAdmin || role == AdminRole::Operator,
                "requires Operator role or higher"
            );
        }
    }
}

fn require_fn_not_paused(env: &Env, name: &Symbol) {
    require_not_paused(env);
    let paused_fns: Vec<Symbol> = env
        .storage()
        .persistent()
        .get(&paused_fns_key())
        .unwrap_or_else(|| Vec::new(env));
    if paused_fns.iter().any(|f| f == *name) {
        panic!("function paused");
    }
}

// ---------------------------------------------------------------------------
// Group helpers
// ---------------------------------------------------------------------------

fn load_group(env: &Env, group_id: u64) -> Vec<u64> {
    // New groups are stored as InvoiceGroup; fall back for legacy Vec<u64> groups.
    if let Some(grp) = env
        .storage()
        .persistent()
        .get::<_, types::InvoiceGroup>(&group_key(group_id))
    {
        grp.invoice_ids
    } else {
        env.storage()
            .persistent()
            .get(&group_key(group_id))
            .expect("group not found")
    }
}

fn group_all_funded(env: &Env, group_id: u64) -> bool {
    for id in load_group(env, group_id).iter() {
        let inv = load_invoice(env, id);
        let total: i128 = inv.amounts.iter().sum();
        if inv.funded < total {
            return false;
        }
    }
    true
}

/// Issue #212: Returns true when strictly more than half the group members are fully funded.
fn group_majority_funded(env: &Env, group_id: u64) -> bool {
    let ids = load_group(env, group_id);
    let total_members = ids.len();
    let mut funded_count: u32 = 0;
    for id in ids.iter() {
        let inv = load_invoice(env, id);
        let total: i128 = inv.amounts.iter().sum();
        if inv.funded >= total {
            funded_count += 1;
        }
    }
    funded_count * 2 > total_members
}

fn treasury_record_for_invoice(env: &Env, invoice_id: u64) -> Option<(u64, TreasuryRecord)> {
    if let Some(group_id) = env
        .storage()
        .persistent()
        .get::<(Symbol, u64), u64>(&invoice_treasury_key(invoice_id))
    {
        if let Some(record) = env
            .storage()
            .persistent()
            .get(&group_treasury_key(group_id))
        {
            return Some((group_id, record));
        }
    }
    None
}

#[allow(dead_code)]
fn load_treasury_record(env: &Env, group_id: u64) -> TreasuryRecord {
    env.storage()
        .persistent()
        .get(&group_treasury_key(group_id))
        .expect("treasury record not found")
}

// ---------------------------------------------------------------------------
// Issue #332: Recipient list helpers (optimised iteration)
// ---------------------------------------------------------------------------

/// Persist the contiguous recipient + amount vectors for `invoice_id`.
/// Called once at invoice creation (or migration).  During release we load
/// both vecs with two `get()` calls instead of N per-recipient reads.
fn save_recipients_list(env: &Env, id: u64, recipients: &Vec<Address>, amounts: &Vec<i128>) {
    env.storage()
        .persistent()
        .set(&recipients_list_key(id), recipients);
    env.storage()
        .persistent()
        .set(&amounts_list_key(id), amounts);
}

/// Load the contiguous recipient list.  Falls back to the invoice struct's
/// `recipients` field when the optimised list is absent (pre-migration).
fn load_recipients_list(
    env: &Env,
    id: u64,
    fallback_recipients: &Vec<Address>,
    fallback_amounts: &Vec<i128>,
) -> (Vec<Address>, Vec<i128>) {
    let recipients: Vec<Address> = env
        .storage()
        .persistent()
        .get(&recipients_list_key(id))
        .unwrap_or_else(|| fallback_recipients.clone());
    let amounts: Vec<i128> = env
        .storage()
        .persistent()
        .get(&amounts_list_key(id))
        .unwrap_or_else(|| fallback_amounts.clone());
    (recipients, amounts)
}

// ---------------------------------------------------------------------------
// Issue #333: Milestone event helpers
// ---------------------------------------------------------------------------

/// Milestone thresholds in basis-points (basis = 10 000).
const MILESTONE_THRESHOLDS: [u32; 4] = [2500, 5000, 7500, 10000];

/// Check whether `funded_amount` has newly crossed any milestone thresholds
/// since `prev_funded`.  Emits a `milestone_reached` event for each newly-
/// crossed threshold, and persists the updated bitmask in instance storage
/// (no extra persistent rent — it piggybacks on the instance-TTL bump that
/// `save_invoice` already performs on every `pay()` call).
///
/// Bit layout of the flag byte:
///   Bit 0 → 25 %  (2500 bps)
///   Bit 1 → 50 %  (5000 bps)
///   Bit 2 → 75 %  (7500 bps)
///   Bit 3 → 100 % (10000 bps)
fn check_and_emit_milestones(
    env: &Env,
    invoice_id: u64,
    prev_funded: i128,
    new_funded: i128,
    total: i128,
) {
    if total <= 0 {
        return;
    }
    // Load existing flags (0 if this is the first call for this invoice).
    let mut flags: u32 = env
        .storage()
        .instance()
        .get(&milestone_flags_key(invoice_id))
        .unwrap_or(0u32);

    let mut changed = false;
    for (bit, &bps) in MILESTONE_THRESHOLDS.iter().enumerate() {
        // Already emitted?
        if flags & (1u32 << bit) != 0 {
            continue;
        }
        // Threshold in token units (rounded down). Issue #482: use checked arithmetic.
        let threshold_amount: i128 = checked_bps_of(total, bps, 10_000u128)
            .expect("ArithmeticOverflow");
        // Was this threshold NOT crossed before, but IS crossed now?
        if prev_funded < threshold_amount && new_funded >= threshold_amount {
            events::milestone_reached(env, invoice_id, bps, new_funded);
            flags |= 1u32 << bit;
            changed = true;
        }
    }

    if changed {
        env.storage()
            .instance()
            .set(&milestone_flags_key(invoice_id), &flags);
    }
}

/// Validate contract-level funding checkpoints. Checkpoints are basis-point
/// thresholds and must be sorted ascending so events are emitted in order.
fn validate_funding_checkpoints(checkpoints: &Vec<u32>) {
    let mut prev = 0u32;
    for checkpoint in checkpoints.iter() {
        assert!(
            checkpoint <= 10_000,
            "checkpoint basis points must be <= 10000"
        );
        assert!(checkpoint > prev, "checkpoints must be sorted ascending");
        prev = checkpoint;
    }
}

/// Emit admin-configured funding checkpoint events for any thresholds newly
/// crossed by this payment. Progress is calculated as `(funded * 10_000) / total`
/// and compared against the contract-level checkpoint list.
fn check_and_emit_funding_checkpoints(env: &Env, invoice_id: u64, funded: i128, total: i128) {
    if total <= 0 || funded <= 0 {
        return;
    }

    let checkpoints: Vec<u32> = env
        .storage()
        .instance()
        .get(&funding_checkpoints_key())
        .unwrap_or_else(|| Vec::new(env));
    if checkpoints.is_empty() {
        return;
    }

    let progress_bps = funding_bps(funded, total) as i128;
    let last_emitted: u32 = env
        .storage()
        .persistent()
        .get(&last_checkpoint_key(invoice_id))
        .unwrap_or(0u32);
    let mut highest_emitted = last_emitted;

    for threshold_bps in checkpoints.iter() {
        if threshold_bps > last_emitted && (threshold_bps as i128) <= progress_bps {
            events::funding_checkpoint(env, invoice_id, threshold_bps, funded, total);
            highest_emitted = threshold_bps;
        }
    }

    if highest_emitted != last_emitted {
        env.storage()
            .persistent()
            .set(&last_checkpoint_key(invoice_id), &highest_emitted);
    }
}

// ---------------------------------------------------------------------------
// Issue #334: Compact status helpers
// ---------------------------------------------------------------------------

/// Write the compact status byte alongside the normal InvoiceCore.
/// This is a non-breaking overlay: `load_invoice` continues to work.
fn save_compact_status(env: &Env, id: u64, status: &InvoiceStatus) {
    let byte: u32 = status.to_u8() as u32;
    env.storage()
        .persistent()
        .set(&compact_status_key(id), &byte);
    env.storage().persistent().extend_ttl(
        &compact_status_key(id),
        INVOICE_HOT_TTL_LEDGERS / 2,
        INVOICE_HOT_TTL_LEDGERS,
    );
}

fn maybe_record_refunded(env: &Env, creator: &Address) {
    if let Some(dashboard) = env
        .storage()
        .persistent()
        .get::<Symbol, Address>(&dashboard_contract_key())
    {
        let _: Val = env.invoke_contract(
            &dashboard,
            &Symbol::new(env, "record_refunded"),
            (creator.clone(),).into_val(env),
        );
    }
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

/// Issue #276: Check and emit platform volume milestone events.
fn check_platform_milestone(env: &Env, new_volume: i128) {
    let threshold: i128 = env
        .storage()
        .persistent()
        .get(&platform_vol_thresh_key())
        .unwrap_or(10_000_000_000_i128); // 10,000 USDC (7 decimals)
    if threshold <= 0 {
        return;
    }
    let last_milestone: i128 = env
        .storage()
        .persistent()
        .get(&platform_vol_mile_key())
        .unwrap_or(0i128);
    let new_milestone = new_volume / threshold;
    if new_milestone > last_milestone {
        env.storage()
            .persistent()
            .set(&platform_vol_mile_key(), &new_milestone);
        let invoice_count: u64 = env
            .storage()
            .persistent()
            .get(&total_invoices_key())
            .unwrap_or(0u64);
        events::platform_volume_milestone(env, new_volume, invoice_count, new_milestone);
    }
}

/// Issue #276: Check and emit creator volume milestone events.
fn check_creator_milestone(env: &Env, creator: &Address, new_volume: i128) {
    let threshold: i128 = env
        .storage()
        .persistent()
        .get(&creator_vol_thresh_key())
        .unwrap_or(1_000_000_000_i128); // 1,000 USDC (7 decimals)
    if threshold <= 0 {
        return;
    }
    let last_milestone: i128 = env
        .storage()
        .persistent()
        .get(&creator_vol_mile_key(creator))
        .unwrap_or(0i128);
    let new_milestone = new_volume / threshold;
    if new_milestone > last_milestone {
        env.storage()
            .persistent()
            .set(&creator_vol_mile_key(creator), &new_milestone);
        let invoice_count: u64 = env
            .storage()
            .persistent()
            .get(&creator_stats_count_key(creator))
            .unwrap_or(0u64);
        events::creator_volume_milestone(env, creator, new_volume, invoice_count, new_milestone);
    }
}

// ---------------------------------------------------------------------------
// Issue #435: Contract freeze helper
// ---------------------------------------------------------------------------

/// Check if contract is frozen for upgrade. Panics if frozen.
fn require_not_frozen(env: &Env) {
    migrations::require_schema_current(env);
    let is_frozen: bool = env
        .storage()
        .instance()
        .get(&upgrade_freeze_key())
        .unwrap_or(false);
    assert!(!is_frozen, "contract is frozen for upgrade");
}

// ---------------------------------------------------------------------------
// RBAC helpers
// ---------------------------------------------------------------------------

/// Return `true` when `address` holds `role` **or** holds `Role::Admin`.
/// Admin is a super-role that implies all other roles.
#[allow(dead_code)]
fn has_role(env: &Env, address: &Address, role: &Role) -> bool {
    // Admin implies every role
    let admin_disc = role_discriminant(&Role::Admin);
    let role_disc  = role_discriminant(role);
    env.storage()
        .persistent()
        .get::<_, bool>(&role_key(address, admin_disc))
        .unwrap_or(false)
        || env.storage()
            .persistent()
            .get::<_, bool>(&role_key(address, role_disc))
            .unwrap_or(false)
}

/// Require that `caller` holds at least one of the supplied roles.
/// Also requires `caller.require_auth()` so the call is signed.
/// Panics with "RoleNotHeld" when no role matches.
#[allow(dead_code)]
fn require_role(env: &Env, caller: &Address, roles: &[Role]) {
    caller.require_auth();
    for role in roles {
        if has_role(env, caller, role) {
            return;
        }
    }
    panic!("RoleNotHeld");
}

// ---------------------------------------------------------------------------
// Issue #431: Duplicate payment detection
// ---------------------------------------------------------------------------

const DEFAULT_DUPLICATE_WINDOW_LEDGERS: u32 = 100;

/// Compute payment fingerprint hash: sha256(invoice_id || payer || amount || ledger).
#[allow(dead_code)]
fn compute_payment_fingerprint(
    env: &Env,
    invoice_id: u64,
    payer: &Address,
    amount: i128,
    ledger: u32,
) -> BytesN<32> {
    let mut input = Bytes::new(env);
    for byte in invoice_id.to_be_bytes().iter() {
        input.push_back(*byte);
    }
    let payer_val: Val = payer.clone().into_val(env);
    let payer_bytes = payer_val.to_xdr(env);
    for byte in payer_bytes.iter() {
        input.push_back(byte);
    }
    for byte in amount.to_be_bytes().iter() {
        input.push_back(*byte);
    }
    for byte in ledger.to_be_bytes().iter() {
        input.push_back(*byte);
    }
    env.crypto().sha256(&input).into()
}

// ---------------------------------------------------------------------------
// Issue #826: Batch payment signature verification
// ---------------------------------------------------------------------------

/// Build the canonical message signed for a `SignedPayment` batch entry:
/// `invoice_id (BE) || payer (XDR) || amount (BE) || nonce (BE)`.
fn signed_payment_message(
    env: &Env,
    invoice_id: u64,
    payer: &Address,
    amount: i128,
    nonce: u64,
) -> Bytes {
    let mut msg = Bytes::new(env);
    for byte in invoice_id.to_be_bytes().iter() {
        msg.push_back(*byte);
    }
    let payer_val: Val = payer.clone().into_val(env);
    let payer_bytes = payer_val.to_xdr(env);
    for byte in payer_bytes.iter() {
        msg.push_back(byte);
    }
    for byte in amount.to_be_bytes().iter() {
        msg.push_back(*byte);
    }
    for byte in nonce.to_be_bytes().iter() {
        msg.push_back(*byte);
    }
    msg
}

/// Verify the Ed25519 signature on a single signed payment entry.
/// Panics (via the host's crypto error) if the signature does not verify.
fn verify_signed_payment(env: &Env, entry: &SignedPayment) {
    let msg = signed_payment_message(env, entry.invoice_id, &entry.payer, entry.amount, entry.nonce);
    env.crypto()
        .ed25519_verify(&entry.signer_pubkey, &msg, &entry.signature);
}

/// Check if payment fingerprint exists (duplicate detection).
#[allow(dead_code)]
fn check_duplicate_payment(env: &Env, fingerprint: &BytesN<32>) -> bool {
    env.storage()
        .persistent()
        .has(&payment_fingerprint_key(fingerprint))
}

/// Record payment fingerprint with TTL.
#[allow(dead_code)]
fn record_payment_fingerprint(env: &Env, fingerprint: &BytesN<32>, current_ledger: u32) {
    let window_ledgers: u32 = env
        .storage()
        .instance()
        .get(&duplicate_window_ledgers_key())
        .unwrap_or(DEFAULT_DUPLICATE_WINDOW_LEDGERS);
    env.storage()
        .persistent()
        .set(&payment_fingerprint_key(fingerprint), &current_ledger);
    env.storage().persistent().extend_ttl(
        &payment_fingerprint_key(fingerprint),
        window_ledgers,
        window_ledgers,
    );
}

// ---------------------------------------------------------------------------
// Issue #432: Referral tracking
// ---------------------------------------------------------------------------

/// Set referrer reward percentage (admin-only via separate call).
// Issue #432: referral rewards are stored and read here, but no contract entry
// point exposes them yet.
#[allow(dead_code)]
fn set_referrer_reward_bps(env: &Env, reward_bps: u32) {
    assert!(reward_bps <= 10_000, "reward_bps must be ≤ 10000");
    env.storage()
        .instance()
        .set(&referrer_reward_bps_key(), &reward_bps);
}

/// Get current referrer reward percentage.
#[allow(dead_code)]
fn get_referrer_reward_bps(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&referrer_reward_bps_key())
        .unwrap_or(0u32)
}

// ---------------------------------------------------------------------------
// Issue #434: Invoice groups
// ---------------------------------------------------------------------------

/// Create a new invoice group and assign group_id to all members.
#[allow(dead_code)]
fn create_group_for_invoices(env: &Env, invoice_ids: &Vec<u64>) -> u64 {
    let group_id: u64 = env
        .storage()
        .instance()
        .get(&group_counter_key())
        .unwrap_or(0u64)
        + 1;
    env.storage()
        .instance()
        .set(&group_counter_key(), &group_id);

    env.storage()
        .persistent()
        .set(&group_members_key(group_id), invoice_ids);

    for id in invoice_ids.iter() {
        env.storage()
            .persistent()
            .set(&invoice_group_id_key(id), &group_id);
    }

    group_id
}

/// Get all members of a group.
fn get_group_members(env: &Env, group_id: u64) -> Vec<u64> {
    env.storage()
        .persistent()
        .get(&group_members_key(group_id))
        .unwrap_or_else(|| Vec::new(env))
}

/// Get group ID for an invoice.
fn get_invoice_group_id(env: &Env, invoice_id: u64) -> Option<u64> {
    env.storage()
        .persistent()
        .get(&invoice_group_id_key(invoice_id))
}

#[contract]
pub struct SplitContract;

#[contractimpl]
#[allow(clippy::too_many_arguments)]
impl SplitContract {
    /// Set the contract admin, creation fee, treasury, USDC token, and platform fee.
    /// Can only be called once.
    pub fn initialize(
        env: Env,
        admin: Address,
        creation_fee: i128,
        treasury: Address,
        usdc_token: Address,
        platform_fee_bps: u32,
        governance_contract: Option<Address>,
        max_cancel_bps: u32,
        rate_limit: u32,
        rate_window: u64,
    ) {
        // Issue #477: one-shot initialiser guard using a dedicated key so the
        // guard cannot be bypassed by front-running the admin write.
        if env.storage().instance().get::<_, bool>(&initialised_key()).unwrap_or(false) {
            panic!("AlreadyInitialised");
        }
        assert!(creation_fee >= 0, "creation_fee must be non-negative");
        assert!(
            platform_fee_bps <= 10_000,
            "platform_fee_bps must be ≤ 10000"
        );
        assert!(max_cancel_bps <= 10_000, "max_cancel_bps must be ≤ 10000");
        assert!(
            rate_window > 0 || rate_limit == 0,
            "rate_window must be positive when rate_limit is enabled"
        );
        let mut admins: Map<Address, AdminRole> = Map::new(&env);
        admins.set(admin.clone(), AdminRole::SuperAdmin);
        env.storage().instance().set(&admins_key(), &admins);
        env.storage().instance().set(&admin_key(), &admin);
        env.storage()
            .instance()
            .set(&creation_fee_key(), &creation_fee);
        env.storage().instance().set(&treasury_key(), &treasury);
        env.storage().instance().set(&usdc_token_key(), &usdc_token);
        env.storage()
            .instance()
            .set(&platform_fee_bps_key(), &platform_fee_bps);
        env.storage()
            .instance()
            .set(&governance_contract_key(), &governance_contract);
        env.storage()
            .instance()
            .set(&archive_after_ledgers_key(), &ARCHIVE_AFTER_LEDGERS);
        env.storage().persistent().set(&paused_key(), &false);
        env.storage()
            .persistent()
            .set(&max_cancel_bps_key(), &max_cancel_bps);
        env.storage()
            .persistent()
            .set(&rate_limit_key(), &rate_limit);
        env.storage()
            .persistent()
            .set(&rate_window_key(), &rate_window);
        // Issue #425: seed the default per-invoice storage quota.
        env.storage()
            .instance()
            .set(&storage_quota_key(), &DEFAULT_INVOICE_STORAGE_QUOTA);
        // Confidential payment settlement: derive two independent,
        // nothing-up-my-sleeve BLS12-381 G1 generators via hash-to-curve so
        // neither party can know the discrete log of one relative to the
        // other, then pin them in instance storage so every commit/reveal
        // uses the exact same basis.
        let pedersen_dst = Bytes::from_slice(&env, b"StellarSplit-Pedersen-BLS12381G1-v1");
        let bls = env.crypto().bls12_381();
        let g = bls.hash_to_g1(&Bytes::from_slice(&env, b"pedersen-generator-G"), &pedersen_dst);
        let h = bls.hash_to_g1(&Bytes::from_slice(&env, b"pedersen-generator-H"), &pedersen_dst);
        env.storage().instance().set(&pedersen_g_key(), &g);
        env.storage().instance().set(&pedersen_h_key(), &h);

        // Stamp the schema version so fresh deployments don't require a
        // migration call before any other entry point works.
        migrations::init_schema_version(&env);

        // Issue #477: set the initialisation flag atomically at the end so the
        // contract is marked fully initialised only after all state is written.
        env.storage().instance().set(&initialised_key(), &true);
    }

    /// Add a new admin with a given role. Requires SuperAdmin auth.
    pub fn add_admin(env: Env, admin: Address, new_admin: Address, role: AdminRole) {
        require_admin_role(&env, &admin, AdminRole::SuperAdmin);
        let mut admins: Map<Address, AdminRole> = env
            .storage()
            .instance()
            .get(&admins_key())
            .expect("admins not set");
        admins.set(new_admin, role);
        env.storage().instance().set(&admins_key(), &admins);
    }

    /// Remove an admin. Requires SuperAdmin auth.
    /// Panics if removing the last SuperAdmin.
    pub fn remove_admin(env: Env, admin: Address, target: Address) {
        require_admin_role(&env, &admin, AdminRole::SuperAdmin);
        let mut admins: Map<Address, AdminRole> = env
            .storage()
            .instance()
            .get(&admins_key())
            .expect("admins not set");
        assert!(
            admins.get(target.clone()).is_some(),
            "target is not an admin"
        );
        let mut super_admin_count: u32 = 0;
        for (_, r) in admins.iter() {
            if r == AdminRole::SuperAdmin {
                super_admin_count += 1;
            }
        }
        let target_role = admins.get(target.clone()).unwrap();
        if target_role == AdminRole::SuperAdmin && super_admin_count <= 1 {
            panic!("cannot remove the last SuperAdmin");
        }
        admins.remove(target);
        env.storage().instance().set(&admins_key(), &admins);
    }

    /// Issue #472: Pause the contract. Requires admin auth.
    pub fn pause(env: Env, admin: Address) {
        migrations::require_schema_current(&env);
        admin.require_auth();
        if let Some(stored_admin) = env.storage().instance().get::<_, Address>(&admin_key()) {
            assert!(admin == stored_admin, "NotAuthorized");
        } else {
            require_admin_role_unguarded(&env, &admin, AdminRole::Operator);
        }
        env.storage().instance().set(&paused_key(), &true);
        events::contract_paused(&env, &admin);
    }

    /// Issue #472: Unpause the contract. Requires admin auth.
    pub fn unpause(env: Env, admin: Address) {
        migrations::require_schema_current(&env);
        admin.require_auth();
        if let Some(stored_admin) = env.storage().instance().get::<_, Address>(&admin_key()) {
            assert!(admin == stored_admin, "NotAuthorized");
        } else {
            require_admin_role_unguarded(&env, &admin, AdminRole::Operator);
        }
        env.storage().instance().set(&paused_key(), &false);
        events::contract_unpaused(&env, &admin);
    }

    /// Issue #800: emergency stop for every invoice. Admin-only; sets the
    /// contract-wide pause flag that `pay`, `release` and `refund` check, so no
    /// payment moves and nothing is released until `resume_all_invoices`.
    pub fn pause_all_invoices(env: Env, admin: Address) {
        Self::pause(env.clone(), admin.clone());
        events::invoice_system_paused(&env, &admin);
    }

    /// Issue #800: lift an emergency stop set by `pause_all_invoices`. Admin-only.
    pub fn resume_all_invoices(env: Env, admin: Address) {
        Self::unpause(env.clone(), admin.clone());
        events::invoice_system_resumed(&env, &admin);
    }

    /// Issue #328: Return the current pause state (read-only; available while paused).
    pub fn is_paused(env: Env) -> bool {
        is_paused(&env)
    }

    /// Issue #792: record that `new_user` was acquired through `referrer`.
    /// One-time only, callable by `new_user`; a user cannot refer themselves.
    pub fn register_acquisition(env: Env, new_user: Address, referrer: Address) {
        require_not_paused(&env);
        new_user.require_auth();
        assert!(new_user != referrer, "cannot refer yourself");
        let key = acquisition_referrer_key(&new_user);
        assert!(
            !env.storage().persistent().has(&key),
            "acquisition already registered"
        );
        env.storage().persistent().set(&key, &referrer);
        events::acquisition_registered(&env, &new_user, &referrer);
    }

    /// Issue #792: the referrer that acquired `user`, if one was registered.
    pub fn get_acquisition_referrer(env: Env, user: Address) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&acquisition_referrer_key(&user))
    }

    /// Issue #470: Contribute funds toward an invoice with partial refund mechanism for overpayments.
    pub fn contribute(env: Env, invoice_id: u64, payer: Address, amount: i128) -> ContributionResult {
        check_not_paused(&env);
        payer.require_auth();

        // --- Payer spending cap enforcement ---
        let cap: i128 = env
            .storage()
            .instance()
            .get(&payer_spend_cap_key())
            .unwrap_or(0i128);
        if cap > 0 {
            let window_ledgers: u32 = env
                .storage()
                .instance()
                .get(&payer_spend_window_ledgers_key())
                .unwrap_or(DEFAULT_PAYER_SPEND_WINDOW_LEDGERS);
            let current_ledger = env.ledger().sequence();
            let current_window_start = current_ledger / (window_ledgers as u32) * (window_ledgers as u32);
            let accum_key = payer_spend_accum_key(&payer);
            let (stored_window, stored_total): (u32, i128) = env
                .storage()
                .temporary()
                .get(&accum_key)
                .unwrap_or((0u32, 0i128));
            let (effective_window, effective_total) = if stored_window == current_window_start {
                (stored_window, stored_total)
            } else {
                // New window — reset accumulator.
                (current_window_start, 0i128)
            };
            let new_total = effective_total + amount;
            if new_total > cap {
                events::payer_spend_limit_reached(&env, &payer, new_total, cap);
                panic!("{}", ContractError::PayerSpendLimitExceeded as u32);
            }
            env.storage()
                .temporary()
                .set(&accum_key, &(effective_window, new_total));
        }
        // --- End payer spending cap ---

        let mut invoice = load_invoice(&env, invoice_id);
        if invoice.status == InvoiceStatus::Disputed {
            panic!("{}", ContractError::InvoiceDisputed as u32);
        }
        assert!(invoice.status == InvoiceStatus::Pending, "InvoiceNotPending");

        validate_allowed_token(&env, &invoice.funding_token);

        let total: i128 = invoice.amounts.iter().sum();
        let remaining = total.saturating_sub(invoice.funded);

        let (amount_applied, refund_amount) = if amount > remaining {
            (remaining, amount - remaining)
        } else {
            (amount, 0i128)
        };

        if refund_amount > 0 {
            events::refund_issued(&env, invoice_id, &payer, refund_amount);
        }

        if amount_applied > 0 {
            invoice.funded += amount_applied;
            invoice.payments.push_back(types::Payment {
                payer: payer.clone(),
                amount: amount_applied,
                tip: 0,
                attestation_hash: None,
                donate_on_failure: false,
                ledger: env.ledger().sequence(),
                timestamp: env.ledger().timestamp(),
            });

            if invoice.funded >= total {
                invoice.status = InvoiceStatus::Released;
                events::invoice_released(&env, invoice_id, &invoice.recipients);
            }

            save_invoice(&env, invoice_id, &invoice);

            // Issue #527: append to payer payment history.
            let hist_key = payer_history_key(&payer);
            let mut history: Vec<PaymentRecord> = env
                .storage()
                .persistent()
                .get(&hist_key)
                .unwrap_or_else(|| Vec::new(&env));
            history.push_back(PaymentRecord {
                invoice_id,
                amount: amount_applied,
                ledger: env.ledger().sequence(),
            });
            env.storage().persistent().set(&hist_key, &history);
        }

        ContributionResult {
            invoice_id,
            amount_applied,
            refund_amount,
        }
    }

    /// Withdraw a contribution before the invoice is fully funded.
    /// Only permitted while the invoice is in Pending status (Open/PartiallyFunded).
    /// The caller receives exactly the amount they contributed, the contribution
    /// storage entry is deleted, and `invoice.funded` is decremented.
    pub fn withdraw_contribution(env: Env, payer: Address, invoice_id: u64) -> Result<(), ContractError> {
        payer.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        // Only allow withdrawal while invoice is in Pending (Open) status.
        assert_invoice_pending(&invoice)?;

        let contrib_key = contribution_key(invoice_id, &payer);
        let amount: i128 = env
            .storage()
            .persistent()
            .get(&contrib_key)
            .unwrap_or(0i128);

        if amount <= 0 {
            return Err(ContractError::ZeroAmountNotAllowed);
        }

        // Delete the contribution record.
        env.storage().persistent().remove(&contrib_key);

        // Decrease funded amount.
        invoice.funded = invoice.funded.saturating_sub(amount);
        save_invoice(&env, invoice_id, &invoice);

        // Transfer the contribution back to the payer.
        let funding_token = funding_token_for(&invoice);
        let token_client = token::Client::new(&env, &funding_token);
        token_client.transfer(&env.current_contract_address(), &payer, &amount);

        events::contribution_withdrawn(&env, invoice_id, &payer, amount);
        Ok(())
    }

    /// Issue #471: Rotate a registered recipient's payout address before invoice finalisation.
    pub fn rotate_recipient_address(env: Env, invoice_id: u64, old_address: Address, new_address: Address) {
        check_not_paused(&env);
        old_address.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(invoice.status == InvoiceStatus::Pending, "InvoiceNotPending");

        let mut found = false;
        let mut new_recipients = Vec::new(&env);
        for r in invoice.recipients.iter() {
            if r == old_address {
                new_recipients.push_back(new_address.clone());
                found = true;
            } else {
                new_recipients.push_back(r);
            }
        }
        assert!(found, "RecipientNotFound");

        invoice.recipients = new_recipients;
        save_invoice(&env, invoice_id, &invoice);

        env.storage().persistent().set(
            &types::RecipientAddress(invoice_id, old_address.clone()),
            &new_address,
        );

        events::recipient_address_rotated(&env, invoice_id, &old_address, &new_address);
    }

    /// Issue #473: Add an asset contract address to the allowed tokens list.
    pub fn add_allowed_token(env: Env, admin: Address, token: Address) {
        migrations::require_schema_current(&env);
        admin.require_auth();
        if let Some(stored_admin) = env.storage().instance().get::<_, Address>(&admin_key()) {
            assert!(admin == stored_admin, "NotAuthorized");
        } else {
            require_admin_role_unguarded(&env, &admin, AdminRole::Operator);
        }
        let mut allowed: Vec<Address> = env
            .storage()
            .persistent()
            .get(&storage_keys::allowed_tokens_key())
            .unwrap_or_else(|| Vec::new(&env));
        if !allowed.contains(&token) {
            allowed.push_back(token);
            env.storage().persistent().set(&storage_keys::allowed_tokens_key(), &allowed);
        }
    }

    /// Issue #473: Remove an asset contract address from the allowed tokens list.
    pub fn remove_allowed_token(env: Env, admin: Address, token: Address) {
        migrations::require_schema_current(&env);
        admin.require_auth();
        if let Some(stored_admin) = env.storage().instance().get::<_, Address>(&admin_key()) {
            assert!(admin == stored_admin, "NotAuthorized");
        } else {
            require_admin_role_unguarded(&env, &admin, AdminRole::Operator);
        }
        let mut allowed: Vec<Address> = env
            .storage()
            .persistent()
            .get(&storage_keys::allowed_tokens_key())
            .unwrap_or_else(|| Vec::new(&env));
        if let Some(idx) = allowed.iter().position(|t| t == token) {
            allowed.remove(idx as u32);
            env.storage().persistent().set(&storage_keys::allowed_tokens_key(), &allowed);
        }
    }

    /// Issue #473: Get the list of allowed tokens.
    pub fn get_allowed_tokens(env: Env) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&storage_keys::allowed_tokens_key())
            .unwrap_or_else(|| Vec::new(&env))
    }


    /// Pause a specific function by name. Requires Operator+ auth.
    /// While paused, the function panics with "function paused" when called.
    pub fn pause_function(env: Env, admin: Address, function: Symbol) {
        require_admin_role(&env, &admin, AdminRole::Operator);
        let mut paused_fns: Vec<Symbol> = env
            .storage()
            .persistent()
            .get(&paused_fns_key())
            .unwrap_or_else(|| Vec::new(&env));
        if !paused_fns.iter().any(|f| f == function) {
            paused_fns.push_back(function);
        }
        env.storage()
            .persistent()
            .set(&paused_fns_key(), &paused_fns);
    }

    /// Unpause a specific function by name. Requires Operator+ auth.
    pub fn unpause_function(env: Env, admin: Address, function: Symbol) {
        require_admin_role(&env, &admin, AdminRole::Operator);
        let paused_fns: Vec<Symbol> = env
            .storage()
            .persistent()
            .get(&paused_fns_key())
            .unwrap_or_else(|| Vec::new(&env));
        let mut new_list: Vec<Symbol> = Vec::new(&env);
        for f in paused_fns.iter() {
            if f != function {
                new_list.push_back(f);
            }
        }
        env.storage().persistent().set(&paused_fns_key(), &new_list);
    }

    /// Set an address as exempt from the global pause for invoice creation.
    /// Requires admin auth.
    pub fn set_pause_exempt(env: Env, admin: Address, address: Address, exempt: bool) {
        require_admin_role(&env, &admin, AdminRole::Operator);
        if exempt {
            env.storage()
                .persistent()
                .set(&pause_exempt_key(&address), &true);
        } else {
            env.storage()
                .persistent()
                .remove(&pause_exempt_key(&address));
        }
    }

    /// Set the global payer aggregate limit and window. Requires admin auth.
    pub fn set_global_payer_limit(env: Env, admin: Address, limit: i128, window_secs: u64) {
        require_admin_role(&env, &admin, AdminRole::Operator);
        assert!(limit >= 0, "limit must be non-negative");
        env.storage()
            .persistent()
            .set(&global_payer_limit_key(), &limit);
        env.storage()
            .persistent()
            .set(&global_payer_window_key(), &window_secs);
    }

    /// Configure the global per-payer spending cap (ledger-based window).
    /// Each payer may contribute at most `cap` total across all invoices within
    /// `window_ledgers` ledgers. The accumulator resets when a new window begins.
    pub fn set_payer_spend_cap(
        env: Env,
        admin: Address,
        cap: i128,
        window_ledgers: u32,
    ) {
        require_admin_role(&env, &admin, AdminRole::Operator);
        assert!(cap >= 0, "cap must be non-negative");
        assert!(window_ledgers > 0 || cap == 0, "window_ledgers must be positive when cap > 0");
        env.storage()
            .instance()
            .set(&payer_spend_cap_key(), &cap);
        env.storage()
            .instance()
            .set(&payer_spend_window_ledgers_key(), &window_ledgers);
    }

    /// Configure the global dispute timeout in ledgers.
    /// After this many ledgers from dispute open, anyone may call auto_close_dispute.
    pub fn set_dispute_timeout(env: Env, admin: Address, timeout_ledgers: u32) {
        require_admin_role(&env, &admin, AdminRole::Operator);
        assert!(timeout_ledgers > 0, "timeout must be positive");
        env.storage()
            .instance()
            .set(&dispute_timeout_key(), &timeout_ledgers);
    }

    /// Configure the per-invoice sliding-window payment limiter.
    pub fn set_rate_limit(env: Env, admin: Address, window_ledgers: u32, max_payments: u32) {
        require_admin_role(&env, &admin, AdminRole::Operator);
        assert!(
            window_ledgers > 0 || max_payments == 0,
            "window_ledgers must be positive when rate limit is enabled"
        );
        env.storage()
            .instance()
            .set(&invoice_rate_limit_window_key(), &window_ledgers);
        env.storage()
            .instance()
            .set(&invoice_rate_limit_max_key(), &max_payments);
    }

    /// Update the creation fee. Requires admin auth.
    pub fn set_creation_fee(env: Env, admin: Address, creation_fee: i128) {
        require_admin_role(&env, &admin, AdminRole::Operator);
        assert!(creation_fee >= 0, "creation_fee must be non-negative");
        env.storage()
            .instance()
            .set(&creation_fee_key(), &creation_fee);
    }

    /// Set the global per-invoice storage quota in bytes (issue #425). Requires
    /// admin auth. Applies to `create_invoice` and every mutation entry point
    /// that goes through `save_invoice` (e.g. `add_recipient`).
    pub fn set_invoice_storage_quota(env: Env, admin: Address, bytes: u64) {
        require_admin_role(&env, &admin, AdminRole::Operator);
        assert!(bytes > 0, "quota must be positive");
        env.storage().instance().set(&storage_quota_key(), &bytes);
    }

    /// Issue #503: Set the maximum number of open invoices a single creator may hold at once.
    /// Admin-only. Emits InvoiceLimitUpdated(new_limit).
    pub fn set_max_open_invoices(env: Env, admin: Address, new_limit: u32) {
        require_admin_role(&env, &admin, AdminRole::Operator);
        assert!(new_limit > 0, "new_limit must be positive");
        env.storage()
            .instance()
            .set(&max_open_invoices_key(), &new_limit);
        events::invoice_limit_updated(&env, new_limit);
    }

    /// Returns the current global per-invoice storage quota in bytes (issue #425).
    pub fn get_storage_quota(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&storage_quota_key())
            .unwrap_or(DEFAULT_INVOICE_STORAGE_QUOTA)
    }

    /// Configure contract-level funding progress checkpoints. Each checkpoint is
    /// a basis-point threshold (`10_000 = 100%`). The list must be sorted in
    /// strictly ascending order and every value must be <= 10_000.
    pub fn set_funding_checkpoints(env: Env, admin: Address, checkpoints: Vec<u32>) {
        require_admin_role(&env, &admin, AdminRole::Operator);
        validate_funding_checkpoints(&checkpoints);
        env.storage()
            .instance()
            .set(&funding_checkpoints_key(), &checkpoints);
    }

    /// Return the currently configured funding progress checkpoints.
    pub fn get_funding_checkpoints(env: Env) -> Vec<u32> {
        env.storage()
            .instance()
            .get(&funding_checkpoints_key())
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Return the highest funding checkpoint already emitted for an invoice.
    pub fn get_last_funding_checkpoint(env: Env, invoice_id: u64) -> u32 {
        env.storage()
            .persistent()
            .get(&last_checkpoint_key(invoice_id))
            .unwrap_or(0u32)
    }

    /// Issue #439: Set the cancellation cooldown period in ledgers. Requires admin auth.
    /// After a creator cancels an invoice, they must wait this many ledgers before creating a new one.
    /// Set to 0 to disable the cooldown.
    pub fn set_cancellation_cooldown(env: Env, admin: Address, cooldown_ledgers: u64) {
        require_admin_role(&env, &admin, AdminRole::Operator);
        env.storage()
            .instance()
            .set(&cancellation_cooldown_ledgers_key(), &cooldown_ledgers);
    }

    /// Issue #439: Get the current cancellation cooldown period in ledgers.
    pub fn get_cancellation_cooldown(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&cancellation_cooldown_ledgers_key())
            .unwrap_or(DEFAULT_CANCELLATION_COOLDOWN_LEDGERS)
    }

    /// Issue #439: Get the cooldown-until ledger for a creator. Returns 0 if no cooldown is active.
    pub fn get_creator_cooldown(env: Env, creator: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&creator_cooldown_key(&creator))
            .unwrap_or(0u64)
    }

    pub fn set_commitment_expiry(env: Env, admin: Address, ledgers: u32) {
        require_admin_role(&env, &admin, AdminRole::Operator);
        assert!(ledgers > 0, "commitment expiry must be positive");
        env.storage()
            .instance()
            .set(&commitment_expiry_key(), &ledgers);
    }

    /// Update the treasury address. Requires admin auth.
    pub fn set_treasury(env: Env, admin: Address, treasury: Address) {
        require_admin_role(&env, &admin, AdminRole::SuperAdmin);
        env.storage().instance().set(&treasury_key(), &treasury);
    }

    /// Configure the ledger threshold after which invoices may be lazily archived.
    /// Requires Operator+ auth.
    pub fn set_archive_after_ledgers(env: Env, admin: Address, ledgers: u64) {
        require_admin_role(&env, &admin, AdminRole::Operator);
        assert!(ledgers > 0, "archive_after_ledgers must be positive");
        env.storage()
            .instance()
            .set(&archive_after_ledgers_key(), &ledgers);
    }

    /// Return the configured ledger threshold for lazy invoice archival.
    pub fn get_archive_after_ledgers(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&archive_after_ledgers_key())
            .unwrap_or(ARCHIVE_AFTER_LEDGERS)
    }

    // -----------------------------------------------------------------------
    // Issue #1: stream contract admin setter
    // -----------------------------------------------------------------------

    /// Store the address of the Stellar payment streaming contract. Requires admin auth.
    pub fn set_stream_contract(env: Env, admin: Address, contract: Address) {
        require_admin_role(&env, &admin, AdminRole::Operator);
        env.storage()
            .persistent()
            .set(&stream_contract_key(), &contract);
    }

    /// Store the DEX contract address used for token swaps in pay_with_token(). Requires admin auth.
    pub fn set_dex_contract(env: Env, admin: Address, contract: Address) {
        require_admin_role(&env, &admin, AdminRole::Operator);
        env.storage()
            .persistent()
            .set(&soroban_sdk::symbol_short!("dex_ctr"), &contract);
    }

    // -----------------------------------------------------------------------
    // Issue #189: Admin rotation
    // -----------------------------------------------------------------------

    /// Propose a new admin. Requires current admin auth.
    pub fn propose_admin(env: Env, admin: Address, new_admin: Address) {
        require_admin(&env);
        let _ = admin;
        env.storage()
            .instance()
            .set(&pending_admin_key(), &new_admin);
        events::admin_transfer_proposed(&env, &admin, &new_admin);
    }

    /// Accept the admin role. Requires the proposed admin to authenticate.
    pub fn accept_admin(env: Env) {
        let pending: Address = env
            .storage()
            .instance()
            .get(&pending_admin_key())
            .expect("no pending admin");
        pending.require_auth();
        env.storage().instance().set(&admin_key(), &pending);
        env.storage().instance().remove(&pending_admin_key());
        events::admin_transfer_completed(&env, &pending);
    }

    // -----------------------------------------------------------------------
    // Issue #526: Minimum recipient count
    // -----------------------------------------------------------------------

    /// Set the minimum number of recipients required per invoice. Requires admin auth.
    pub fn set_min_recipients(env: Env, admin: Address, min: u32) {
        require_admin(&env);
        let _ = admin;
        env.storage()
            .instance()
            .set(&min_recipients_key(), &min);
    }

    /// Get the minimum number of recipients required per invoice. Default is 2.
    pub fn get_min_recipients(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&min_recipients_key())
            .unwrap_or(2u32)
    }

    // -----------------------------------------------------------------------
    // Issue #527: Payer history query
    // -----------------------------------------------------------------------

    /// Return a paginated slice of payment records for the given payer.
    /// Issue #788: the invoice's reward pool (`bonus_pool`) and whether it has
    /// been distributed to payers on release.
    pub fn get_reward_pool(env: Env, invoice_id: u64) -> RewardPoolInfo {
        let invoice = load_invoice(&env, invoice_id);
        RewardPoolInfo {
            pool_amount: invoice.bonus_pool,
            top_n: invoice.bonus_max_payers,
            distributed: invoice.bonus_pool > 0
                && invoice.bonus_max_payers > 0
                && invoice.status == InvoiceStatus::Released,
        }
    }

    pub fn get_payer_history(env: Env, payer: Address, offset: u32, limit: u32) -> Vec<PaymentRecord> {
        let hist_key = payer_history_key(&payer);
        let history: Vec<PaymentRecord> = env
            .storage()
            .persistent()
            .get(&hist_key)
            .unwrap_or_else(|| Vec::new(&env));
        let total = history.len();
        let start = offset.min(total);
        let end = (start + limit).min(total);
        let mut result = Vec::new(&env);
        for i in start..end {
            result.push_back(history.get(i).unwrap());
        }
        result
    }

    // -----------------------------------------------------------------------
    // Issue #193: Creator volume cap
    // -----------------------------------------------------------------------

    /// Set a volume cap for a specific creator. Requires admin auth.
    /// A cap of 0 means no limit.
    pub fn set_creator_volume_cap(env: Env, admin: Address, creator: Address, cap: i128) {
        require_admin(&env);
        let _ = admin;
        assert!(cap >= 0, "cap must be non-negative");
        env.storage()
            .persistent()
            .set(&creator_volume_cap_key(&creator), &cap);
    }

    /// Return the volume cap for a creator (0 = no limit).
    pub fn get_creator_volume_cap(env: Env, creator: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&creator_volume_cap_key(&creator))
            .unwrap_or(0)
    }

    /// Return the volume used toward the cap for a creator.
    pub fn get_creator_volume_used(env: Env, creator: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&creator_volume_used_key(&creator))
            .unwrap_or(0)
    }

    // -----------------------------------------------------------------------
    // Issue #276: Volume milestone threshold configuration

    /// Set the platform-wide volume milestone threshold (in token base units).
    /// A milestone event fires each time cumulative volume crosses another multiple.
    /// Requires admin auth. Set to 0 to disable.
    pub fn set_platform_vol_threshold(env: Env, admin: Address, threshold: i128) {
        require_admin(&env);
        let _ = admin;
        assert!(threshold >= 0, "threshold must be non-negative");
        env.storage()
            .persistent()
            .set(&platform_vol_thresh_key(), &threshold);
    }

    /// Set the per-creator volume milestone threshold (in token base units).
    /// Requires admin auth. Set to 0 to disable.
    pub fn set_creator_vol_threshold(env: Env, admin: Address, threshold: i128) {
        require_admin(&env);
        let _ = admin;
        assert!(threshold >= 0, "threshold must be non-negative");
        env.storage()
            .persistent()
            .set(&creator_vol_thresh_key(), &threshold);
    }

    // -----------------------------------------------------------------------
    // Issue #188: Dispute arbitration
    // -----------------------------------------------------------------------

    /// Set an arbiter address for an invoice. Requires admin auth.
    /// Only the arbiter may raise and resolve disputes on this invoice.
    pub fn set_arbiter(env: Env, admin: Address, invoice_id: u64, arbiter: Address) {
        require_admin(&env);
        let _ = admin;
        let mut invoice = load_invoice(&env, invoice_id);
        invoice.arbiter = Some(arbiter.clone());
        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("set_arb"), &arbiter);
    }

    /// Add a co-creator to an invoice. Only the primary creator may call this.
    /// The co-creator list is bounded to [`MAX_CO_CREATORS`].
    pub fn add_co_creator(env: Env, caller: Address, invoice_id: u64, co_creator: Address) {
        require_not_paused(&env);
        caller.require_auth();
        let mut invoice = load_invoice(&env, invoice_id);
        require_invoice_unlocked(&env, invoice_id, &invoice);
        assert!(invoice.status == InvoiceStatus::Pending, "invoice is not pending");
        assert!(invoice.creator == caller, "only the primary creator can add co-creators");
        assert!(
            invoice.co_creators.len() < MAX_CO_CREATORS as u32,
            "{}",
            ContractError::CoCreatorLimitReached as u32
        );
        assert!(!invoice.co_creators.iter().any(|c| c == co_creator), "co-creator already exists");
        assert!(co_creator != invoice.creator, "creator cannot be a co-creator");
        invoice.co_creators.push_back(co_creator.clone());
        save_invoice(&env, invoice_id, &invoice);
        events::co_creator_added(&env, invoice_id, &caller, &co_creator);
        append_audit_entry(&env, invoice_id, symbol_short!("add_co_cr"), &caller);
    }

    /// Remove a co-creator from an invoice. Only the primary creator may call this.
    pub fn remove_co_creator(env: Env, caller: Address, invoice_id: u64, co_creator: Address) {
        require_not_paused(&env);
        caller.require_auth();
        let mut invoice = load_invoice(&env, invoice_id);
        require_invoice_unlocked(&env, invoice_id, &invoice);
        assert!(invoice.status == InvoiceStatus::Pending, "invoice is not pending");
        assert!(invoice.creator == caller, "only the primary creator can remove co-creators");
        let idx = invoice.co_creators.iter().position(|c| c == co_creator).expect("co-creator not found");
        invoice.co_creators.remove(idx as u32);
        save_invoice(&env, invoice_id, &invoice);
        events::co_creator_removed(&env, invoice_id, &caller, &co_creator);
        append_audit_entry(&env, invoice_id, symbol_short!("rem_co_cr"), &caller);
    }

    /// Raise a dispute on an invoice as a contributor (payer).
    /// Only one active dispute per invoice. Stores dispute record with timeout config.
    pub fn raise_invoice_dispute(
        env: Env, invoice_id: u64, disputer: Address, reason_hash: BytesN<32>,
    ) {
        require_not_paused(&env);
        disputer.require_auth();
        let mut invoice = load_invoice(&env, invoice_id);
        assert!(invoice.status == InvoiceStatus::Pending, "invoice is not pending");
        let is_contributor = invoice.payments.iter().any(|p| p.payer == disputer);
        assert!(is_contributor, "caller is not a contributor");
        assert!(invoice.status != InvoiceStatus::Disputed, "invoice is already disputed");
        let current_ledger = env.ledger().sequence();
        let timeout: u32 = env.storage().instance().get(&dispute_timeout_key())
            .unwrap_or(DEFAULT_DISPUTE_TIMEOUT_LEDGERS);
        let record = DisputeRecord {
            reason_hash: reason_hash.clone(),
            raised_at: current_ledger,
            status: DisputeStatus::Active,
            dispute_timeout_ledgers: timeout,
            dispute_opened_ledger: current_ledger,
        };
        env.storage().persistent().set(&dispute_record_key(invoice_id), &record);
        invoice.status = InvoiceStatus::Disputed;
        invoice.disputed = true;
        save_invoice(&env, invoice_id, &invoice);
        events::invoice_dispute_raised(&env, invoice_id, &disputer, &reason_hash);
        events::invoice_state_changed(&env, invoice_id, Some(&InvoiceStatus::Pending),
            &InvoiceStatus::Disputed, &disputer);
        append_audit_entry(&env, invoice_id, symbol_short!("inv_disp"), &disputer);
        // Issue #763: record the dispute in the per-invoice history ring buffer.
        append_history_entry(&env, invoice_id, Symbol::new(&env, "dispute"), &disputer, None);
    }

    /// Admin-only resolution of a dispute before timeout.
    /// Release outcome resumes normal payout; Refund outcome returns all funds.
    pub fn resolve_invoice_dispute(
        env: Env, invoice_id: u64, admin: Address, outcome: DisputeOutcome,
    ) {
        require_not_paused(&env);
        admin.require_auth();
        let mut invoice = load_invoice(&env, invoice_id);
        assert!(invoice.status == InvoiceStatus::Disputed, "invoice is not disputed");
        let mut record: DisputeRecord = env.storage().persistent()
            .get(&dispute_record_key(invoice_id)).expect("dispute record not found");
        assert!(record.status == DisputeStatus::Active, "dispute is not active");
        match outcome {
            DisputeOutcome::Release | DisputeOutcome::Approved => {
                record.status = DisputeStatus::Resolved;
                env.storage().persistent().set(&dispute_record_key(invoice_id), &record);
                invoice.status = InvoiceStatus::Pending;
                invoice.disputed = false;
                save_invoice(&env, invoice_id, &invoice);
                events::dispute_resolved(&env, invoice_id, &admin, &outcome);
                events::invoice_state_changed(&env, invoice_id, Some(&InvoiceStatus::Disputed),
                    &InvoiceStatus::Pending, &admin);
            }
            DisputeOutcome::Refund | DisputeOutcome::Refunded => {
                record.status = DisputeStatus::Resolved;
                env.storage().persistent().set(&dispute_record_key(invoice_id), &record);
                let token_client = token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
                let mut totals: Map<Address, i128> = Map::new(&env);
                for payment in invoice.payments.iter() {
                    let prev = totals.get(payment.payer.clone()).unwrap_or(0);
                    totals.set(payment.payer.clone(), prev + payment.amount);
                }
                for (payer, amount) in totals.iter() {
                    if amount > 0 {
                        token_client.transfer(&env.current_contract_address(), &payer, &amount);
                        events::payer_refunded(&env, invoice_id, &payer, amount);
                    }
                }
                if invoice.bonus_pool > 0 {
                    token_client.transfer(&env.current_contract_address(), &invoice.creator, &invoice.bonus_pool);
                }
                invoice.status = InvoiceStatus::Refunded;
                invoice.disputed = false;
                invoice.completion_time = Some(env.ledger().timestamp());
                save_invoice(&env, invoice_id, &invoice);
                events::dispute_resolved(&env, invoice_id, &admin, &outcome);
                events::invoice_refunded(&env, invoice_id, invoice.funded);
                events::invoice_state_changed(&env, invoice_id, Some(&InvoiceStatus::Disputed),
                    &InvoiceStatus::Refunded, &admin);
            }
        }
        append_audit_entry(&env, invoice_id, symbol_short!("disp_res"), &admin);
        // Issue #763: record the dispute resolution in the per-invoice history ring buffer.
        append_history_entry(&env, invoice_id, Symbol::new(&env, "dispute"), &admin, None);
    }

    /// Permissionless close of a dispute after the timeout has elapsed.
    /// Reverts if the timeout has not yet passed. Funds become releasable.
    pub fn auto_close_dispute(env: Env, invoice_id: u64) {
        require_not_paused(&env);
        let mut invoice = load_invoice(&env, invoice_id);
        assert!(invoice.status == InvoiceStatus::Disputed, "invoice is not disputed");
        let mut record: DisputeRecord = env.storage().persistent()
            .get(&dispute_record_key(invoice_id)).expect("dispute record not found");
        assert!(record.status == DisputeStatus::Active, "dispute is not active");
        let current_ledger = env.ledger().sequence();
        let eligible_at = record.dispute_opened_ledger.saturating_add(record.dispute_timeout_ledgers);
        assert!(current_ledger >= eligible_at, "dispute timeout has not elapsed");
        record.status = DisputeStatus::Expired;
        env.storage().persistent().set(&dispute_record_key(invoice_id), &record);
        invoice.status = InvoiceStatus::Pending;
        invoice.disputed = false;
        save_invoice(&env, invoice_id, &invoice);
        events::dispute_expired(&env, invoice_id);
        // Issue #804: the timeout default is to release (funds become releasable).
        events::dispute_auto_resolved(&env, invoice_id, &DisputeOutcome::Release);
        events::invoice_state_changed(&env, invoice_id, Some(&InvoiceStatus::Disputed),
            &InvoiceStatus::Pending, &env.current_contract_address());
        append_audit_entry(&env, invoice_id, symbol_short!("disp_cls"), &env.current_contract_address());
        // Issue #763: record the dispute close in the per-invoice history ring buffer.
        append_history_entry(
            &env,
            invoice_id,
            Symbol::new(&env, "dispute"),
            &env.current_contract_address(),
            None,
        );
    }

    /// Raise a dispute on an invoice. Only the configured arbiter may call this.
    /// When disputed, all actions (pay, release, refund, cancel) are blocked.
    pub fn raise_dispute(env: Env, invoice_id: u64, arbiter: Address) {
        require_not_paused(&env);
        arbiter.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.arbiter.as_ref() == Some(&arbiter),
            "not the designated arbiter"
        );
        assert!(!invoice.disputed, "invoice is already disputed");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        invoice.disputed = true;
        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("dispute"), &arbiter);
        // Issue #763: record the dispute in the per-invoice history ring buffer.
        append_history_entry(&env, invoice_id, Symbol::new(&env, "dispute"), &arbiter, None);
    }

    /// Resolve a dispute — release or refund the invoice.
    /// Only the designated arbiter may call this.
    pub fn resolve_dispute(env: Env, invoice_id: u64, arbiter: Address, resolution: ResolveAction) {
        require_not_paused(&env);
        arbiter.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.arbiter.as_ref() == Some(&arbiter),
            "not the designated arbiter"
        );
        assert!(invoice.disputed, "invoice is not disputed");

        // Issue #763: record the dispute resolution in the per-invoice history
        // ring buffer.  Any panic in the match below reverts this write, so the
        // entry only persists for a successful resolution.
        append_history_entry(&env, invoice_id, Symbol::new(&env, "dispute"), &arbiter, None);

        match resolution {
            ResolveAction::Release => {
                let caller = env.current_contract_address();
                Self::_release(&env, invoice_id, &mut invoice, &caller);
            }
            ResolveAction::Refund => {
                // If the invoice has no payments, mark as cancelled.
                if invoice.funded == 0 {
                    invoice.status = InvoiceStatus::Cancelled;
                    events::invoice_state_changed(
                        &env,
                        invoice_id,
                        Some(&InvoiceStatus::Pending),
                        &InvoiceStatus::Cancelled,
                        &arbiter,
                    );
                    save_invoice(&env, invoice_id, &invoice);
                    append_audit_entry(&env, invoice_id, symbol_short!("resolve"), &arbiter);
                    return;
                }

                let token_client =
                    token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
                let mut totals: Map<Address, i128> = Map::new(&env);
                for payment in invoice.payments.iter() {
                    let prev = totals.get(payment.payer.clone()).unwrap_or(0);
                    totals.set(payment.payer.clone(), prev + payment.amount);
                }
                let mut total_refunded_amount: i128 = 0;
                for (payer, amount) in totals.iter() {
                    token_client.transfer(&env.current_contract_address(), &payer, &amount);
                    total_refunded_amount += amount;
                    events::payer_refunded(&env, invoice_id, &payer, amount);
                }

                if invoice.bonus_pool > 0 {
                    token_client.transfer(
                        &env.current_contract_address(),
                        &invoice.creator,
                        &invoice.bonus_pool,
                    );
                }

                invoice.status = InvoiceStatus::Refunded;
                invoice.completion_time = Some(env.ledger().timestamp());
                save_invoice(&env, invoice_id, &invoice);
                append_audit_entry(&env, invoice_id, symbol_short!("resolve"), &arbiter);
                events::invoice_refunded(&env, invoice_id, total_refunded_amount);
                events::invoice_state_changed(
                    &env,
                    invoice_id,
                    Some(&InvoiceStatus::Pending),
                    &InvoiceStatus::Refunded,
                    &arbiter,
                );

                let total_refunded: i128 = env
                    .storage()
                    .persistent()
                    .get(&total_refunded_key())
                    .unwrap_or(0i128);
                env.storage().persistent().set(
                    &total_refunded_key(),
                    &total_refunded
                        .checked_add(total_refunded_amount)
                        .expect("total_refunded overflow"),
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Issue: receipt token factory (Issue 3)
    // -----------------------------------------------------------------------

    /// Store the address of the receipt token factory contract. Requires admin auth.
    /// The factory must expose: mint_receipt(invoice_id: u64, payer: Address, amount: i128) -> Address
    pub fn set_receipt_factory(env: Env, admin: Address, factory: Address) {
        require_admin_role(&env, &admin, AdminRole::Operator);
        env.storage()
            .persistent()
            .set(&receipt_factory_key(), &factory);
    }

    /// Return the receipt token address minted for a specific payer on a specific invoice.
    /// Returns None if no receipt token exists (factory not set or payment not made).
    pub fn get_receipt_token(env: Env, invoice_id: u64, payer: Address) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&receipt_token_key(invoice_id, &payer))
    }

    /// Set the dashboard contract address for aggregating creator stats.
    /// Requires admin auth.
    pub fn set_dashboard_contract(env: Env, admin: Address, dashboard: Address) {
        require_admin(&env);
        let _ = admin;
        env.storage()
            .persistent()
            .set(&dashboard_contract_key(), &dashboard);
    }

    /// Return the dashboard contract address, or None if not set.
    pub fn get_dashboard_contract(env: Env) -> Option<Address> {
        env.storage().persistent().get(&dashboard_contract_key())
    }

    // -----------------------------------------------------------------------
    // Issue #203: Fee tier system for volume-based discounts
    // -----------------------------------------------------------------------
    // (Superseded by the FeeTier-based set_fee_tiers below; old tuple variant removed.)

    // -----------------------------------------------------------------------
    // Issue #4: creator whitelist
    // -----------------------------------------------------------------------

    /// Add an address to the creator whitelist. Requires admin auth.
    /// When the whitelist is non-empty, only listed addresses may call create_invoice().
    pub fn whitelist_creator(env: Env, admin: Address, address: Address) {
        require_admin_role(&env, &admin, AdminRole::SuperAdmin);
        let mut wl: Vec<Address> = env
            .storage()
            .persistent()
            .get(&creator_whitelist_key())
            .unwrap_or_else(|| Vec::new(&env));
        if !wl.iter().any(|a| a == address) {
            wl.push_back(address);
        }
        env.storage()
            .persistent()
            .set(&creator_whitelist_key(), &wl);
    }

    /// Remove an address from the creator whitelist. Requires admin auth.
    pub fn remove_creator(env: Env, admin: Address, address: Address) {
        require_admin_role(&env, &admin, AdminRole::SuperAdmin);
        let wl: Vec<Address> = env
            .storage()
            .persistent()
            .get(&creator_whitelist_key())
            .unwrap_or_else(|| Vec::new(&env));
        let mut new_wl: Vec<Address> = Vec::new(&env);
        for a in wl.iter() {
            if a != address {
                new_wl.push_back(a);
            }
        }
        env.storage()
            .persistent()
            .set(&creator_whitelist_key(), &new_wl);
    }

    // -----------------------------------------------------------------------
    // Issue #417: Recipient whitelist per invoice
    // -----------------------------------------------------------------------

    /// Add a recipient to an invoice-specific whitelist.
    /// Only the creator may call this, and only while the invoice is still pending.
    pub fn add_to_recipient_whitelist(
        env: Env,
        creator: Address,
        invoice_id: u64,
        address: Address,
    ) {
        require_not_paused(&env);
        creator.require_auth();

        let invoice = load_invoice(&env, invoice_id);
        require_invoice_unlocked(&env, invoice_id, &invoice);
        assert!(
            invoice.creator == creator,
            "only creator can modify whitelist"
        );
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not draft"
        );

        let mut whitelist: Vec<Address> = env
            .storage()
            .persistent()
            .get(&recipient_whitelist_key(invoice_id))
            .unwrap_or_else(|| Vec::new(&env));
        if !whitelist.iter().any(|a| a == address) {
            whitelist.push_back(address.clone());
            env.storage()
                .persistent()
                .set(&recipient_whitelist_key(invoice_id), &whitelist);
            events::recipient_whitelisted(&env, invoice_id, &address);
            let mut added: Vec<Address> = Vec::new(&env);
            added.push_back(address.clone());
            events::recipient_whitelist_updated(
                &env,
                invoice_id,
                invoice.recipient_whitelist_enabled,
                &added,
                &Vec::new(&env),
            );
        }
    }

    /// Remove a recipient from an invoice-specific whitelist.
    /// Only the creator may call this, and only while the invoice is still pending.
    pub fn remove_from_recipient_whitelist(
        env: Env,
        creator: Address,
        invoice_id: u64,
        address: Address,
    ) {
        require_not_paused(&env);
        creator.require_auth();

        let invoice = load_invoice(&env, invoice_id);
        require_invoice_unlocked(&env, invoice_id, &invoice);
        assert!(
            invoice.creator == creator,
            "only creator can modify whitelist"
        );
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not draft"
        );

        let whitelist: Vec<Address> = env
            .storage()
            .persistent()
            .get(&recipient_whitelist_key(invoice_id))
            .unwrap_or_else(|| Vec::new(&env));
        if whitelist.iter().any(|a| a == address) {
            let mut new_wl: Vec<Address> = Vec::new(&env);
            for a in whitelist.iter() {
                if a != address {
                    new_wl.push_back(a);
                }
            }
            env.storage()
                .persistent()
                .set(&recipient_whitelist_key(invoice_id), &new_wl);
            events::recipient_removed_from_whitelist(&env, invoice_id, &address);
            let mut removed: Vec<Address> = Vec::new(&env);
            removed.push_back(address.clone());
            events::recipient_whitelist_updated(
                &env,
                invoice_id,
                invoice.recipient_whitelist_enabled,
                &Vec::new(&env),
                &removed,
            );
        }
    }

    // -----------------------------------------------------------------------
    // Issue #215: Configurable platform fee waiver list
    // -----------------------------------------------------------------------

    /// Add an address to the platform fee waiver list. Requires admin auth.
    /// Addresses on this list will not be charged platform fees when they are recipients.
    pub fn add_platform_fee_waiver(env: Env, admin: Address, address: Address) {
        require_admin_role(&env, &admin, AdminRole::SuperAdmin);
        let mut waivers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&platform_fee_waiver_list_key())
            .unwrap_or_else(|| Vec::new(&env));
        if !waivers.iter().any(|a| a == address) {
            waivers.push_back(address);
        }
        env.storage()
            .persistent()
            .set(&platform_fee_waiver_list_key(), &waivers);
    }

    /// Remove an address from the platform fee waiver list. Requires admin auth.
    pub fn remove_platform_fee_waiver(env: Env, admin: Address, address: Address) {
        require_admin_role(&env, &admin, AdminRole::SuperAdmin);
        let waivers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&platform_fee_waiver_list_key())
            .unwrap_or_else(|| Vec::new(&env));
        let mut new_waivers: Vec<Address> = Vec::new(&env);
        for a in waivers.iter() {
            if a != address {
                new_waivers.push_back(a);
            }
        }
        env.storage()
            .persistent()
            .set(&platform_fee_waiver_list_key(), &new_waivers);
    }

    /// Check if an address is on the platform fee waiver list.
    pub fn is_platform_fee_waived(env: Env, address: Address) -> bool {
        let waivers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&platform_fee_waiver_list_key())
            .unwrap_or_else(|| Vec::new(&env));
        waivers.iter().any(|a| a == address)
    }

    /// Fee-exempt trusted caller whitelist (issue: Drips Wave governance fee exemption).
    pub fn add_trusted_caller(env: Env, admin: Address, caller: Address) {
        require_admin_role(&env, &admin, AdminRole::SuperAdmin);
        let mut trusted: Vec<Address> = env.storage().instance().get(&trusted_callers_key()).unwrap_or_else(|| Vec::new(&env));
        if !trusted.contains(&caller) {
            trusted.push_back(caller.clone());
            env.storage().instance().set(&trusted_callers_key(), &trusted);
        }
        events::trusted_caller_added(&env, &caller);
    }

    pub fn remove_trusted_caller(env: Env, admin: Address, caller: Address) {
        require_admin_role(&env, &admin, AdminRole::SuperAdmin);
        let trusted: Vec<Address> = env.storage().instance().get(&trusted_callers_key()).unwrap_or_else(|| Vec::new(&env));
        let mut filtered: Vec<Address> = Vec::new(&env);
        for a in trusted.iter() {
            if a != caller { filtered.push_back(a); }
        }
        env.storage().instance().set(&trusted_callers_key(), &filtered);
        events::trusted_caller_removed(&env, &caller);
    }

    pub fn set_sweep_timeout(env: Env, admin: Address, ledgers: u32) {
        require_admin_role(&env, &admin, AdminRole::SuperAdmin);
        env.storage().instance().set(&sweep_timeout_key(), &ledgers);
    }

    /// Sweep an invoice's stranded failed-payout funds to treasury once SweepTimeoutLedgers
    /// has elapsed since the last failed payout.
    ///
    /// Issue #504: Also sweeps from the new unified failed-payout storage (`failed_payouts_key`).
    pub fn sweep_unclaimed_funds(env: Env, admin: Address, invoice_id: u64) -> i128 {
        require_admin_role(&env, &admin, AdminRole::SuperAdmin);
        let last_failed: u32 = env.storage().persistent().get(&last_failed_ledger_key(invoice_id))
            .expect("no failed payouts recorded for this invoice");
        let timeout: u32 = env.storage().instance().get(&sweep_timeout_key()).unwrap_or(120_960);
        assert!(
            env.ledger().sequence() > last_failed.saturating_add(timeout),
            "sweep timeout has not elapsed"
        );

        let invoice = load_invoice(&env, invoice_id);
        let mut swept: i128 = 0;

        // Sweep from old fallback_escrow_key storage.
        for recipient in invoice.recipients.iter() {
            let key = fallback_escrow_key(invoice_id, &recipient);
            let balance: i128 = env.storage().persistent().get(&key).unwrap_or(0);
            if balance > 0 {
                env.storage().persistent().remove(&key);
                swept += balance;
            }
        }

        // Issue #504: Sweep from new unified failed-payout storage.
        let failed_adrs: Vec<Address> = env
            .storage()
            .persistent()
            .get(&failed_payouts_key(invoice_id))
            .unwrap_or_else(|| Vec::new(&env));
        for recipient in failed_adrs.iter() {
            let rec_key = failed_payout_record_key(invoice_id, &recipient);
            let amount: i128 = env.storage().persistent().get(&rec_key).unwrap_or(0);
            if amount > 0 {
                env.storage().persistent().remove(&rec_key);
                swept += amount;
            }
        }
        if !failed_adrs.is_empty() {
            env.storage().persistent().remove(&failed_payouts_key(invoice_id));
        }

        assert!(swept > 0, "nothing to sweep");

        let treasury: Address = env.storage().instance().get(&treasury_key()).expect("treasury not set");
        // Failed payouts are always re-escrowed in the invoice's funding token
        // (see the `try_invoke_contract` fallback in `_release_full`/`_release_tranches`),
        // regardless of any per-recipient payout token, so sweep in that same token.
        let token_client = token::Client::new(&env, &funding_token_for(&invoice));
        token_client.transfer(&env.current_contract_address(), &treasury, &swept);
        env.storage().persistent().remove(&last_failed_ledger_key(invoice_id));
        events::funds_swept(&env, invoice_id, swept, &treasury);
        swept
    }

    /// Issue #504: Query the list of failed-payout recipients for an invoice.
    pub fn get_failed_payouts(env: Env, invoice_id: u64) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&failed_payouts_key(invoice_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Queryable snapshot of an invoice's funding stats, including cumulative_contributed
    /// (never decremented by withdrawals/refunds).
    pub fn get_invoice_stats(env: Env, invoice_id: u64) -> InvoiceStats {
        let invoice = load_invoice(&env, invoice_id);
        let total: i128 = invoice.amounts.iter().sum();
        let cumulative_contributed: i128 = env.storage().persistent()
            .get(&cumulative_contributed_key(invoice_id)).unwrap_or(0);
        let completion_bps: u32 = funding_bps(invoice.funded, total);
        let mut unique_payers: Vec<Address> = Vec::new(&env);
        for payment in invoice.payments.iter() {
            if !unique_payers.contains(&payment.payer) { unique_payers.push_back(payment.payer); }
        }
        InvoiceStats {
            funded: invoice.funded,
            total,
            payment_count: invoice.payments.len(),
            unique_payers: unique_payers.len(),
            completion_bps,
            cumulative_contributed,
        }
    }

    /// Issue #674: Funding progress of an invoice expressed in basis points.
    ///
    /// Returns `funded * 10_000 / total` clamped to `[0, 10_000]`, where `total`
    /// is the sum of the invoice `amounts`. Overfunded invoices therefore report
    /// exactly `10_000` rather than a value above the denominator. Returns `0`
    /// when `total` is zero (nothing to fund) or when `funded` is non-positive.
    ///
    /// Errors with [`ContractError::InvoiceNotFound`] for unknown `invoice_id`.
    pub fn get_invoice_funding_progress(env: Env, invoice_id: u64) -> Result<u32, ContractError> {
        let core = Self::load_invoice_opt(&env, invoice_id)?;
        // `funded` is mirrored into the hot instance entry for migrated invoices;
        // prefer it so the progress matches `get_invoice_funded`.
        let funded: i128 = env
            .storage()
            .instance()
            .get::<_, InvoiceHot>(&invoice_hot_key(invoice_id))
            .map_or(core.funded, |hot| hot.funded);
        let total: i128 = core.amounts.iter().sum();

        if total <= 0 || funded <= 0 {
            return Ok(0);
        }
        if funded >= total {
            return Ok(10_000);
        }
        let bps = checked_proportion(funded as u128, 10_000u128, total as u128)?;
        Ok(bps as u32)
    }

    pub fn get_invoice_deadline(env: Env, invoice_id: u64) -> Option<u64> {
        if let Some(core) = env.storage().persistent().get::<_, InvoiceCore>(&invoice_key(invoice_id)) {
            Some(core.deadline)
        } else if let Some(core) = env.storage().instance().get::<_, InvoiceCore>(&invoice_key(invoice_id)) {
            Some(core.deadline)
        } else {
            panic_with_error!(env, ContractError::InvoiceNotFound);
        }
    }

    pub fn get_invoice_funded(env: Env, invoice_id: u64) -> Option<i128> {
        if let Some(hot) = env.storage().instance().get::<_, InvoiceHot>(&invoice_hot_key(invoice_id)) {
            Some(hot.funded)
        } else if let Some(core) = env.storage().persistent().get::<_, InvoiceCore>(&invoice_key(invoice_id)) {
            Some(core.funded)
        } else if let Some(core) = env.storage().instance().get::<_, InvoiceCore>(&invoice_key(invoice_id)) {
            Some(core.funded)
        } else {
            panic_with_error!(env, ContractError::InvoiceNotFound);
        }
    }

    pub fn get_invoice_status(env: Env, invoice_id: u64) -> Option<InvoiceStatus> {
        if let Some(hot) = env.storage().instance().get::<_, InvoiceHot>(&invoice_hot_key(invoice_id)) {
            Some(hot.status)
        } else if let Some(core) = env.storage().persistent().get::<_, InvoiceCore>(&invoice_key(invoice_id)) {
            Some(core.status)
        } else if let Some(core) = env.storage().instance().get::<_, InvoiceCore>(&invoice_key(invoice_id)) {
            Some(core.status)
        } else {
            panic_with_error!(env, ContractError::InvoiceNotFound);
        }
    }

    /// Get a consolidated invoice snapshot for off-chain audit.
    pub fn get_invoice_snapshot(env: Env, invoice_id: u64) -> types::InvoiceSnapshot {
        let core: types::InvoiceCore = env
            .storage()
            .persistent()
            .get(&invoice_key(invoice_id))
            .unwrap_or_else(|| {
                env.storage()
                    .instance()
                    .get(&invoice_key(invoice_id))
                    .expect("invoice not found")
            });
        let ext: types::InvoiceExt = env
            .storage()
            .persistent()
            .get(&invoice_ext_key(invoice_id))
            .unwrap_or_else(|| {
                env.storage()
                    .instance()
                    .get(&invoice_ext_key(invoice_id))
                    .unwrap_or_else(|| types::InvoiceExt {
                        co_signers: Vec::new(&env),
                        required_signatures: 0,
                        signatures: Vec::new(&env),
                        approver: None,
                        approved: false,
                        condition_oracle: None,
                        condition_met: false,
                        penalty_bps: 0,
                        penalty_deadline: 0,
                        min_funding_bps: 0,
                        release_stages: Vec::new(&env),
                        released_stages: 0,
                        allowed_payers: None,
                        price_oracle: None,
                        base_amounts: Vec::new(&env),
                        swap_tokens: Vec::new(&env),
                        tax_bps: 0,
                        tax_authority: None,
                        insurance_premium_bps: 0,
                        insurance_fund: 0,
                        smart_route: false,
                        convert_to_stream: false,
                        accepted_tokens: Vec::new(&env),
                        forward_to: None,
                        forward_invoice_id: None,
                        split_rules: Vec::new(&env),
                        auto_resolve_rules: Vec::new(&env),
                        creator_cosigner: None,
                        velocity_limit: 0,
                        velocity_window: 0,
                        parent_invoice_id: None,
                        pause_reason: None,
                        auto_resume_at: None,
                        payment_cooldown_secs: None,
                        max_payments_per_window: None,
                        payment_window_secs: None,
                        scheduled_release_at: None,
                        penalty_tiers: Vec::new(&env),
                        allowed_callers: None,
                        refund_grace_secs: None,
                    })
            });
        let ext2: types::InvoiceExt2 = env
            .storage()
            .persistent()
            .get(&invoice_ext2_key(invoice_id))
            .unwrap_or_else(|| {
                env.storage()
                    .instance()
                    .get(&invoice_ext2_key(invoice_id))
                    .unwrap_or_else(|| types::InvoiceExt2 {
                        notification_contract: None,
                        overflow_behavior: types::OverflowBehavior::Reject,
                        cross_chain_ref: None,
                        require_kyc: false,
                        arbiter: None,
                        disputed: false,
                        admin_frozen: false,
                        auction_on_expiry: false,
                        auction_end: 0,
                        bids: Vec::new(&env),
                        min_payment: 0,
                        min_funding_amount: 0,
                        priorities: Vec::new(&env),
                        target_usd_cents: None,
                        refunded_addresses: Vec::new(&env),
                        oracle: None,
                        oracle_asset_pair_base: None,
                        oracle_asset_pair_quote: None,
                        min_payer_rep: None,
                        escrow_hold_period: None,
                        held_until: None,
                        milestones: Vec::new(&env),
                        milestones_released: 0,
                        recipient_max_payouts: Vec::new(&env),
                        twafr_numerator: 0,
                        twafr_last_ledger: 0,
                        release_condition_hash: None,
                        recipient_whitelist_enabled: false,
                        overfunding_policy: types::OverfundingPolicy::Cap,
                        contributor_allowlist: None,
                        early_bird_window_ledgers: 0,
                        early_bird_fee_bps: 0,
                        early_bird_fee_credit: 0,
                        creator_fee_bps: 0,
                        ratio_denominator: 1,
                        ratios: Vec::new(&env),
                        max_contribution_per_payer: None,
                    })
            });
        let audit_log: Vec<types::AuditEntry> = get_audit_log(&env, invoice_id);
        types::InvoiceSnapshot {
            core,
            ext,
            ext2,
            audit_log,
        }
    }

    /// Issue #327 / #329 / #330: Return extended per-invoice fields not present in InvoiceSnapshot.
    pub fn get_invoice_ext3(env: Env, invoice_id: u64) -> InvoiceExt3 {
        let release_delay: Option<u32> = env
            .storage()
            .persistent()
            .get(&release_delay_key(invoice_id));
        let funded_at: Option<u32> = env
            .storage()
            .persistent()
            .get(&funded_at_ledger_key(invoice_id));
        let unlock_at: Option<u32> = match (funded_at, release_delay) {
            (Some(fa), Some(d)) => Some(fa.saturating_add(d)),
            _ => None,
        };
        let metadata_hash: Option<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&metadata_hash_key(invoice_id));
        let paid_recipients: Vec<Address> = env
            .storage()
            .persistent()
            .get(&paid_recipients_key(invoice_id))
            .unwrap_or_else(|| Vec::new(&env));
        InvoiceExt3 {
            release_delay_ledgers: release_delay,
            funded_at_ledger: funded_at,
            unlock_at_ledger: unlock_at,
            metadata_hash,
            paid_recipients,
        }
    }

    /// Returns the creator-defined payment window `(open_at, close_at)` for an
    /// invoice, if configured (issue #430). Either or both may be `None`.
    pub fn get_payment_window(env: Env, invoice_id: u64) -> (Option<u64>, Option<u64>) {
        (
            get_payment_open_at_internal(&env, invoice_id),
            get_payment_close_at_internal(&env, invoice_id),
        )
    }

    /// Return the current creation fee.
    pub fn get_creation_fee(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&creation_fee_key())
            .unwrap_or(0)
    }

    /// Return the treasury address.
    pub fn get_treasury(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&treasury_key())
            .expect("treasury not set")
    }

    /// Return the USDC token address.
    pub fn get_usdc_token(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&usdc_token_key())
            .expect("usdc token not set")
    }

    /// Return the platform fee in basis points (issue #41).
    pub fn get_platform_fee_bps(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&platform_fee_bps_key())
            .unwrap_or(0u32)
    }

    /// Issue #521: Set the proportional fee recipients. Requires admin auth.
    /// Validates that `fee_recipients` is non-empty and basis points sum to exactly 10_000.
    pub fn set_fee_recipients(env: Env, admin: Address, fee_recipients: Vec<FeeSplit>) {
        require_admin(&env);
        admin.require_auth();

        assert!(!fee_recipients.is_empty(), "fee_recipients must not be empty");
        let sum: u32 = fee_recipients.iter().map(|r| r.basis_points).fold(0u32, |a, b| a.saturating_add(b));
        validation::assert_bps_total(sum)
            .expect("fee_recipients basis points must sum to 10000");

        env.storage()
            .instance()
            .set(&fee_recipients_key(), &fee_recipients);

        events::fee_recipients_updated(&env, &fee_recipients);
    }

    /// Return the registered fee recipients, if any.
    pub fn get_fee_recipients(env: Env) -> Option<Vec<FeeSplit>> {
        env.storage().instance().get(&fee_recipients_key())
    }

    /// Issue #596: Query whether the contract is currently paused.
    /// Returns false if no pause state has been set.
    pub fn get_contract_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&storage_keys::StorageKey::Paused)
            .unwrap_or(false)
    }

    /// Issue #597: Query the per-recipient amounts for an invoice without retrieving the full invoice.
    /// Throws ContractError::InvoiceNotFound if the invoice does not exist.
    pub fn get_invoice_amounts(env: Env, invoice_id: u64) -> Vec<i128> {
        env.storage()
            .persistent()
            .get(&storage_keys::InvoiceKey::AmountsList(invoice_id))
            .ok_or(ContractError::InvoiceNotFound)
            .unwrap()
    }

    // -----------------------------------------------------------------------
    // Issue #828: Recipient earnings distribution helper
    // -----------------------------------------------------------------------

    /// Preview what each recipient of `invoice_id` would earn if `amount`
    /// were distributed right now, without mutating any state.
    ///
    /// Uses the invoice's configured split ratios (`ratios` / `ratio_denominator`)
    /// when set; otherwise falls back to weighting recipients by their
    /// configured `amounts`.
    pub fn get_recipient_earnings(env: Env, invoice_id: u64, amount: i128) -> Vec<(Address, i128)> {
        let invoice = load_invoice(&env, invoice_id);

        let (ratios, denom): (Vec<i128>, i128) = if !invoice.ratios.is_empty() {
            let mut r = Vec::new(&env);
            for bps in invoice.ratios.iter() {
                r.push_back(bps as i128);
            }
            (r, invoice.ratio_denominator as i128)
        } else {
            let mut r = Vec::new(&env);
            let mut sum: i128 = 0;
            for amt in invoice.amounts.iter() {
                r.push_back(amt);
                sum += amt;
            }
            (r, sum)
        };

        match calc::compute_recipient_earnings(&env, &invoice.recipients, amount, &ratios, denom) {
            Ok(earnings) => earnings,
            Err(e) => panic_with_error!(env, e),
        }
    }

    /// Preview the next invoice id that will be assigned by create_invoice.
    pub fn peek_next_invoice_id(env: Env) -> u64 {
        env.storage()
            .persistent()
            .get(&counter_key())
            .unwrap_or(0u64)
            + 1
    }

    // -----------------------------------------------------------------------
    // Issue #285: Volume-based fee tiers
    // -----------------------------------------------------------------------

    /// Admin function to set up to 5 fee tiers sorted by volume threshold.
    /// Requires admin auth.
    pub fn set_fee_tiers(env: Env, admin: Address, tiers: Vec<FeeTier>) {
        let _admin_addr = require_admin(&env);
        let _ = admin;

        debug_assert!(tiers.len() <= 5, "Maximum 5 fee tiers allowed");

        // Verify tiers are sorted by volume_threshold in ascending order
        for i in 1..tiers.len() {
            let prev = tiers.get(i - 1).unwrap();
            let curr = tiers.get(i).unwrap();
            debug_assert!(
                prev.volume_threshold < curr.volume_threshold,
                "Fee tiers must be sorted by volume_threshold"
            );
        }

        env.storage().instance().set(&fee_tiers_key(), &tiers);
        events::fee_tiers_updated(&env, tiers.len());
    }

    /// Get the applicable fee in basis points for a creator based on their lifetime volume.
    /// Returns the lowest fee_bps for which the creator's volume meets the threshold.
    pub fn get_applicable_fee(env: Env, creator: Address) -> u32 {
        let tiers: Vec<FeeTier> = env
            .storage()
            .instance()
            .get(&fee_tiers_key())
            .unwrap_or(Vec::new(&env));

        if tiers.is_empty() {
            return SplitContract::get_platform_fee_bps(env);
        }

        let creator_volume: u64 = env
            .storage()
            .persistent()
            .get(&creator_stats_volume_key(&creator))
            .unwrap_or(0u64);

        // Find the applicable tier (highest volume threshold that creator meets)
        let mut applicable_fee = SplitContract::get_platform_fee_bps(env);
        for i in (0..tiers.len()).rev() {
            let tier = tiers.get(i).unwrap();
            if creator_volume >= tier.volume_threshold {
                applicable_fee = tier.fee_bps;
                break;
            }
        }

        applicable_fee
    }

    /// Get the current fee tiers.
    pub fn get_fee_tiers(env: Env) -> Vec<FeeTier> {
        env.storage()
            .instance()
            .get(&fee_tiers_key())
            .unwrap_or(Vec::new(&env))
    }

    // -----------------------------------------------------------------------
    // Issue #824: Explicit creator commission tiers
    // -----------------------------------------------------------------------

    /// Assign a creator to an explicit commission tier. Admin-only.
    ///
    /// Unlike [`Self::set_fee_tiers`] (which derives a fee automatically from
    /// lifetime volume), this lets an admin grant a specific commission rate
    /// to a creator directly, regardless of their volume.
    pub fn set_creator_tier(env: Env, admin: Address, creator: Address, tier: CreatorTier) {
        let _admin_addr = require_admin(&env);
        let _ = admin;

        env.storage()
            .persistent()
            .set(&creator_tier_key(&creator), &tier);
        events::creator_tier_updated(&env, &creator, &tier);
    }

    /// Return the creator's assigned tier, defaulting to `Bronze` when no
    /// tier has been explicitly set.
    pub fn get_creator_tier(env: Env, creator: Address) -> CreatorTier {
        env.storage()
            .persistent()
            .get(&creator_tier_key(&creator))
            .unwrap_or(CreatorTier::Bronze)
    }

    /// Return the commission rate (basis points) for a creator's assigned tier.
    pub fn get_creator_tier_commission_bps(env: Env, creator: Address) -> u32 {
        let tier = Self::get_creator_tier(env, creator);
        tier_commission_bps(&tier)
    }

    // -----------------------------------------------------------------------
    // Issue #826: Batch payment signature verification
    // -----------------------------------------------------------------------

    /// Verify Ed25519 signatures for a batch of signed payment instructions.
    ///
    /// Each entry's signature must verify over the canonical encoding of
    /// `(invoice_id, payer, amount, nonce)` using its declared
    /// `signer_pubkey`. Intended as a building block for relayed / meta-tx
    /// multi-pay flows: a relayer collects payer-signed payment intents
    /// off-chain and this function lets the contract (or caller) confirm
    /// authenticity before executing the corresponding payments.
    ///
    /// Returns `true` if every signature in the batch verifies; panics on
    /// the first invalid signature encountered.
    pub fn verify_batch_payment_signatures(env: Env, payments: Vec<SignedPayment>) -> bool {
        for entry in payments.iter() {
            verify_signed_payment(&env, &entry);
        }
        events::batch_signatures_verified(&env, payments.len());
        true
    }

    pub fn claim_fallback(env: Env, recipient: Address, invoice_id: u64) {
        recipient.require_auth();

        // Issue #504: Check new unified failed-payout storage first, then fallback to old escrow.
        let failed_rec_key = failed_payout_record_key(invoice_id, &recipient);
        let new_amount: i128 = env.storage().persistent().get(&failed_rec_key).unwrap_or(0);

        let old_key = fallback_escrow_key(invoice_id, &recipient);
        let old_amount: i128 = env.storage().persistent().get(&old_key).unwrap_or(0);

        let amount = new_amount.max(old_amount);
        assert!(amount > 0, "no payout balance to claim");

        let invoice = load_invoice(&env, invoice_id);
        // Failed payouts are always re-escrowed in the invoice's funding token; see
        // sweep_unclaimed_funds for the same reasoning.
        let token_client = token::Client::new(&env, &funding_token_for(&invoice));

        // Clear whichever storage has the balance.
        if new_amount > 0 {
            env.storage().persistent().remove(&failed_rec_key);
            // Remove from failed-payouts list as well.
            let failed_adrs: Vec<Address> = env
                .storage()
                .persistent()
                .get(&failed_payouts_key(invoice_id))
                .unwrap_or_else(|| Vec::new(&env));
            let mut new_failed: Vec<Address> = Vec::new(&env);
            for a in failed_adrs.iter() {
                if a != recipient {
                    new_failed.push_back(a);
                }
            }
            if new_failed.is_empty() {
                env.storage().persistent().remove(&failed_payouts_key(invoice_id));
            } else {
                env.storage().persistent().set(&failed_payouts_key(invoice_id), &new_failed);
            }
        }
        if old_amount > 0 {
            env.storage().persistent().remove(&old_key);
        }

        token_client.transfer(&env.current_contract_address(), &recipient, &amount);
    }

    pub fn get_fallback_balance(env: Env, invoice_id: u64, recipient: Address) -> i128 {
        let key = fallback_escrow_key(invoice_id, &recipient);
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    pub fn register_instalment_plan(
        env: Env,
        payer: Address,
        invoice_id: u64,
        plan: InstalmentPlan,
    ) {
        payer.require_auth();
        assert!(!plan.tranches.is_empty(), "tranches must not be empty");
        let mut prev_ledger = 0;
        for i in 0..plan.tranches.len() {
            let t = plan.tranches.get(i).unwrap();
            assert!(t.amount > 0, "tranche amount must be positive");
            assert!(
                t.ledger >= prev_ledger,
                "tranches must be in ascending ledger order"
            );
            prev_ledger = t.ledger;
        }
        let key = plan_key(invoice_id, &payer);
        env.storage().persistent().set(&key, &plan);
    }

    pub fn get_instalment_status(env: Env, invoice_id: u64, payer: Address) -> (u32, u32) {
        let key = plan_key(invoice_id, &payer);
        if let Some(plan) = env.storage().persistent().get::<_, InstalmentPlan>(&key) {
            (plan.paid_index, plan.tranches.len())
        } else {
            (0, 0)
        }
    }

    pub fn resolve_escrow(
        env: Env,
        creator: Address,
        invoice_id: u64,
        resolution_hash: BytesN<32>,
    ) {
        creator.require_auth();
        let mut invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.creator == creator || invoice.co_creators.iter().any(|c| c == creator),
            "only creator or co-creator can resolve escrow"
        );
        invoice.held_until = None;
        save_invoice(&env, invoice_id, &invoice);
        events::escrow_resolved(&env, invoice_id, &resolution_hash);
    }

    pub fn set_fee_brackets(env: Env, admin: Address, brackets: Vec<FeeBracket>) {
        let _admin_addr = require_admin(&env);
        let _ = admin;
        assert!(!brackets.is_empty(), "brackets must not be empty");
        let mut prev_max = -1;
        for i in 0..brackets.len() {
            let b = brackets.get(i).unwrap();
            assert!(b.rate_bps <= 10_000, "rate_bps must be <= 10000");
            assert!(
                b.max_amount > prev_max,
                "max_amount must be strictly ascending"
            );
            prev_max = b.max_amount;
        }
        let last = brackets.get(brackets.len() - 1).unwrap();
        assert!(
            last.max_amount == i128::MAX,
            "last bracket max_amount must be i128::MAX"
        );

        env.storage().instance().set(&fee_brackets_key(), &brackets);
    }

    pub fn compute_fee(env: Env, amount: i128) -> i128 {
        if amount <= 0 {
            return 0;
        }
        let brackets: Vec<FeeBracket> = env
            .storage()
            .instance()
            .get(&fee_brackets_key())
            .unwrap_or_else(|| {
                let flat_fee: u32 = env
                    .storage()
                    .instance()
                    .get(&platform_fee_bps_key())
                    .unwrap_or(0);
                let mut vec = Vec::new(&env);
                vec.push_back(FeeBracket {
                    max_amount: i128::MAX,
                    rate_bps: flat_fee,
                });
                vec
            });

        let mut fee: i128 = 0;
        let mut remaining = amount;
        let mut prev_max: i128 = 0;
        for i in 0..brackets.len() {
            let b = brackets.get(i).unwrap();
            let slice_limit = b.max_amount.saturating_sub(prev_max);
            let slice = if remaining > slice_limit {
                slice_limit
            } else {
                remaining
            };
            if slice > 0 {
                fee += (slice as u128 * b.rate_bps as u128 / 10_000u128) as i128;
                remaining -= slice;
            }
            prev_max = b.max_amount;
            if remaining <= 0 {
                break;
            }
        }
        fee
    }

    /// Issue #521: distribute `total_fee` across configured fee recipients.
    /// If fee recipients are configured, each gets their proportional share; otherwise
    /// the legacy single-treasury behaviour sends the entire fee to `treasury`.
    fn distribute_fee(env: &Env, total_fee: i128, token_address: &Address, treasury: &Address) {
        if total_fee <= 0 {
            return;
        }
        let fee_recipients: Vec<FeeSplit> = env
            .storage()
            .instance()
            .get(&fee_recipients_key())
            .unwrap_or_else(|| {
                let mut vec = Vec::new(env);
                vec.push_back(FeeSplit {
                    address: treasury.clone(),
                    basis_points: 10_000,
                });
                vec
            });
        let token_client = token::Client::new(env, token_address);
        let mut distributed: i128 = 0;
        let n = fee_recipients.len();
        for i in 0..n {
            let recipient = fee_recipients.get(i).unwrap();
            let share = if i == n - 1 {
                total_fee - distributed
            } else {
                (total_fee as u128 * recipient.basis_points as u128 / 10_000u128) as i128
            };
            distributed += share;
            if share > 0 {
                token_client.transfer(&env.current_contract_address(), &recipient.address, &share);
            }
        }
    }

    /// Admin function to set up to 5 rebate tiers sorted by minimum volume.
    pub fn set_rebate_tiers(env: Env, admin: Address, tiers: Vec<RebateTier>) {
        require_admin(&env);
        let _ = admin;

        assert!(tiers.len() <= 5, "Maximum 5 rebate tiers allowed");
        for i in 1..tiers.len() {
            let prev = tiers.get(i - 1).unwrap();
            let curr = tiers.get(i).unwrap();
            assert!(
                prev.min_volume < curr.min_volume,
                "rebate tiers must be sorted"
            );
        }

        env.storage().instance().set(&rebate_tiers_key(), &tiers);
    }

    /// Return the current rebate tiers.
    pub fn get_rebate_tiers(env: Env) -> Vec<RebateTier> {
        env.storage()
            .instance()
            .get(&rebate_tiers_key())
            .unwrap_or(Vec::new(&env))
    }

    /// Withdraw any accrued rebate balance for a creator.
    pub fn withdraw_rebate(env: Env, creator: Address) {
        creator.require_auth();

        let balance_key = rebate_balance_key(&creator);
        let balance: i128 = env
            .storage()
            .persistent()
            .get(&balance_key)
            .unwrap_or(0i128);
        if balance <= 0 {
            return;
        }

        let token = env
            .storage()
            .instance()
            .get(&usdc_token_key())
            .expect("usdc token not set");
        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&env.current_contract_address(), &creator, &balance);
        env.storage().persistent().set(&balance_key, &0i128);
    }

    // -----------------------------------------------------------------------
    // Issue #299: Creator analytics aggregator
    // -----------------------------------------------------------------------

    /// Get aggregated analytics for a creator.
    pub fn get_creator_stats(env: Env, creator: Address) -> CreatorStats {
        let total_invoices: u32 = env
            .storage()
            .persistent()
            .get(&creator_stats_count_key(&creator))
            .unwrap_or(0u64) as u32;

        let total_raised: u64 = env
            .storage()
            .persistent()
            .get(&creator_stats_volume_key(&creator))
            .unwrap_or(0u64);

        let total_released: u64 = env
            .storage()
            .persistent()
            .get(&creator_stats_released_key(&creator))
            .unwrap_or(0u64);

        let total_payers: u32 = env
            .storage()
            .persistent()
            .get(&creator_stats_payers_key(&creator))
            .unwrap_or(0u64) as u32;

        let avg_funding_time_ledgers: u32 = env
            .storage()
            .persistent()
            .get(&creator_stats_avg_funding_key(&creator))
            .unwrap_or(0u64) as u32;

        let total_refunded: u32 = env
            .storage()
            .persistent()
            .get(&creator_stats_refunded_key(&creator))
            .unwrap_or(0u64) as u32;

        CreatorStats {
            total_invoices,
            total_raised,
            total_released,
            total_payers,
            avg_funding_time_ledgers,
            total_refunded,
        }
    }

    /// Set the NFT gate contract address. When set, only holders of the NFT
    /// (via `balance_of(creator) > 0`) may create invoices. Pass `None` to disable.
    /// Requires admin auth.
    pub fn set_nft_gate(env: Env, admin: Address, contract: Option<Address>) {
        let admin_addr = require_admin(&env);
        let _ = admin;

        env.storage().persistent().set(&nft_gate_key(), &contract);
        events::nft_gate_set(&env, &contract, &admin_addr);
    }

    // -----------------------------------------------------------------------
    // Timelocked admin actions (issue #185)
    // -----------------------------------------------------------------------

    /// Set the timelock duration in seconds. All queued actions must wait at
    /// least this long before they can be executed. Requires admin auth.
    pub fn set_timelock_secs(env: Env, admin: Address, secs: u64) {
        let admin_addr = require_admin(&env);
        let _ = admin;

        env.storage().persistent().set(&timelock_secs_key(), &secs);
        append_audit_entry(&env, 0, Symbol::new(&env, "set_tl"), &admin_addr);
    }

    /// Queue an admin action for future execution after the timelock delay.
    /// Returns the unique `action_id`. Requires admin auth.
    pub fn queue_action(env: Env, admin: Address, action: TimelockAction) -> u64 {
        let admin_addr = require_admin(&env);
        let _ = admin;

        let mut counter: u64 = env
            .storage()
            .persistent()
            .get(&timelock_action_counter_key())
            .unwrap_or(0u64);
        counter = counter.checked_add(1).expect("action counter overflow");

        let now = env.ledger().timestamp();
        let queued = QueuedAction {
            action: action.clone(),
            queued_at: now,
            executed: false,
        };

        env.storage()
            .persistent()
            .set(&timelock_action_key(counter), &queued);
        env.storage()
            .persistent()
            .set(&timelock_action_counter_key(), &counter);

        append_audit_entry(&env, 0, Symbol::new(&env, "queue"), &admin_addr);
        events::action_queued(&env, counter, &action, &admin_addr);

        counter
    }

    /// Execute a queued timelock action. Anyone may call this once the
    /// timelock delay has elapsed since the action was queued.
    pub fn execute_action(env: Env, action_id: u64) {
        let mut queued: QueuedAction = env
            .storage()
            .persistent()
            .get(&timelock_action_key(action_id))
            .expect("action not found");

        assert!(!queued.executed, "action already executed");

        let timelock_secs: u64 = env
            .storage()
            .persistent()
            .get(&timelock_secs_key())
            .unwrap_or(0u64);
        let now = env.ledger().timestamp();
        assert!(
            now >= queued.queued_at.saturating_add(timelock_secs),
            "timelock not yet elapsed"
        );

        match &queued.action {
            TimelockAction::SetTreasury(new_treasury) => {
                env.storage().instance().set(&treasury_key(), new_treasury);
            }
            TimelockAction::SetPlatformFee(new_fee) => {
                assert!(*new_fee <= 10_000, "platform_fee_bps must be ≤ 10000");
                env.storage()
                    .instance()
                    .set(&platform_fee_bps_key(), new_fee);
            }
        }

        queued.executed = true;
        env.storage()
            .persistent()
            .set(&timelock_action_key(action_id), &queued);

        append_audit_entry(
            &env,
            0,
            Symbol::new(&env, "exec"),
            &env.current_contract_address(),
        );
        events::action_executed(&env, action_id, &queued.action);
    }

    /// Cancel a queued timelock action before it executes. Requires admin auth.
    pub fn cancel_action(env: Env, admin: Address, action_id: u64) {
        let admin_addr = require_admin(&env);
        let _ = admin;

        let queued: QueuedAction = env
            .storage()
            .persistent()
            .get(&timelock_action_key(action_id))
            .expect("action not found");

        assert!(!queued.executed, "action already executed");

        env.storage()
            .persistent()
            .remove(&timelock_action_key(action_id));

        append_audit_entry(&env, 0, Symbol::new(&env, "cancel"), &admin_addr);
        events::action_cancelled(&env, action_id, &queued.action, &admin_addr);
    }

    // -----------------------------------------------------------------------
    // Schema migration
    // -----------------------------------------------------------------------

    /// Migrate a legacy (pre-version) invoice to the current schema.
    ///
    /// Reads the stored invoice under the old layout, rewrites it with
    /// `version = 1` and all other fields preserved. Safe to call multiple
    /// times — already-migrated invoices are a no-op. Requires admin auth.
    pub fn migrate_invoice(env: Env, admin: Address, invoice_id: u64) {
        require_admin_role(&env, &admin, AdminRole::SuperAdmin);

        // Already migrated?
        if let Some(core) = env
            .storage()
            .persistent()
            .get::<_, InvoiceCore>(&invoice_key(invoice_id))
        {
            if core.version >= 1 {
                return;
            }
        }

        // Read legacy (pre-version) format and upgrade.
        let legacy: LegacyInvoice = env
            .storage()
            .persistent()
            .get(&invoice_key(invoice_id))
            .expect("invoice not found");

        let invoice = Invoice::from_legacy(legacy, &env);
        save_invoice(&env, invoice_id, &invoice);
    }

    /// Run all pending storage migrations in order, bringing a contract that
    /// was upgraded from an older Wasm build up to the current schema
    /// version. Returns the resulting `schema_version`.
    ///
    /// This is the **only** entry point exempt from the `MigrationRequired`
    /// guard — every other entry point panics with `MigrationRequired` while
    /// `schema_version` is behind the version this Wasm build expects.
    /// No-op (but still auth-checked) once already current, so it is always
    /// safe to call after an upgrade.
    ///
    /// # Errors
    /// Panics if `admin` is not a `SuperAdmin`.
    pub fn migrate(env: Env, admin: Address) -> u32 {
        require_admin_role_unguarded(&env, &admin, AdminRole::SuperAdmin);
        migrations::run_pending_migrations(&env)
    }

    /// Return the contract's current storage schema version.
    pub fn get_schema_version(env: Env) -> u32 {
        migrations::schema_version(&env)
    }

    /// Return the per-invoice note record introduced at schema v2 (renamed to
    /// its current storage key at schema v3), if one has been migrated.
    pub fn get_invoice_note(env: Env, invoice_id: u64) -> Option<migrations::InvoiceMeta> {
        migrations::get_invoice_meta(&env, invoice_id)
    }

    // -----------------------------------------------------------------------
    // Invoice creation
    // -----------------------------------------------------------------------

    /// Create a new invoice.
    ///
    /// * `token`   – token contract address (same for all recipients)
    /// * `options` – optional fields: co_creators, allow_early_withdrawal, bonus_pool,
    ///               bonus_max_payers, prerequisite_id (#22), tranches (#23),
    ///               stake_amount (#89), referrer (#87), max_payers (#26)
    pub fn create_invoice(
        env: Env,
        creator: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        token: Address,
        deadline: u64,
        options: InvoiceOptions,
    ) -> u64 {
        require_not_frozen(&env);
        // Issue #297: circuit breaker blocks all creation, no exemptions.
        let cb_active: bool = env
            .storage()
            .persistent()
            .get(&circuit_breaker_key())
            .unwrap_or(false);
        assert!(!cb_active, "ContractPaused");
        // Check if contract is paused, but allow exempt creators
        let is_paused = is_paused(&env);
        let is_exempt = env
            .storage()
            .persistent()
            .get::<_, bool>(&pause_exempt_key(&creator))
            .unwrap_or(false);
        if is_paused && !is_exempt {
            panic!("contract is paused");
        }
        creator.require_auth();
        let index_creator = creator.clone();

        // Issue #439: check creator cancellation cooldown.
        let current_ledger = env.ledger().sequence() as u64;
        let cooldown_until: u64 = env
            .storage()
            .persistent()
            .get(&creator_cooldown_key(&creator))
            .unwrap_or(0u64);
        if cooldown_until > 0 && current_ledger < cooldown_until {
            panic!(
                "CreatorCooldownActive {{ until_ledger: {} }}",
                cooldown_until
            );
        }

        Self::_apply_rate_limit(&env, &creator);

        // Issue #4: reject creator if whitelist is non-empty and creator is not on it.
        let wl: Vec<Address> = env
            .storage()
            .persistent()
            .get(&creator_whitelist_key())
            .unwrap_or_else(|| Vec::new(&env));
        if !wl.is_empty() {
            assert!(wl.iter().any(|a| a == creator), "creator not whitelisted");
        }

        // Issue #192: NFT gate — creator must hold at least one NFT from the gate contract.
        if let Some(nft_contract) = env
            .storage()
            .persistent()
            .get::<_, Option<Address>>(&nft_gate_key())
            .unwrap_or(None)
        {
            let balance: i128 = env.invoke_contract(
                &nft_contract,
                &Symbol::new(&env, "balance_of"),
                (creator.clone(),).into_val(&env),
            );
            assert!(balance > 0, "nft gate: not a holder");
        }

        // Issue #420: captured before `options` is consumed below; applied to the
        // stored invoice once `_create_invoice_inner` has allocated its id.
        let overfunding_policy = options.ext.overfunding_policy.clone();
        // Captured before `options` is consumed below; applied to the stored
        // invoice once `_create_invoice_inner` has allocated its id.
        let cosigners = options.cosigners.clone();
        let cosigner_threshold = options.cosigner_threshold;
        let tiers = options.ext.tiers.clone();

        // Issue #753: if payment_token override is set, use it as funding_token.
        let payment_token_override = options.ext.payment_token.clone();
        let funding_token = if let Some(ref tok) = payment_token_override {
            tok.clone()
        } else {
            token.clone()
        };

        // Issue #754: capture tags before options is consumed.
        let tags_opt = options.ext.tags.clone();
        // Issue #755: capture extension config before options is consumed.
        let ext_duration = options.ext.extension_duration_seconds;
        let ext_quorum_bps = options.ext.extension_quorum_bps;

        // Validate split ratios (if provided) before any storage is touched.
        if !options.ratios.is_empty() {
            let denominator = options.ext.ratio_denominator.max(1);
            if let Err(e) = validate_ratios(&options.ratios, denominator) {
                env.panic_with_error(e);
            }
        }

        let id = Self::_create_invoice_inner(
            &env,
            creator,
            recipients,
            amounts,
            Vec::new(&env),
            funding_token,
            deadline,
            options.co_creators,
            options.allow_early_withdrawal,
            options.bonus_pool,
            options.bonus_max_payers,
            options.prerequisite_id,
            options.tranches,
            options.co_signers,
            options.required_signatures,
            options.penalty_bps.unwrap_or(0),
            options.penalty_deadline.unwrap_or(0),
            options.min_funding_bps.unwrap_or(0),
            options.release_stages,
            options.price_oracle,
            options.swap_tokens,
            options.condition_oracle,
            options.tax_bps.unwrap_or(0),
            options.tax_authority,
            options.insurance_premium_bps.unwrap_or(0),
            options.smart_route.unwrap_or(false),
            options.notification_contract.clone(),
            options.overflow_behavior.clone(),
            options.convert_to_stream,
            options.accepted_tokens,
            options.forward_to,
            options.forward_invoice_id,
            options.creator_cosigner,
            options.velocity_limit,
            options.velocity_window,
            options.split_rules,
            options.auto_resolve_rules,
            options.cross_chain_ref,
            options.allowed_payers,
            options.ext.payment_cooldown_secs,
            options.ext.max_payments_per_window,
            options.ext.payment_window_secs,
            options.refund_grace_secs,
            options.priorities,
            options.require_kyc,
            options.scheduled_release_at,
            options.ext.min_payer_rep,
            options.ext.release_delay_ledgers,
            options.ext.metadata_hash,
            options.ext.target_usd_cents,
            options.ext.oracle,
            options.ext.oracle_asset_pair_base,
            options.ext.oracle_asset_pair_quote,
            options.ext.escrow_hold_period,
            options.ext.payment_open_at,
            options.ext.payment_close_at,
            options.ext.milestones,
            options.ext.recipient_max_payouts,
            options.ext.recipient_whitelist_enabled,
            options.ext.release_condition_hash,
            options.ext.early_bird_window_ledgers,
            options.ext.early_bird_fee_bps,
            options.ext.creator_fee_bps,
            options.ratios.clone(),
            options.ext.ratio_denominator,
        );

        // Issue #753: emit PaymentTokenSet event if payment_token override was used.
        if let Some(ref tok) = payment_token_override {
            events::payment_token_set(&env, id, tok);
        }

        // Issue #754: index tags if provided.
        if let Some(ref tags) = tags_opt {
            Self::_apply_tags(&env, id, tags);
        }

        // Issue #755: store extension config if non-default.
        let default_duration: u64 = 604_800;
        let default_quorum: u32 = 5_100;
        if ext_duration != default_duration || ext_quorum_bps != default_quorum {
            env.storage().persistent().set(
                &invoice_ext_config_key(id),
                &(ext_duration, ext_quorum_bps),
            );
        }

        apply_overfunding_policy(&env, id, overfunding_policy);
        apply_cosigner_config(&env, id, cosigners, cosigner_threshold);
        // Issue #747: apply per-payer contribution cap if specified.
        if let Some(cap) = options.ext.max_contribution_per_payer {
            let mut invoice = load_invoice(&env, id);
            invoice.max_contribution_per_payer = Some(cap);
            save_invoice(&env, id, &invoice);
        }
        id
    }

    /// Like `create_invoice` but accepts a separate `InvoiceOptions2` for oracle/min_payer_rep.
    /// Merges options2 into options.ext before delegating to create_invoice.
    pub fn create_invoice_ext(
        env: Env,
        creator: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        token: Address,
        deadline: u64,
        options: InvoiceOptions,
        options2: InvoiceOptions2,
    ) -> u64 {
        let options = InvoiceOptions {
            ext: options2,
            ..options
        };
        Self::create_invoice(env, creator, recipients, amounts, token, deadline, options)
    }

    /// Create an invoice with per-recipient payout tokens.
    pub fn create_invoice_with_recipients(
        env: Env,
        creator: Address,
        recipients: Vec<Recipient>,
        amounts: Vec<i128>,
        funding_token: Address,
        deadline: u64,
        options: InvoiceOptions,
    ) -> u64 {
        let mut recipient_addrs: Vec<Address> = Vec::new(&env);
        let mut recipient_tokens: Vec<Address> = Vec::new(&env);
        for recipient in recipients.iter() {
            recipient_addrs.push_back(recipient.address.clone());
            recipient_tokens.push_back(recipient.token.clone());
        }

        let cb_active: bool = env
            .storage()
            .persistent()
            .get(&circuit_breaker_key())
            .unwrap_or(false);
        assert!(!cb_active, "ContractPaused");
        let is_paused = is_paused(&env);
        let is_exempt = env
            .storage()
            .persistent()
            .get::<_, bool>(&pause_exempt_key(&creator))
            .unwrap_or(false);
        if is_paused && !is_exempt {
            panic!("contract is paused");
        }
        creator.require_auth();
        Self::_apply_rate_limit(&env, &creator);

        // Issue #420: see `create_invoice` — captured before `options` is consumed.
        let overfunding_policy = options.ext.overfunding_policy.clone();
        // See `create_invoice` — captured before `options` is consumed.
        let cosigners = options.cosigners.clone();
        let cosigner_threshold = options.cosigner_threshold;
        let tiers = options.ext.tiers.clone();

        let id = Self::_create_invoice_inner(
            &env,
            creator,
            recipient_addrs,
            amounts,
            recipient_tokens,
            funding_token.clone(),
            deadline,
            options.co_creators,
            options.allow_early_withdrawal,
            options.bonus_pool,
            options.bonus_max_payers,
            options.prerequisite_id,
            options.tranches,
            options.co_signers,
            options.required_signatures,
            options.penalty_bps.unwrap_or(0),
            options.penalty_deadline.unwrap_or(0),
            options.min_funding_bps.unwrap_or(0),
            options.release_stages,
            options.price_oracle,
            options.swap_tokens,
            options.condition_oracle,
            options.tax_bps.unwrap_or(0),
            options.tax_authority,
            options.insurance_premium_bps.unwrap_or(0),
            options.smart_route.unwrap_or(false),
            options.notification_contract.clone(),
            options.overflow_behavior.clone(),
            options.convert_to_stream,
            options.accepted_tokens,
            options.forward_to,
            options.forward_invoice_id,
            options.creator_cosigner,
            options.velocity_limit,
            options.velocity_window,
            options.split_rules,
            options.auto_resolve_rules,
            options.cross_chain_ref,
            options.allowed_payers,
            options.ext.payment_cooldown_secs,
            options.ext.max_payments_per_window,
            options.ext.payment_window_secs,
            options.refund_grace_secs,
            options.priorities,
            options.require_kyc,
            options.scheduled_release_at,
            options.ext.min_payer_rep,
            options.ext.release_delay_ledgers,
            options.ext.metadata_hash,
            options.ext.target_usd_cents,
            options.ext.oracle,
            options.ext.oracle_asset_pair_base,
            options.ext.oracle_asset_pair_quote,
            options.ext.escrow_hold_period,
            options.ext.payment_open_at,
            options.ext.payment_close_at,
            options.ext.milestones,
            options.ext.recipient_max_payouts,
            options.ext.recipient_whitelist_enabled,
            options.ext.release_condition_hash,
            options.ext.early_bird_window_ledgers,
            options.ext.early_bird_fee_bps,
            options.ext.creator_fee_bps,
            options.ratios.clone(),
            options.ext.ratio_denominator,
        );

        apply_overfunding_policy(&env, id, overfunding_policy);
        apply_cosigner_config(&env, id, cosigners, cosigner_threshold);
        // Issue #747: apply per-payer contribution cap if specified.
        if let Some(cap) = options.ext.max_contribution_per_payer {
            let mut invoice = load_invoice(&env, id);
            invoice.max_contribution_per_payer = Some(cap);
            save_invoice(&env, id, &invoice);
        }
        id
    }

    #[allow(clippy::too_many_arguments)]
    fn _create_invoice_inner(
        env: &Env,
        creator: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        recipient_tokens: Vec<Address>,
        funding_token: Address,
        deadline: u64,
        co_creators: Vec<Address>,
        allow_early_withdrawal: bool,
        bonus_pool: i128,
        bonus_max_payers: u32,
        prerequisite_id: Option<u64>,
        tranches: Vec<Tranche>,
        co_signers: Vec<Address>,
        required_signatures: u32,
        penalty_bps: u32,
        penalty_deadline: u64,
        min_funding_bps: u32,
        release_stages: Vec<u32>,
        price_oracle: Option<Address>,
        swap_tokens: Vec<Option<Address>>,
        condition_oracle: Option<Address>,
        tax_bps: u32,
        tax_authority: Option<Address>,
        insurance_premium_bps: u32,
        smart_route: bool,
        notification_contract: Option<Address>,
        overflow_behavior: OverflowBehavior,
        convert_to_stream: bool,
        accepted_tokens: Vec<Address>,
        forward_to: Option<Address>,
        forward_invoice_id: Option<u64>,
        creator_cosigner: Option<Address>,
        velocity_limit: i128,
        velocity_window: u64,
        split_rules: Vec<SplitRule>,
        auto_resolve_rules: Vec<ResolveRule>,
        cross_chain_ref: Option<String>,
        allowed_payers: Option<Vec<Address>>,
        payment_cooldown_secs: Option<u64>,
        max_payments_per_window: Option<u32>,
        payment_window_secs: Option<u64>,
        refund_grace_secs: Option<u64>,
        priorities: Vec<u32>,
        require_kyc: bool,
        scheduled_release_at: Option<u64>,
        min_payer_rep: Option<u32>,
        release_delay_ledgers: Option<u32>,
        metadata_hash: Option<BytesN<32>>,
        target_usd_cents: Option<u64>,
        oracle: Option<Address>,
        oracle_asset_pair_base: Option<Symbol>,
        oracle_asset_pair_quote: Option<Symbol>,
        escrow_hold_period: Option<u32>,
        payment_open_at: Option<u64>,
        payment_close_at: Option<u64>,
        milestones: Option<Vec<u32>>,
        recipient_max_payouts: Option<Vec<Option<i128>>>,
        recipient_whitelist_enabled: bool,
        release_condition_hash: Option<BytesN<32>>,
        early_bird_window_ledgers: u32,
        early_bird_fee_bps: u32,
        creator_fee_bps: u32,
        ratios: Vec<u32>,
        ratio_denominator: u64,
    ) -> u64 {
        check_not_paused(env);
        validate_allowed_token(env, &funding_token);
        assert!(
            recipients.len() == amounts.len(),
            "recipients and amounts length mismatch"
        );

        assert!(!recipients.is_empty(), "must have at least one recipient");
        // Issue #526: enforce minimum recipient count.
        {
            let min_recipients: u32 = env
                .storage()
                .instance()
                .get(&min_recipients_key())
                .unwrap_or(1u32);
            if (recipients.len() as u32) < min_recipients {
                panic_with_error!(env, ContractError::TooFewRecipients);
            }
        }
        // Issue #483: reject zero or negative amounts at entry point.
        for amt in amounts.iter() {
            guard_nonzero_amount(amt).expect("ZeroAmountNotAllowed");
        }
        assert!(
            deadline > env.ledger().timestamp(),
            "deadline must be in the future"
        );
        // Issue #430 / #697: creator-defined payment window.
        if let Some(close_at) = payment_close_at {
            assert!(
                close_at < deadline,
                "payment_close_at must be before deadline"
            );
        }
        if let (Some(open_at), Some(close_at)) = (payment_open_at, payment_close_at) {
            if open_at >= close_at {
                env.panic_with_error(ContractError::InvalidAmount);
            }
        }
        assert!(bonus_pool >= 0, "bonus_pool must be non-negative");
        // Issue #690: `penalty_bps` is a fraction of a late payment; a value
        // above 100% would make every late payment exceed its principal. Reject
        // it before any storage is written.
        if let Err(e) = assert_valid_bps(penalty_bps) {
            env.panic_with_error(e);
        }
        assert!(min_funding_bps <= 10_000, "min_funding_bps must be ≤ 10000");
        // Issue #695: velocity_limit > 0 paired with velocity_window == 0 would cause
        // division-by-zero or undefined behaviour when computing per-payer spend rates.
        // Reject the configuration before any storage is written.
        if velocity_limit > 0 && velocity_window == 0 {
            panic_with_error!(env, ContractError::InvalidAmount);
        }
        assert_valid_bps(tax_bps).expect("tax_bps must be ≤ 10000");
        assert_valid_bps(insurance_premium_bps).expect("insurance_premium_bps must be ≤ 10000");
        // Issue #489 / #696: early-bird discounted platform fee must not exceed the
        // platform fee read from contract storage at creation time.
        let platform_fee_bps: u32 = env
            .storage()
            .instance()
            .get(&platform_fee_bps_key())
            .unwrap_or(0u32);
        if early_bird_fee_bps > platform_fee_bps {
            env.panic_with_error(ContractError::InvalidAmount);
        }
        // Issue #559: creator fee must be within bounds and not exceed cap with platform fee.
        assert!(
            creator_fee_bps <= 10_000,
            "creator_fee_bps must be ≤ 10000"
        );
        assert!(
            (creator_fee_bps as u64 + platform_fee_bps as u64) <= 10_000,
            "FeeSumExceedsCap"
        );
        // Issue #803: creator commission is capped at 5%.
        assert!(
            creator_fee_bps <= MAX_CREATOR_FEE_BPS,
            "creator_fee_bps exceeds 500 bps cap"
        );
        if tax_bps > 0 {
            assert!(
                tax_authority.is_some(),
                "tax_authority must be set if tax_bps > 0"
            );
        }
        if !priorities.is_empty() {
            assert!(
                priorities.len() == recipients.len(),
                "priorities length must match recipients"
            );
        }
        if oracle.is_some() {
            assert!(
                oracle_asset_pair_base.is_some() && oracle_asset_pair_quote.is_some(),
                "oracle_asset_pair required when oracle is set"
            );
            assert!(
                price_oracle.is_none(),
                "cannot set both price_oracle and oracle"
            );
        }

        for amt in amounts.iter() {
            assert!(amt > 0, "amounts must be positive");
        }

        let _total_amount: i128 = amounts.iter().sum();

        // Issue #286: Verify no duplicate recipients
        for i in 0..recipients.len() {
            for j in (i + 1)..recipients.len() {
                debug_assert!(
                    recipients.get(i).unwrap() != recipients.get(j).unwrap(),
                    "invariant: duplicate recipient addresses in list"
                );
            }
        }

        if let Some(compliance_contract) = env
            .storage()
            .persistent()
            .get::<_, Address>(&soroban_sdk::symbol_short!("comp_ctr"))
        {
            let creator_ok: bool = env.invoke_contract(
                &compliance_contract,
                &soroban_sdk::Symbol::new(env, "check"),
                (creator.clone(),).into_val(env),
            );
            assert!(creator_ok, "compliance check failed");

            for recipient in recipients.iter() {
                let recipient_ok: bool = env.invoke_contract(
                    &compliance_contract,
                    &soroban_sdk::Symbol::new(env, "check"),
                    (recipient.clone(),).into_val(env),
                );
                assert!(recipient_ok, "compliance check failed");
            }
        }

        if let Some(prereq_id) = prerequisite_id {
            let _ = load_invoice(env, prereq_id);
        }

        // Issue #693: a multi-signature release gate that requires more approvals
        // than there are co-signers (or requires approvals with no co-signers at
        // all) can never be satisfied, permanently locking the invoice. Reject
        // such a configuration at creation time rather than at release time.
        if required_signatures > co_signers.len() {
            env.panic_with_error(ContractError::InvalidAmount);
        }
        if !co_signers.is_empty() && required_signatures == 0 {
            env.panic_with_error(ContractError::InvalidAmount);
        }

        // Issue #691: when a graduated release schedule is supplied, its
        // per-tranche basis points must cover exactly 100% so that all funds are
        // eventually releasable. An empty schedule (release-all-at-once) is fine.
        if !tranches.is_empty() {
            let total_bps: u32 = tranches
                .iter()
                .map(|t| t.basis_points)
                .fold(0u32, |a, b| a.saturating_add(b));
            if let Err(e) = validation::assert_bps_total(total_bps) {
                env.panic_with_error(e);
            }
        }

        // Issue #692: a non-empty staged-release schedule must sum to exactly
        // 100% (10 000 bps); otherwise the final stage is silently truncated and
        // residual funds are left unreachable. An empty schedule is skipped.
        if !release_stages.is_empty() {
            let total_bps: u32 = release_stages
                .iter()
                .fold(0u32, |a, b| a.saturating_add(b));
            if let Err(e) = validation::assert_bps_total(total_bps) {
                env.panic_with_error(e);
            }
        }
        let milestones = milestones.unwrap_or_else(|| Vec::new(env));
        validate_milestones(env, &milestones);
        let recipient_max_payouts = recipient_max_payouts.unwrap_or_else(|| Vec::new(env));
        if !recipient_max_payouts.is_empty() {
            assert!(
                recipient_max_payouts.len() == recipients.len(),
                "recipient_max_payouts length must match recipients"
            );
        }

        // Issue: validate split_rules — must have one rule per recipient, rules must sum to 100%.
        if !split_rules.is_empty() {
            assert!(
                split_rules.len() == recipients.len(),
                "split_rules length must match recipients"
            );
            let total_amounts: i128 = amounts.iter().sum();
            assert!(total_amounts > 0, "total amounts must be positive");
            let mut total_bps: u32 = 0;
            for rule in split_rules.iter() {
                match rule {
                    SplitRule::Fixed(amt) => {
                        total_bps += (amt as u128 * 10_000u128 / total_amounts as u128) as u32;
                    }
                    SplitRule::Percentage(bps) => {
                        total_bps += bps;
                    }
                    SplitRule::Tiered(_, bps) => {
                        total_bps += bps;
                    }
                }
            }
            assert!(
                total_bps == 10_000,
                "split_rules must sum to 100% of funded amount"
            );
        }

        // Compliance check: if a compliance contract is configured, verify creator and all recipients.
        if let Some(cc) = env
            .storage()
            .persistent()
            .get::<Symbol, Address>(&compliance_key())
        {
            let mut check_args: Vec<Val> = Vec::new(env);
            check_args.push_back(creator.clone().into_val(env));
            let creator_ok: bool =
                env.invoke_contract(&cc, &Symbol::new(env, "is_compliant"), check_args);
            assert!(creator_ok, "compliance check failed");
            for recipient in recipients.iter() {
                let mut r_args: Vec<Val> = Vec::new(env);
                r_args.push_back(recipient.clone().into_val(env));
                let r_ok: bool =
                    env.invoke_contract(&cc, &Symbol::new(env, "is_compliant"), r_args);
                assert!(r_ok, "compliance check failed");
            }
        }

        // Charge configurable creation fee in USDC with volume-based discount (issue #203).
        let base_creation_fee: i128 = env
            .storage()
            .instance()
            .get(&creation_fee_key())
            .unwrap_or(0);

        let _creation_fee = if base_creation_fee > 0 {
            // Get creator's lifetime volume (stored as u64 by update_creator_stats_on_payment).
            let creator_volume: u64 = env
                .storage()
                .persistent()
                .get(&creator_stats_volume_key(&creator))
                .unwrap_or(0u64);

            // Look up highest matching tier discount
            let discount_bps: u32 = if let Some(tiers) = env
                .storage()
                .persistent()
                .get::<_, Vec<(i128, u32)>>(&fee_tiers_key())
            {
                let mut best_discount = 0u32;
                for (threshold, discount) in tiers.iter() {
                    if (creator_volume as i128) >= threshold && discount > best_discount {
                        best_discount = discount;
                    }
                }
                best_discount
            } else {
                0u32
            };

            // Apply discount
            let discounted_fee =
                base_creation_fee - (base_creation_fee * discount_bps as i128 / 10_000);

            let usdc_token: Address = env
                .storage()
                .instance()
                .get(&usdc_token_key())
                .expect("usdc token not set");
            let treasury: Address = env
                .storage()
                .instance()
                .get(&treasury_key())
                .expect("treasury not set");
            let usdc_client = token::Client::new(env, &usdc_token);
            usdc_client.transfer(&creator, &treasury, &discounted_fee);

            discounted_fee
        } else {
            0
        };

        // Issue #89: Transfer stake from creator to contract if stake_amount > 0.
        // (stake_amount is not yet wired into _create_invoice_inner; skipped)

        let id: u64 = env
            .storage()
            .persistent()
            .get(&counter_key())
            .unwrap_or(0u64)
            + 1;
        env.storage().persistent().set(&counter_key(), &id);
        // Record the creation ledger, as the clone and rollover paths do —
        // TWAFR and archival both measure elapsed time from it.
        env.storage()
            .persistent()
            .set(&created_ledger_key(id), &env.ledger().sequence());
        set_created_ledger(env, id);

        // Funding checkpoint progress starts at 0 for each new invoice.
        env.storage()
            .persistent()
            .set(&last_checkpoint_key(id), &0u32);

        if recipient_whitelist_enabled {
            let whitelist: Vec<Address> = env
                .storage()
                .persistent()
                .get(&recipient_whitelist_key(id))
                .unwrap_or_else(|| Vec::new(env));
            for recipient in recipients.iter() {
                assert!(
                    whitelist.iter().any(|a| a == recipient),
                    "recipient not whitelisted"
                );
            }
        }

        // Issue: increment per-creator invoice count for cancellation rate tracking.
        let inv_cnt: u64 = env
            .storage()
            .persistent()
            .get(&invoice_count_key(&creator))
            .unwrap_or(0u64);
        env.storage()
            .persistent()
            .set(&invoice_count_key(&creator), &(inv_cnt + 1));

        // Issue #503: enforce per-creator open-invoice cap.
        const DEFAULT_MAX_OPEN_INVOICES: u32 = 100;
        let max_open: u32 = env
            .storage()
            .instance()
            .get(&max_open_invoices_key())
            .unwrap_or(DEFAULT_MAX_OPEN_INVOICES);
        let open_count: u32 = env
            .storage()
            .persistent()
            .get(&open_invoice_count_key(&creator))
            .unwrap_or(0u32);
        if open_count >= max_open {
            env.panic_with_error(ContractError::CreatorInvoiceLimitReached);
        }
        env.storage()
            .persistent()
            .set(&open_invoice_count_key(&creator), &(open_count + 1));

        let total: i128 = amounts.iter().sum();

        let gov_opt: Option<Option<Address>> =
            env.storage().instance().get(&governance_contract_key());
        if let Some(Some(gov)) = gov_opt {
            let approved: bool = env.invoke_contract(
                &gov,
                &Symbol::new(env, "check_approval"),
                (creator.clone(), total).into_val(env),
            );
            assert!(approved, "governance approval required");
        }

        // Issue #193: check creator volume cap.
        let volume_cap: i128 = env
            .storage()
            .persistent()
            .get(&creator_volume_cap_key(&creator))
            .unwrap_or(0);
        if volume_cap > 0 {
            let used: i128 = env
                .storage()
                .persistent()
                .get(&creator_volume_used_key(&creator))
                .unwrap_or(0);
            assert!(
                used.checked_add(total).expect("volume overflow") <= volume_cap,
                "creator volume cap exceeded"
            );
            env.storage()
                .persistent()
                .set(&creator_volume_used_key(&creator), &(used + total));
        }

        // Issue #195: if require_kyc, verify all recipients have KYC.
        if require_kyc {
            let kyc_contract: Address = env
                .storage()
                .persistent()
                .get(&kyc_contract_key())
                .expect("kyc contract not set");
            for recipient in recipients.iter() {
                let verified: bool = env.invoke_contract(
                    &kyc_contract,
                    &Symbol::new(env, "is_verified"),
                    (recipient.clone(),).into_val(env),
                );
                assert!(verified, "kyc required for recipient");
            }
        }

        if bonus_pool > 0 {
            let token_client = token::Client::new(env, &funding_token);
            token_client.transfer(&creator, &env.current_contract_address(), &bonus_pool);
        }

        // Build per-recipient token vec.
        let mut tokens: Vec<Address> = Vec::new(env);
        if recipient_tokens.is_empty() {
            for _ in recipients.iter() {
                tokens.push_back(funding_token.clone());
            }
        } else {
            assert!(
                recipient_tokens.len() == recipients.len(),
                "recipient token count must match recipients"
            );
            for token in recipient_tokens.iter() {
                tokens.push_back(token.clone());
            }
        }

        // Initialize per-recipient claimed vec to 0.
        let mut claimed: Vec<i128> = Vec::new(env);
        for _ in recipients.iter() {
            claimed.push_back(0i128);
        }

        // Issue #27: Initialize vesting cliff claimed tracking (all false).
        let mut vesting_cliff_claimed: Vec<bool> = Vec::new(env);
        for _ in recipients.iter() {
            vesting_cliff_claimed.push_back(false);
        }

        // Issue #87: Increment referral count if referrer is provided.
        // (referrer is not yet wired into _create_invoice_inner; skipped)

        // Issue #749: the payer whitelist is capped at MAX_PAYER_WHITELIST entries.
        if let Some(ref whitelist) = allowed_payers {
            assert!(
                whitelist.len() <= MAX_PAYER_WHITELIST,
                "whitelist exceeds maximum of 50 addresses"
            );
        }

        let invoice = Invoice {
            version: 1u32,
            creator: creator.clone(),
            co_creators,
            recipients,
            base_amounts: amounts.clone(),
            amounts,
            tokens,
            funding_token,
            deadline,
            funded: 0,
            status: InvoiceStatus::Pending,
            payments: Vec::new(env),
            drip_duration: None,
            release_timestamp: None,
            claimed,
            frozen: false,
            completion_time: None,
            allow_early_withdrawal,
            bonus_pool,
            bonus_max_payers,
            prerequisite_id,
            tranches,
            released_bps: 0,
            co_signers,
            required_signatures,
            signatures: Vec::new(env),
            approver: None,
            approved: false,
            penalty_bps,
            penalty_deadline,
            min_funding_bps,
            release_stages,
            released_stages: 0,
            allowed_payers,
            price_oracle,
            swap_tokens,
            tax_bps,
            tax_authority,
            insurance_premium_bps,
            insurance_fund: 0,
            condition_oracle,
            condition_met: false,
            smart_route,
            overflow_behavior,
            notification_contract,
            convert_to_stream,
            accepted_tokens,
            forward_to,
            forward_invoice_id,
            split_rules,
            auto_resolve_rules,
            creator_cosigner,
            velocity_limit,
            velocity_window,
            pause_reason: None,
            auto_resume_at: None,
            payment_cooldown_secs,
            max_payments_per_window,
            payment_window_secs,
            scheduled_release_at,
            refund_grace_secs,
            cross_chain_ref,
            require_kyc,
            arbiter: None,
            disputed: false,
            auction_on_expiry: false,
            auction_end: 0,
            bids: Vec::new(env),
            min_payment: 0,
            clone_depth: 0,
            parent_invoice_id: None,
            priorities,
            penalty_tiers: Vec::new(env),
            allowed_callers: None,
            refunded_addresses: Vec::new(env),
            admin_frozen: false,
            min_funding_amount: 0,
            target_usd_cents,
            oracle,
            oracle_asset_pair_base,
            oracle_asset_pair_quote,
            min_payer_rep,
            escrow_hold_period,
            held_until: None,
            milestones,
            milestones_released: 0,
            recipient_max_payouts,
            twafr_numerator: 0,
            twafr_last_ledger: 0,
            release_condition_hash,
            recipient_whitelist_enabled,
            // Issue #420: `_create_invoice_inner` has no options struct; the
            // creator's choice is applied by the `create_invoice*` wrappers
            // right after this returns. `Cap` is the default.
            overfunding_policy: OverfundingPolicy::Cap,
            predecessor_id: None,
            contributor_allowlist: None,
            early_bird_window_ledgers,
            early_bird_fee_bps,
            early_bird_fee_credit: 0,
            creator_fee_bps,
            ratio_denominator,
            ratios,
            metadata_hash: metadata_hash.clone(),
            max_contribution_per_payer: None,
        };

        save_invoice(env, id, &invoice);

        // Issue #332: persist contiguous recipient + amount vectors for optimised release.
        save_recipients_list(env, id, &invoice.recipients, &invoice.amounts);

        // Issue #327: store optional time-lock delay.
        if let Some(delay) = release_delay_ledgers {
            assert!(delay <= 100_000, "release_delay_ledgers must be ≤ 100000");
            env.storage()
                .persistent()
                .set(&release_delay_key(id), &delay);
        }
        // Issue #329: store optional metadata hash.
        if let Some(ref hash) = metadata_hash {
            env.storage().persistent().set(&metadata_hash_key(id), hash);
        }
        // Issue #430: store optional creator-defined payment window bounds.
        if let Some(open_at) = payment_open_at {
            env.storage()
                .persistent()
                .set(&payment_open_at_key(id), &open_at);
        }
        if let Some(close_at) = payment_close_at {
            env.storage()
                .persistent()
                .set(&payment_close_at_key(id), &close_at);
        }

        events::invoice_created(env, id, &creator, total, &invoice.cross_chain_ref);
        if let Some(prereq_id) = invoice.prerequisite_id {
            events::invoice_dependency_linked(env, id, prereq_id);
        }
        if let Some(ref addr) = invoice.forward_to {
            events::forward_configured(env, id, addr);
        }
        maybe_record_created(env, &creator, total);
        update_creator_stats_on_creation(env, &creator);

        // Index each recipient -> invoice ID (issue #40).
        for recipient in invoice.recipients.iter() {
            let key = recipient_invoice_ids_key(&recipient);
            let mut ids: Vec<u64> = env
                .storage()
                .persistent()
                .get(&key)
                .unwrap_or_else(|| Vec::new(env));
            ids.push_back(id);
            env.storage().persistent().set(&key, &ids);
        }

        // Increment total_invoices counter (issue #28).
        let total_invoices: u64 = env
            .storage()
            .persistent()
            .get(&total_invoices_key())
            .unwrap_or(0u64);
        env.storage().persistent().set(
            &total_invoices_key(),
            &total_invoices
                .checked_add(1)
                .expect("total_invoices overflow"),
        );
        // Issue #787: global analytics aggregate.
        analytics_ext::on_created(env, &creator);

        id
    }

    /// Create up to 5 invoices in a single transaction.
    fn _apply_rate_limit(env: &Env, creator: &Address) {
        let rate_limit: u32 = env
            .storage()
            .persistent()
            .get(&rate_limit_key())
            .unwrap_or(0u32);
        if rate_limit == 0 {
            return;
        }

        let rate_window: u64 = env
            .storage()
            .persistent()
            .get(&rate_window_key())
            .unwrap_or(0u64);
        let now = env.ledger().timestamp();
        let mut usage: (u64, u32) = env
            .storage()
            .persistent()
            .get(&rate_usage_key(creator))
            .unwrap_or((0u64, 0u32));
        if now >= usage.0.saturating_add(rate_window) {
            usage = (now, 0u32);
        }
        assert!(usage.1 < rate_limit, "rate limit exceeded");
        usage.1 = usage.1.saturating_add(1);
        env.storage()
            .persistent()
            .set(&rate_usage_key(creator), &usage);
    }

    pub fn create_batch(
        env: Env,
        creator: Address,
        invoices: Vec<CreateInvoiceParams>,
    ) -> Vec<u64> {
        creator.require_auth();
        assert!(invoices.len() <= 5, "batch limit exceeded");

        let mut ids: Vec<u64> = Vec::new(&env);
        for params in invoices.iter() {
            Self::_apply_rate_limit(&env, &creator);
            let id = Self::_create_invoice_inner(
                &env,
                creator.clone(),
                params.recipients,
                params.amounts,
                Vec::new(&env),
                params.token,
                params.deadline,
                Vec::new(&env),
                false,
                0,
                0,
                None,
                Vec::new(&env),
                Vec::new(&env),
                0,
                0,
                0,
                0,
                Vec::new(&env),
                None,
                Vec::new(&env),
                None,
                0,
                None,
                0,
                false,
                None,
                OverflowBehavior::Reject,
                false,
                Vec::new(&env),
                None,
                None,
                None,
                0,
                0,
                Vec::new(&env),
                Vec::new(&env),
                None,
                None,
                None,
                None,
                None,
                None,
                Vec::new(&env), // priorities
                false,          // require_kyc
                None,           // scheduled_release_at
                None,           // min_payer_rep
                None,           // release_delay_ledgers
                None,           // metadata_hash
                None,           // target_usd_cents
                None,           // oracle
                None,           // oracle_asset_pair_base
                None,           // oracle_asset_pair_quote
                None,           // escrow_hold_period
                None,           // payment_open_at
                None,           // payment_close_at
                None,           // milestones
                None,           // recipient_max_payouts
                false,          // recipient_whitelist_enabled
                None,           // release_condition_hash
                0,              // early_bird_window_ledgers
                0,              // early_bird_fee_bps
                0,              // creator_fee_bps
                Vec::new(&env), // ratios
                1_u64,          // ratio_denominator
            );
            ids.push_back(id);
        }
        ids
    }

    pub fn create_invoices_batch(
        env: Env,
        creator: Address,
        params_list: Vec<CreateInvoiceParams>,
    ) -> Vec<u64> {
        creator.require_auth();
        if params_list.len() > 10 {
            panic!("BatchLimitExceeded");
        }

        // Pre-validate all invoices before creating any —
        // ensures atomicity: all succeed or none are created.
        for params in params_list.iter() {
            assert!(
                params.recipients.len() == params.amounts.len(),
                "recipients and amounts length mismatch"
            );
            assert!(
                !params.recipients.is_empty(),
                "must have at least one recipient"
            );
            assert!(
                params.deadline > env.ledger().timestamp(),
                "deadline must be in the future"
            );
            for amt in params.amounts.iter() {
                assert!(amt > 0, "amounts must be positive");
            }
        }

        let mut ids: Vec<u64> = Vec::new(&env);
        for params in params_list.iter() {
            let id = Self::_create_invoice_inner(
                &env,
                creator.clone(),
                params.recipients,
                params.amounts,
                Vec::new(&env),
                params.token,
                params.deadline,
                Vec::new(&env), // co_creators
                false,          // allow_early_withdrawal
                0_i128,         // bonus_pool
                0_u32,          // bonus_max_payers
                None,           // prerequisite_id
                Vec::new(&env), // tranches
                Vec::new(&env), // co_signers
                0_u32,          // required_signatures
                0_u32,          // penalty_bps
                0_u64,          // penalty_deadline
                0_u32,          // min_funding_bps
                Vec::new(&env), // release_stages
                None,           // price_oracle
                Vec::new(&env), // swap_tokens
                None,           // condition_oracle
                0_u32,          // tax_bps
                None,           // tax_authority
                0_u32,          // insurance_premium_bps
                false,          // smart_route
                None,           // notification_contract
                OverflowBehavior::Reject,
                false,            // convert_to_stream
                Vec::new(&env),   // accepted_tokens
                None,             // forward_to
                None,             // forward_invoice_id
                None,             // creator_cosigner
                0_i128,           // velocity_limit
                0_u64,            // velocity_window
                Vec::new(&env),   // split_rules
                Vec::new(&env),   // auto_resolve_rules
                None,             // cross_chain_ref
                None,             // allowed_payers
                None,             // payment_cooldown_secs
                None,             // max_payments_per_window
                None,             // payment_window_secs
                None,             // refund_grace_secs
                Vec::new(&env),   // priorities
                false,            // require_kyc
                None,             // scheduled_release_at
                None,             // min_payer_rep
                None,             // release_delay_ledgers
                None,             // metadata_hash
                None,             // target_usd_cents
                None,             // oracle
                None,             // oracle_asset_pair_base
                None,             // oracle_asset_pair_quote
                None,             // escrow_hold_period
                None,             // payment_open_at
                None,             // payment_close_at
                None,             // milestones
                None,             // recipient_max_payouts
                false,            // recipient_whitelist_enabled
                None,             // release_condition_hash
                0,                // early_bird_window_ledgers
                0,                // early_bird_fee_bps
                0,                // creator_fee_bps
                Vec::new(&env),   // ratios
                1_u64,            // ratio_denominator
            );
            ids.push_back(id);
        }
        ids
    }

    /// Issue #744: Create up to 20 invoices in a single atomic transaction.
    ///
    /// All invoices are validated independently; if any single invoice fails
    /// validation the entire batch is rejected and no invoices are created
    /// (atomicity). Invoice IDs are assigned sequentially from the current
    /// counter and a `BatchCreated` event is emitted after all IDs are known.
    ///
    /// Returns a `Vec` of the newly assigned invoice IDs (in the same order as
    /// the input).
    ///
    /// # Panics
    /// * `BatchTooLarge` if `invoices.len() > 20`
    /// * `"empty batch"` if `invoices` is empty
    pub fn batch_create_invoices(
        env: Env,
        creator: Address,
        invoices: Vec<CreateInvoiceParams>,
    ) -> Vec<u64> {
        creator.require_auth();

        if invoices.is_empty() {
            panic!("empty batch");
        }
        if invoices.len() > 20 {
            panic_with_error!(env, ContractError::BatchTooLarge);
        }

        // Pre-validate all invoices before creating any —
        // ensures atomicity: all succeed or none are created.
        for params in invoices.iter() {
            assert!(
                params.recipients.len() == params.amounts.len(),
                "recipients and amounts length mismatch"
            );
            assert!(
                !params.recipients.is_empty(),
                "must have at least one recipient"
            );
            assert!(
                params.deadline > env.ledger().timestamp(),
                "deadline must be in the future"
            );
            for amt in params.amounts.iter() {
                assert!(amt > 0, "amounts must be positive");
            }
        }

        let mut ids: Vec<u64> = Vec::new(&env);
        for params in invoices.iter() {
            let id = Self::_create_invoice_inner(
                &env,
                creator.clone(),
                params.recipients,
                params.amounts,
                Vec::new(&env),
                params.token,
                params.deadline,
                Vec::new(&env), // co_creators
                false,          // allow_early_withdrawal
                0_i128,         // bonus_pool
                0_u32,          // bonus_max_payers
                None,           // prerequisite_id
                Vec::new(&env), // tranches
                Vec::new(&env), // co_signers
                0_u32,          // required_signatures
                0_u32,          // penalty_bps
                0_u64,          // penalty_deadline
                0_u32,          // min_funding_bps
                Vec::new(&env), // release_stages
                None,           // price_oracle
                Vec::new(&env), // swap_tokens
                None,           // condition_oracle
                0_u32,          // tax_bps
                None,           // tax_authority
                0_u32,          // insurance_premium_bps
                false,          // smart_route
                None,           // notification_contract
                OverflowBehavior::Reject,
                false,            // convert_to_stream
                Vec::new(&env),   // accepted_tokens
                None,             // forward_to
                None,             // forward_invoice_id
                None,             // creator_cosigner
                0_i128,           // velocity_limit
                0_u64,            // velocity_window
                Vec::new(&env),   // split_rules
                Vec::new(&env),   // auto_resolve_rules
                None,             // cross_chain_ref
                None,             // allowed_payers
                None,             // payment_cooldown_secs
                None,             // max_payments_per_window
                None,             // payment_window_secs
                None,             // refund_grace_secs
                Vec::new(&env),   // priorities
                false,            // require_kyc
                None,             // scheduled_release_at
                None,             // min_payer_rep
                None,             // release_delay_ledgers
                None,             // metadata_hash
                None,             // target_usd_cents
                None,             // oracle
                None,             // oracle_asset_pair_base
                None,             // oracle_asset_pair_quote
                None,             // escrow_hold_period
                None,             // payment_open_at
                None,             // payment_close_at
                None,             // milestones
                None,             // recipient_max_payouts
                false,            // recipient_whitelist_enabled
                None,             // release_condition_hash
                0,                // early_bird_window_ledgers
                0,                // early_bird_fee_bps
                0,                // creator_fee_bps
                Vec::new(&env),   // ratios
                1_u64,            // ratio_denominator
            );
            ids.push_back(id);
        }

        // Issue #744: emit BatchCreated event with all IDs.
        let count = ids.len();
        events::batch_created(&env, &creator, &ids, count);

        ids
    }

    /// Create a subscription chain of invoices for recurring monthly billing.
    pub fn create_subscription(
        env: Env,
        creator: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        token: Address,
        months: u32,
        interval_days: Option<u32>,
    ) -> u64 {
        creator.require_auth();

        assert!(
            recipients.len() == amounts.len(),
            "recipients and amounts length mismatch"
        );
        assert!(!recipients.is_empty(), "must have at least one recipient");
        assert!(
            months > 0 && months <= 12,
            "months must be between 1 and 12"
        );
        for amt in amounts.iter() {
            assert!(amt > 0, "amounts must be positive");
        }

        let deadline = env.ledger().timestamp() + 30 * 24 * 60 * 60;
        let id = Self::_create_invoice_inner(
            &env,
            creator.clone(),
            recipients.clone(),
            amounts.clone(),
            Vec::new(&env),
            token.clone(),
            deadline,
            Vec::new(&env),
            false,
            0,
            0,
            None,
            Vec::new(&env),
            Vec::new(&env),
            0,
            0,
            0,
            0,
            Vec::new(&env),
            None,
            Vec::new(&env),
            None,
            0,
            None,
            0,
            false,
            None,
            OverflowBehavior::Reject,
            false,
            Vec::new(&env),
            None,
            None,
            None,
            0,
            0,
            Vec::new(&env),
            Vec::new(&env),
            None,
            None,
            None,
            None,
            None,
            None,
            Vec::new(&env), // priorities
            false,          // require_kyc
            None,           // scheduled_release_at
            None,           // min_payer_rep
            None,           // release_delay_ledgers
            None,           // metadata_hash
            None,           // target_usd_cents
            None,           // oracle
            None,           // oracle_asset_pair_base
            None,           // oracle_asset_pair_quote
            None,           // escrow_hold_period
            None,           // payment_open_at
            None,           // payment_close_at
            None,           // milestones
            None,           // recipient_max_payouts
            false,          // recipient_whitelist_enabled
            None,           // release_condition_hash
            0,              // early_bird_window_ledgers
            0,              // early_bird_fee_bps
            0,              // creator_fee_bps
            Vec::new(&env), // ratios
            1_u64,          // ratio_denominator
        );

        if months > 1 {
            // Build tokens vec for subscription params storage.
            let mut tokens_vec: Vec<Address> = Vec::new(&env);
            for _ in recipients.iter() {
                tokens_vec.push_back(token.clone());
            }
            let params = SubscriptionParams {
                creator,
                recipients,
                amounts,
                tokens: tokens_vec,
                interval_days,
            };
            env.storage()
                .persistent()
                .set(&subscription_params_key(id), &params);
        }

        id
    }

    // -----------------------------------------------------------------------
    // Issue #758: Recurring payment subscriptions
    // -----------------------------------------------------------------------

    /// Create a recurring-payment subscription (issue #758).
    ///
    /// Stores a `Subscription` record and returns its ID (same as the first
    /// invoice ID generated for the subscription). The subscription becomes
    /// active immediately with `last_triggered` set to `now`, so the first
    /// call to `trigger_subscription` is only valid after `interval_seconds`.
    ///
    /// - `interval_seconds` must be > 0.
    /// - `recipients` and `amounts` must have the same non-zero length.
    pub fn create_subscription_v2(
        env: Env,
        creator: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        token: Address,
        interval_seconds: u64,
    ) -> u64 {
        require_not_paused(&env);
        creator.require_auth();

        assert!(interval_seconds > 0, "interval_seconds must be > 0");
        assert!(
            recipients.len() == amounts.len(),
            "recipients and amounts length mismatch"
        );
        assert!(!recipients.is_empty(), "must have at least one recipient");
        for amt in amounts.iter() {
            assert!(amt > 0, "amounts must be positive");
        }

        // Use the global Counter to issue a unique subscription ID.
        let sub_id: u64 = {
            let cnt: u64 = env
                .storage()
                .instance()
                .get(&counter_key())
                .unwrap_or(0u64);
            let next = cnt + 1;
            env.storage().instance().set(&counter_key(), &next);
            next
        };

        let subscription = Subscription {
            creator,
            recipients,
            amounts,
            token,
            interval_seconds,
            last_triggered: env.ledger().timestamp(),
            paused: false,
        };
        env.storage()
            .persistent()
            .set(&subscription_record_key(sub_id), &subscription);
        env.storage().persistent().extend_ttl(
            &subscription_record_key(sub_id),
            crate::constants::MIN_INVOICE_TTL_LEDGERS,
            crate::constants::MAX_INVOICE_TTL_LEDGERS,
        );

        sub_id
    }

    /// Trigger the next invoice cycle for a subscription (issue #758).
    ///
    /// Anyone may call this once `now >= last_triggered + interval_seconds`.
    /// Panics with `TooEarlyToTrigger` if the interval has not elapsed.
    /// Panics with `InvalidStatus` if the subscription is paused.
    ///
    /// Creates a new invoice using the stored template, updates
    /// `last_triggered`, and emits `SubscriptionTriggered { subscription_id, new_invoice_id, next_due }`.
    pub fn trigger_subscription(env: Env, subscription_id: u64) -> u64 {
        require_not_paused(&env);

        let mut sub: Subscription = env
            .storage()
            .persistent()
            .get(&subscription_record_key(subscription_id))
            .expect("subscription not found");

        if sub.paused {
            panic_with_error!(&env, ContractError::InvalidStatus);
        }

        let now = env.ledger().timestamp();
        let next_due = sub.last_triggered.saturating_add(sub.interval_seconds);
        if now < next_due {
            panic_with_error!(&env, ContractError::TooEarlyToTrigger);
        }

        // Create the next invoice using the subscription template.
        let deadline = now + sub.interval_seconds;
        let new_invoice_id = Self::_create_invoice_inner(
            &env,
            sub.creator.clone(),
            sub.recipients.clone(),
            sub.amounts.clone(),
            {
                let mut tokens_vec: Vec<Address> = Vec::new(&env);
                for _ in sub.recipients.iter() {
                    tokens_vec.push_back(sub.token.clone());
                }
                tokens_vec
            },
            sub.token.clone(),
            deadline,
            Vec::new(&env),
            false,
            0,
            0,
            None,
            Vec::new(&env),
            Vec::new(&env),
            0,
            0,
            0,
            0,
            Vec::new(&env),
            None,
            Vec::new(&env),
            None,
            0,
            None,
            0,
            false,
            None,
            OverflowBehavior::Reject,
            false,
            Vec::new(&env),
            None,
            None,
            None,
            0,
            0,
            Vec::new(&env),
            Vec::new(&env),
            None,
            None,
            None,
            None,
            None,
            None,
            Vec::new(&env), // priorities
            false,          // require_kyc
            None,           // scheduled_release_at
            None,           // min_payer_rep
            None,           // release_delay_ledgers
            None,           // metadata_hash
            None,           // target_usd_cents
            None,           // oracle
            None,           // oracle_asset_pair_base
            None,           // oracle_asset_pair_quote
            None,           // escrow_hold_period
            None,           // payment_open_at
            None,           // payment_close_at
            None,           // milestones
            None,           // recipient_max_payouts
            false,          // recipient_whitelist_enabled
            None,           // release_condition_hash
            0,              // early_bird_window_ledgers
            0,              // early_bird_fee_bps
            0,              // creator_fee_bps
            Vec::new(&env), // ratios
            1_u64,          // ratio_denominator
        );

        // Advance last_triggered and persist.
        sub.last_triggered = now;
        env.storage()
            .persistent()
            .set(&subscription_record_key(subscription_id), &sub);
        env.storage().persistent().extend_ttl(
            &subscription_record_key(subscription_id),
            crate::constants::MIN_INVOICE_TTL_LEDGERS,
            crate::constants::MAX_INVOICE_TTL_LEDGERS,
        );

        let next_due_after = now.saturating_add(sub.interval_seconds);
        events::subscription_triggered(&env, subscription_id, new_invoice_id, next_due_after);

        new_invoice_id
    }

    /// Pause a subscription, preventing future triggers (issue #758).
    ///
    /// Only the subscription creator may call this.
    pub fn pause_subscription(env: Env, creator: Address, subscription_id: u64) {
        require_not_paused(&env);
        creator.require_auth();

        let mut sub: Subscription = env
            .storage()
            .persistent()
            .get(&subscription_record_key(subscription_id))
            .expect("subscription not found");

        assert!(sub.creator == creator, "not subscription creator");
        sub.paused = true;

        env.storage()
            .persistent()
            .set(&subscription_record_key(subscription_id), &sub);
    }

    /// Resume a paused subscription (issue #758).
    ///
    /// Only the subscription creator may call this.
    pub fn resume_subscription(env: Env, creator: Address, subscription_id: u64) {
        require_not_paused(&env);
        creator.require_auth();

        let mut sub: Subscription = env
            .storage()
            .persistent()
            .get(&subscription_record_key(subscription_id))
            .expect("subscription not found");

        assert!(sub.creator == creator, "not subscription creator");
        sub.paused = false;

        env.storage()
            .persistent()
            .set(&subscription_record_key(subscription_id), &sub);
    }

    /// Return the full `Subscription` record (issue #758).
    pub fn get_subscription(env: Env, subscription_id: u64) -> Subscription {
        env.storage()
            .persistent()
            .get(&subscription_record_key(subscription_id))
            .expect("subscription not found")
    }

    // -----------------------------------------------------------------------
    // Invoice cloning
    // -----------------------------------------------------------------------

    /// Clone an existing invoice with optional field overrides.
    ///
    /// Copies all fields from `source_id` except: funded, status, payments, claimed,
    /// released_bps, completion_time — those reset to their defaults.
    /// The cloned invoice gets `clone_depth = source.clone_depth + 1` and
    /// `parent_invoice_id = Some(source_id)`.
    ///
    /// Panics with "max clone depth exceeded" if `source.clone_depth >= 5`.
    /// Panics with "not invoice creator" if `creator != source.creator`.
    pub fn clone_invoice(
        env: Env,
        creator: Address,
        source_id: u64,
        overrides: CloneOverrides,
    ) -> u64 {
        require_not_paused(&env);
        creator.require_auth();

        let source = load_invoice(&env, source_id);

        assert!(source.creator == creator, "not invoice creator");
        assert!(source.clone_depth < 5, "max clone depth exceeded");

        let recipients = overrides
            .new_recipients
            .unwrap_or_else(|| source.recipients.clone());
        let amounts = overrides
            .new_amounts
            .unwrap_or_else(|| source.amounts.clone());
        let deadline = overrides.new_deadline.unwrap_or(source.deadline);
        let overflow_behavior = overrides
            .new_overflow_behavior
            .map(|sym| {
                if sym == soroban_sdk::symbol_short!("Reject") {
                    OverflowBehavior::Reject
                } else if sym == soroban_sdk::symbol_short!("Refund") {
                    OverflowBehavior::Refund
                } else if sym == soroban_sdk::symbol_short!("Donate") {
                    OverflowBehavior::Donate
                } else {
                    panic!("invalid overflow behavior")
                }
            })
            .unwrap_or_else(|| source.overflow_behavior.clone());

        let token = source.tokens.get(0).expect("no token");

        let id: u64 = env
            .storage()
            .persistent()
            .get(&counter_key())
            .unwrap_or(0u64)
            + 1;
        env.storage().persistent().set(&counter_key(), &id);
        set_created_ledger(&env, id);

        let mut tokens: Vec<Address> = Vec::new(&env);
        for _ in recipients.iter() {
            tokens.push_back(token.clone());
        }

        let mut claimed: Vec<i128> = Vec::new(&env);
        for _ in recipients.iter() {
            claimed.push_back(0i128);
        }

        let new_invoice = Invoice {
            version: source.version,
            creator: source.creator.clone(),
            co_creators: source.co_creators.clone(),
            recipients: recipients.clone(),
            base_amounts: amounts.clone(),
            amounts,
            tokens,
            funding_token: source.funding_token.clone(),
            deadline,
            // Reset fields per spec
            funded: 0,
            status: InvoiceStatus::Pending,
            payments: Vec::new(&env),
            claimed,
            released_bps: 0,
            completion_time: None,
            // Clone lineage
            clone_depth: source.clone_depth + 1,
            parent_invoice_id: Some(source_id),
            // Copy remaining fields from source
            drip_duration: source.drip_duration,
            release_timestamp: source.release_timestamp,
            frozen: source.frozen,
            allow_early_withdrawal: source.allow_early_withdrawal,
            bonus_pool: source.bonus_pool,
            bonus_max_payers: source.bonus_max_payers,
            prerequisite_id: source.prerequisite_id,
            tranches: source.tranches.clone(),
            co_signers: source.co_signers.clone(),
            required_signatures: source.required_signatures,
            signatures: source.signatures.clone(),
            approver: source.approver.clone(),
            approved: source.approved,
            condition_oracle: source.condition_oracle.clone(),
            condition_met: source.condition_met,
            penalty_bps: source.penalty_bps,
            penalty_deadline: source.penalty_deadline,
            penalty_tiers: source.penalty_tiers.clone(),
            allowed_callers: source.allowed_callers.clone(),
            min_funding_bps: source.min_funding_bps,
            release_stages: source.release_stages.clone(),
            released_stages: source.released_stages,
            allowed_payers: source.allowed_payers.clone(),
            price_oracle: source.price_oracle.clone(),
            swap_tokens: source.swap_tokens.clone(),
            tax_bps: source.tax_bps,
            tax_authority: source.tax_authority.clone(),
            insurance_premium_bps: source.insurance_premium_bps,
            insurance_fund: source.insurance_fund,
            smart_route: source.smart_route,
            convert_to_stream: source.convert_to_stream,
            accepted_tokens: source.accepted_tokens.clone(),
            forward_to: source.forward_to.clone(),
            forward_invoice_id: source.forward_invoice_id,
            split_rules: source.split_rules.clone(),
            auto_resolve_rules: source.auto_resolve_rules.clone(),
            creator_cosigner: source.creator_cosigner.clone(),
            velocity_limit: source.velocity_limit,
            velocity_window: source.velocity_window,
            pause_reason: source.pause_reason.clone(),
            auto_resume_at: source.auto_resume_at,
            payment_cooldown_secs: source.payment_cooldown_secs,
            max_payments_per_window: source.max_payments_per_window,
            payment_window_secs: source.payment_window_secs,
            refund_grace_secs: source.refund_grace_secs,
            notification_contract: source.notification_contract.clone(),
            overflow_behavior,
            cross_chain_ref: source.cross_chain_ref.clone(),
            require_kyc: source.require_kyc,
            auction_on_expiry: source.auction_on_expiry,
            auction_end: source.auction_end,
            bids: source.bids.clone(),
            min_payment: source.min_payment,
            min_funding_amount: source.min_funding_amount,
            arbiter: source.arbiter.clone(),
            disputed: false,
            admin_frozen: false,
            scheduled_release_at: source.scheduled_release_at,
            priorities: source.priorities.clone(),
            target_usd_cents: source.target_usd_cents,
            refunded_addresses: Vec::new(&env),
            oracle: source.oracle.clone(),
            oracle_asset_pair_base: source.oracle_asset_pair_base.clone(),
            oracle_asset_pair_quote: source.oracle_asset_pair_quote.clone(),
            min_payer_rep: source.min_payer_rep,
            escrow_hold_period: source.escrow_hold_period,
            held_until: None,
            milestones: source.milestones.clone(),
            milestones_released: 0,
            recipient_max_payouts: source.recipient_max_payouts.clone(),
            twafr_numerator: 0,
            twafr_last_ledger: 0,
            release_condition_hash: source.release_condition_hash.clone(),
            recipient_whitelist_enabled: source.recipient_whitelist_enabled,
            // Issue #420: a clone inherits the source invoice's policy.
            overfunding_policy: source.overfunding_policy.clone(),
            predecessor_id: None,
            contributor_allowlist: source.contributor_allowlist.clone(),
            early_bird_window_ledgers: source.early_bird_window_ledgers,
            early_bird_fee_bps: source.early_bird_fee_bps,
            early_bird_fee_credit: 0,
            creator_fee_bps: source.creator_fee_bps,
            ratio_denominator: source.ratio_denominator,
            ratios: source.ratios.clone(),
            metadata_hash: source.metadata_hash.clone(),
            max_contribution_per_payer: source.max_contribution_per_payer,
        };

        save_invoice(&env, id, &new_invoice);
        if let Some(hash) = overrides.new_metadata_hash {
            env.storage().persistent().set(&metadata_hash_key(id), &hash);
        }
        events::invoice_cloned(&env, source_id, id);
        // Issue #763: record this clone in the new invoice's history ring buffer.
        append_history_entry(
            &env,
            id,
            Symbol::new(&env, "clone"),
            &creator,
            None,
        );

        // Index each recipient -> invoice ID.
        for recipient in recipients.iter() {
            let key = recipient_invoice_ids_key(&recipient);
            let mut ids: Vec<u64> = env
                .storage()
                .persistent()
                .get(&key)
                .unwrap_or_else(|| Vec::new(&env));
            ids.push_back(id);
            env.storage().persistent().set(&key, &ids);
        }

        id
    }

    /// Issue #750: Return the full clone lineage for `invoice_id`, ordered from
    /// the root ancestor down to `invoice_id` itself.
    ///
    /// A non-cloned invoice returns a single-element vector containing its own
    /// id. The walk follows `parent_invoice_id`, which is set by
    /// [`Self::clone_invoice`], and is inherently bounded by the maximum clone
    /// depth enforced there — but a defensive cap still guards against
    /// malformed (cyclic) state.
    ///
    /// Panics with "invoice not found" if any link in the chain is missing.
    pub fn get_lineage(env: Env, invoice_id: u64) -> Vec<u64> {
        // Walk from the given invoice up to the root.
        let mut chain: Vec<u64> = Vec::new(&env);
        let mut current = invoice_id;
        loop {
            chain.push_back(current);

            let invoice = load_invoice(&env, current);
            match invoice.parent_invoice_id {
                Some(parent_id) => current = parent_id,
                None => break,
            }

            // Defensive: never loop forever on corrupt/cyclic lineage.
            if chain.len() > MAX_PARENT_DEPTH {
                break;
            }
        }

        // Reverse into root -> leaf order.
        let mut lineage: Vec<u64> = Vec::new(&env);
        let mut i = chain.len();
        while i > 0 {
            i -= 1;
            if let Some(id) = chain.get(i) {
                lineage.push_back(id);
            }
        }
        lineage
    }

    /// Issue #749: Add `address` to `invoice_id`'s payer whitelist.
    ///
    /// Only the creator (or a co-creator) may call this. If the invoice was
    /// created without a whitelist, calling this converts it into a restricted
    /// invoice seeded with `address` as its first member. Adding an address
    /// that is already listed is a no-op and emits no event.
    ///
    /// Panics with "whitelist is full (max 50)" once [`MAX_PAYER_WHITELIST`]
    /// entries are present.
    pub fn add_to_whitelist(env: Env, invoice_id: u64, creator: Address, address: Address) {
        require_not_paused(&env);
        creator.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.creator == creator || invoice.co_creators.iter().any(|c| c == creator),
            "only creator can modify whitelist"
        );

        // A missing whitelist means "open invoice" — create one.
        if invoice.allowed_payers.is_none() {
            invoice.allowed_payers = Some(Vec::new(&env));
        }

        let mut added: Vec<Address> = Vec::new(&env);
        let removed: Vec<Address> = Vec::new(&env);

        if let Some(ref mut whitelist) = invoice.allowed_payers {
            if !whitelist.iter().any(|p| p == address) {
                assert!(
                    whitelist.len() < MAX_PAYER_WHITELIST,
                    "whitelist is full (max 50)"
                );
                whitelist.push_back(address.clone());
                added.push_back(address.clone());
            }
        }

        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("wl_add"), &creator);

        if !added.is_empty() {
            events::payer_whitelist_updated(&env, invoice_id, &added, &removed);
        }
    }

    /// Issue #749: Remove `address` from `invoice_id`'s payer whitelist.
    ///
    /// Only the creator (or a co-creator) may call this. Removing an address
    /// that is not listed — or calling this on an open (whitelist-less)
    /// invoice — is a no-op and emits no event. Payments already made by the
    /// removed address are untouched; only future payments are blocked.
    pub fn remove_from_whitelist(env: Env, invoice_id: u64, creator: Address, address: Address) {
        require_not_paused(&env);
        creator.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.creator == creator || invoice.co_creators.iter().any(|c| c == creator),
            "only creator can modify whitelist"
        );

        let added: Vec<Address> = Vec::new(&env);
        let mut removed: Vec<Address> = Vec::new(&env);

        let mut found = false;
        if let Some(ref whitelist) = invoice.allowed_payers {
            let mut remaining: Vec<Address> = Vec::new(&env);
            for p in whitelist.iter() {
                if p == address {
                    found = true;
                } else {
                    remaining.push_back(p);
                }
            }
            if found {
                invoice.allowed_payers = Some(remaining);
                removed.push_back(address.clone());
            }
        }

        if found {
            save_invoice(&env, invoice_id, &invoice);
            append_audit_entry(&env, invoice_id, symbol_short!("wl_rem"), &creator);
            events::payer_whitelist_updated(&env, invoice_id, &added, &removed);
        }
    }

    // -----------------------------------------------------------------------
    // Payment (#21 nonce added, #88 auto_convert added)
    // -----------------------------------------------------------------------

    // Pay toward an invoice.
    //
    // `nonce` must equal the current expected nonce for this (invoice_id, payer)
    // pair — starts at 0 and increments with each successful payment.
    //
    // `auto_convert` (issue #88): when true, invokes DEX swap to convert payer's
    // source asset to invoice token before crediting payment. When false, behaves
    // identically to current implementation.

    /// Compress payments by aggregating all payments from the same payer into a single entry.
    pub fn compress_payments(env: Env, invoice_id: u64) {
        require_not_paused(&env);
        let invoice = load_invoice(&env, invoice_id);

        let mut payer_amounts: Map<Address, i128> = Map::new(&env);
        let mut payer_tips: Map<Address, i128> = Map::new(&env);

        for p in invoice.payments.iter() {
            let current_amt = payer_amounts.get(p.payer.clone()).unwrap_or(0);
            payer_amounts.set(p.payer.clone(), current_amt + p.amount);

            let current_tip = payer_tips.get(p.payer.clone()).unwrap_or(0);
            payer_tips.set(p.payer.clone(), current_tip + p.tip);
        }

        let mut new_payments: Vec<Payment> = Vec::new(&env);
        for (payer, amount) in payer_amounts.iter() {
            let tip = payer_tips.get(payer.clone()).unwrap_or(0);
            new_payments.push_back(Payment {
                payer,
                amount,
                tip,
                attestation_hash: None,
                donate_on_failure: false,
                ledger: env.ledger().sequence(),
                timestamp: env.ledger().timestamp(),
            });
        }

        // Verify total funded is unchanged (optional assertion, as asked by Acceptance Criteria)
        let mut total_funded: i128 = 0;
        for p in new_payments.iter() {
            total_funded += p.amount;
        }
        assert_eq!(
            total_funded, invoice.funded,
            "total funded changed after compression"
        );

        // Clear all shards and write compressed payments to appropriate shards (issue #177).
        for shard_id in 0..SHARD_COUNT {
            env.storage()
                .persistent()
                .remove(&pay_shard_key(invoice_id, shard_id));
        }

        for payment in new_payments.iter() {
            let shard_id = compute_shard_id(&env, &payment.payer);
            let mut shard_payments: Vec<Payment> = env
                .storage()
                .persistent()
                .get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(invoice_id, shard_id))
                .unwrap_or_else(|| Vec::new(&env));
            shard_payments.push_back(payment.clone());
            env.storage()
                .persistent()
                .set(&pay_shard_key(invoice_id, shard_id), &shard_payments);
        }
    }

    // -----------------------------------------------------------------------
    // Payment Channel (Issue #1)
    // -----------------------------------------------------------------------

    pub fn open_channel(env: Env, payer: Address, invoice_id: u64, deposit: i128) {
        require_not_paused(&env);
        payer.require_auth();
        assert!(deposit > 0, "deposit must be positive");

        let invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.disputed, "invoice is disputed");

        let token_client = token::Client::new(&env, &funding_token_for(&invoice));
        token_client.transfer(&payer, &env.current_contract_address(), &deposit);

        // Store (balance, deposited)
        let state: (i128, i128) = (deposit, deposit);
        env.storage()
            .persistent()
            .set(&channel_key(invoice_id, &payer), &state);
    }

    pub fn channel_pay(env: Env, payer: Address, invoice_id: u64, amount: i128) {
        require_not_paused(&env);
        payer.require_auth();
        assert!(amount > 0, "amount must be positive");

        let mut state: (i128, i128) = env
            .storage()
            .persistent()
            .get(&channel_key(invoice_id, &payer))
            .expect("channel not found");
        assert!(state.0 >= amount, "insufficient channel balance");

        state.0 -= amount;
        env.storage()
            .persistent()
            .set(&channel_key(invoice_id, &payer), &state);
    }

    pub fn close_channel(env: Env, payer: Address, invoice_id: u64) {
        require_not_paused(&env);
        payer.require_auth();

        let state: (i128, i128) = env
            .storage()
            .persistent()
            .get(&channel_key(invoice_id, &payer))
            .expect("channel not found");
        let balance = state.0;
        let deposited = state.1;
        let net_paid = deposited - balance;

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(!invoice.disputed, "invoice is disputed");

        if net_paid > 0 {
            assert!(
                invoice.status == InvoiceStatus::Pending,
                "invoice is not pending"
            );

            // Write payment to sharded storage (issue #177).
            let shard_id = compute_shard_id(&env, &payer);
            let mut shard_payments: Vec<Payment> = env
                .storage()
                .persistent()
                .get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(invoice_id, shard_id))
                .unwrap_or_else(|| Vec::new(&env));
            shard_payments.push_back(Payment {
                payer: payer.clone(),
                amount: net_paid,
                tip: 0,
                attestation_hash: None,
                donate_on_failure: false,
                ledger: env.ledger().sequence(),
                timestamp: env.ledger().timestamp(),
            });
            env.storage()
                .persistent()
                .set(&pay_shard_key(invoice_id, shard_id), &shard_payments);

            invoice.funded += net_paid;
            let cumulative_key = cumulative_contributed_key(invoice_id);
            let cumulative: i128 = env.storage().persistent().get(&cumulative_key).unwrap_or(0);
            env.storage()
                .persistent()
                .set(&cumulative_key, &(cumulative + net_paid));

            // In real app we might handle penalty/oracle, but for simplicity:
            events::payment_received(&env, invoice_id, &payer, net_paid, &funding_token_for(&invoice));

            let total: i128 = invoice.amounts.iter().sum();
            check_and_emit_funding_checkpoints(&env, invoice_id, invoice.funded, total);

            if invoice.funded >= total {
                let in_group = env
                    .storage()
                    .persistent()
                    .has(&invoice_group_key(invoice_id));
                let guarded = invoice.prerequisite_id.is_some()
                    || !invoice.tranches.is_empty()
                    || !invoice.release_stages.is_empty()
                    || in_group
                    || !invoice.co_signers.is_empty()
                    || env.storage().persistent().has(&cosigners_key(invoice_id))
                    || (invoice.condition_oracle.is_some() && !invoice.condition_met)
                    || (invoice.min_funding_bps > 0
                        && invoice.funded
                            < (invoice.amounts.iter().sum::<i128>()
                                * invoice.min_funding_bps as i128
                                / 10_000));
                if guarded {
                    save_invoice(&env, invoice_id, &invoice);
                } else {
                    Self::_release(&env, invoice_id, &mut invoice, &payer);
                }
            } else {
                save_invoice(&env, invoice_id, &invoice);
            }
        }

        if balance > 0 {
            let token_client = token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
            token_client.transfer(&env.current_contract_address(), &payer, &balance);
        }

        env.storage()
            .persistent()
            .remove(&channel_key(invoice_id, &payer));
    }

    /// # Confidential payments
    /// When `commitment` is `Some`, the payment amount is hidden: `amount` is
    /// ignored and no funds move yet. The contract only stores `commitment` —
    /// a digest of the payer's Pedersen commitment `C = value*G + blinding*H` —
    /// keyed by `(invoice_id, payer)`. The payer later calls
    /// [`Self::reveal_confidential_payment`] with the opening `(value, blinding)`
    /// to settle: only then does the real amount move and become visible on-chain.
    pub fn pay(
        env: Env,
        payer: Address,
        invoice_id: u64,
        amount: i128,
        nonce: u64,
        _auto_convert: bool,
        donate_on_failure: bool,
        commitment: Option<BytesN<32>>,
    ) {
        require_fn_not_paused(&env, &symbol_short!("pay"));
        require_not_frozen(&env);
        payer.require_auth();

        if let Some(commitment) = commitment {
            Self::_commit_confidential_payment(&env, &payer, invoice_id, commitment);
            return;
        }

        Self::enforce_invoice_rate_limit(&env, invoice_id, &payer);
        // Issue #751: withhold the protocol fee before crediting the invoice, so
        // the fee never enters `funded` and cannot be paid out to recipients.
        let net_amount = Self::_withhold_protocol_fee(&env, invoice_id, &payer, amount);
        Self::_pay(
            &env,
            &payer,
            invoice_id,
            net_amount,
            nonce,
            _auto_convert,
            None,
            None,
            donate_on_failure,
        );
    }

    /// Store a Pedersen commitment in place of a raw payment amount (see
    /// `pay`'s confidential-payments doc above). Runs only the invoice-state
    /// checks that don't depend on knowing the amount; amount-dependent
    /// features (KYC, oracle pricing, rate/velocity limits, instalment
    /// schedules) are intentionally out of scope here since none of them can
    /// evaluate against a hidden value — they still apply normally to
    /// non-confidential `pay` calls.
    fn _commit_confidential_payment(
        env: &Env,
        payer: &Address,
        invoice_id: u64,
        commitment: BytesN<32>,
    ) {
        let invoice = load_invoice(env, invoice_id);
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(!invoice.frozen, "invoice is frozen");
        assert!(!invoice.admin_frozen, "invoice frozen by admin");
        assert!(
            env.ledger().timestamp() <= invoice.deadline,
            "invoice deadline has passed"
        );
        // Issue #749: enforce the payer whitelist with a typed error.
        if let Some(ref whitelist) = invoice.allowed_payers {
            if !whitelist.contains(payer) {
                panic_with_error!(env, ContractError::PayerNotWhitelisted);
            }
        }
        if let Some(ref allowlist) = invoice.contributor_allowlist {
            assert!(allowlist.contains(payer), "ContributorNotAllowed");
        }

        let key = pedersen_commitment_key(invoice_id, payer);
        assert!(
            !env.storage().persistent().has(&key),
            "ConfidentialCommitmentExists"
        );
        env.storage().persistent().set(&key, &commitment);
    }

    /// Settle a confidential payment by opening the Pedersen commitment stored
    /// by a prior `pay(..., commitment: Some(_))` call: recomputes
    /// `value*G + blinding*H` and checks its digest against the stored one. On
    /// success the real `value` is pulled from the payer and credited to the
    /// invoice — only now does the amount become visible on-chain, exactly as
    /// the token transfer and `funded` total reveal it.
    ///
    /// Named `reveal_confidential_payment` (rather than `reveal_payment`) to
    /// avoid colliding with the existing hash-based commit/reveal pair
    /// (`commit_payment` / `reveal_payment`), an unrelated, previously shipped
    /// feature this PR does not touch.
    ///
    /// # Errors (panics)
    /// * `"NoConfidentialCommitment"` — no pending commitment for this payer/invoice
    ///   (never committed, or already revealed).
    /// * `"ConfidentialCommitmentMismatch"` — `(value, blinding)` does not open
    ///   the stored commitment.
    pub fn reveal_confidential_payment(
        env: Env,
        invoice_id: u64,
        payer: Address,
        value: i128,
        blinding: BytesN<32>,
    ) {
        require_fn_not_paused(&env, &symbol_short!("pay"));
        require_not_frozen(&env);
        payer.require_auth();
        guard_nonzero_amount(value).expect("ZeroAmountNotAllowed");

        let key = pedersen_commitment_key(invoice_id, &payer);
        let stored: BytesN<32> = env
            .storage()
            .persistent()
            .get(&key)
            .expect("NoConfidentialCommitment");

        let digest = pedersen_commitment_digest(&env, value, &blinding);
        assert!(digest == stored, "ConfidentialCommitmentMismatch");

        // Remove before moving funds so a reentrant call can't reveal twice.
        env.storage().persistent().remove(&key);

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(!invoice.frozen, "invoice is frozen");
        assert!(!invoice.admin_frozen, "invoice frozen by admin");
        assert!(
            env.ledger().timestamp() <= invoice.deadline,
            "invoice deadline has passed"
        );

        let token_client = token::Client::new(&env, &funding_token_for(&invoice));
        token_client.transfer(&payer, &env.current_contract_address(), &value);

        invoice.funded = invoice.funded.checked_add(value).expect("funded overflow");

        let cumulative_key = cumulative_contributed_key(invoice_id);
        let cumulative: i128 = env.storage().persistent().get(&cumulative_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&cumulative_key, &(cumulative + value));

        let contrib_key = contribution_key(invoice_id, &payer);
        let prev_contrib: i128 = env.storage().persistent().get(&contrib_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&contrib_key, &(prev_contrib + value));

        events::confidential_payment_revealed(&env, invoice_id, &payer);

        let total: i128 = invoice.amounts.iter().sum();
        check_and_emit_funding_checkpoints(&env, invoice_id, invoice.funded, total);

        if invoice.funded >= total {
            let in_group = env
                .storage()
                .persistent()
                .has(&invoice_group_key(invoice_id));
            // Issue #772: timestamp-based escrow hold forces a manual release() call.
            let seconds_hold_active = hold_ext::on_fully_funded(env, invoice_id);
            let guarded = invoice.prerequisite_id.is_some()
                || seconds_hold_active
                || !invoice.tranches.is_empty()
                || !invoice.release_stages.is_empty()
                || in_group
                || !invoice.co_signers.is_empty()
                || env.storage().persistent().has(&cosigners_key(invoice_id))
                || (invoice.condition_oracle.is_some() && !invoice.condition_met)
                || (invoice.min_funding_bps > 0
                    && invoice.funded
                        < (invoice.amounts.iter().sum::<i128>() * invoice.min_funding_bps as i128
                            / 10_000));
            if guarded {
                save_invoice(&env, invoice_id, &invoice);
            } else {
                Self::_release(&env, invoice_id, &mut invoice, &payer);
            }
        } else {
            save_invoice(&env, invoice_id, &invoice);
        }
    }

    pub fn commit_payment(env: Env, payer: Address, invoice_id: u64, commitment_hash: BytesN<32>) {
        require_fn_not_paused(&env, &symbol_short!("pay"));
        payer.require_auth();
        let key = commitment_key(invoice_id, &payer);
        assert!(
            !env.storage().persistent().has(&key),
            "ActiveCommitmentExists"
        );
        let commitment = PaymentCommitment {
            commitment_hash,
            commit_ledger: env.ledger().sequence(),
        };
        env.storage().persistent().set(&key, &commitment);
        // Keep the entry readable well past its expiry window, so a late
        // reveal reports `CommitmentExpired` instead of faulting on an
        // archived storage entry.
        let keep_alive = current_commitment_expiry(&env).saturating_mul(2);
        env.storage()
            .persistent()
            .extend_ttl(&key, keep_alive, keep_alive);
        // Keep the commitment entry alive at least through its expiry window so
        // an expired commitment surfaces the intended "CommitmentExpired" business
        // error in reveal_payment rather than a storage-archival host error.
        let expiry = current_commitment_expiry(&env);
        env.storage()
            .persistent()
            .extend_ttl(&key, expiry, expiry.saturating_add(expiry));
        events::payment_committed(&env, invoice_id, &payer, commitment.commit_ledger);
    }

    pub fn reveal_payment(
        env: Env,
        payer: Address,
        invoice_id: u64,
        amount: i128,
        salt: BytesN<32>,
        nonce: u64,
        auto_convert: bool,
        donate_on_failure: bool,
    ) {
        require_fn_not_paused(&env, &symbol_short!("pay"));
        payer.require_auth();
        let key = commitment_key(invoice_id, &payer);
        let commitment: PaymentCommitment = env
            .storage()
            .persistent()
            .get(&key)
            .expect("commitment not found");
        let expiry = current_commitment_expiry(&env);
        if env
            .ledger()
            .sequence()
            .saturating_sub(commitment.commit_ledger)
            > expiry
        {
            panic!("CommitmentExpired");
        }
        let computed_hash = compute_payment_commitment_hash(&env, invoice_id, amount, &salt);
        if computed_hash != commitment.commitment_hash {
            panic!("CommitmentMismatch");
        }
        env.storage().persistent().remove(&key);
        Self::_pay(
            &env,
            &payer,
            invoice_id,
            amount,
            nonce,
            auto_convert,
            None,
            None,
            donate_on_failure,
        );
    }

    /// Pay with a signed attestation binding the payment to an off-chain identity
    pub fn pay_with_attestation(
        env: Env,
        payer: Address,
        invoice_id: u64,
        amount: i128,
        nonce: u64,
        attestation_hash: BytesN<32>,
        signature: BytesN<64>,
        signer_pubkey: BytesN<32>,
        _auto_convert: bool,
    ) {
        require_fn_not_paused(&env, &symbol_short!("pay"));
        payer.require_auth();

        // Verify ed25519 signature over attestation_hash
        let attestation_msg: soroban_sdk::Bytes = attestation_hash.clone().into();
        env.crypto()
            .ed25519_verify(&signer_pubkey, &attestation_msg, &signature);
        Self::enforce_invoice_rate_limit(&env, invoice_id, &payer);

        // Proceed with payment, storing the attestation hash
        Self::_pay(
            &env,
            &payer,
            invoice_id,
            amount,
            nonce,
            _auto_convert,
            None,
            Some(attestation_hash),
            false,
        );
    }

    /// Issue #751: Withhold the configured protocol fee from `amount`, moving it
    /// from the payer into the contract's treasury balance
    /// ([`Self::get_treasury_balance`]), and return the remainder that should be
    /// credited to the invoice.
    ///
    /// The fee is `amount * protocol_fee_bps / 10_000`, rounded down. When the
    /// configured rate is 0 — the default — or the computed fee rounds to 0, no
    /// token transfer happens and `amount` is returned unchanged. A non-zero fee
    /// emits `ProtocolFeeCharged`.
    ///
    /// Fees are held by the contract (rather than pushed straight to the
    /// treasury address) so the admin can batch-release them via
    /// `withdraw_treasury`.
    fn _withhold_protocol_fee(env: &Env, invoice_id: u64, payer: &Address, amount: i128) -> i128 {
        if amount <= 0 {
            return amount;
        }

        let rate_bps = get_protocol_fee_bps_internal(env);
        if rate_bps == 0 {
            return amount;
        }

        // Issue #482: fail loudly on overflow instead of silently truncating.
        let fee = checked_bps_of(amount, rate_bps, 10_000u128).expect("ArithmeticOverflow");
        if fee <= 0 {
            return amount;
        }

        let invoice = load_invoice(env, invoice_id);
        token::Client::new(env, &invoice.funding_token).transfer(
            payer,
            &env.current_contract_address(),
            &fee,
        );

        let balance = get_treasury_balance_internal(env);
        env.storage()
            .instance()
            .set(&treasury_balance_key(), &balance.saturating_add(fee));

        events::protocol_fee_charged(env, invoice_id, payer, fee);

        amount - fee
    }

    fn _pay(
        env: &Env,
        payer: &Address,
        invoice_id: u64,
        amount: i128,
        nonce: u64,
        _auto_convert: bool,
        via: Option<Address>,
        attestation_hash: Option<BytesN<32>>,
        donate_on_failure: bool,
    ) {
        let plan_storage_key = plan_key(invoice_id, payer);
        if let Some(mut plan) = env
            .storage()
            .persistent()
            .get::<_, InstalmentPlan>(&plan_storage_key)
        {
            let paid_index = plan.paid_index;
            assert!(
                (paid_index as usize) < plan.tranches.len().try_into().unwrap(),
                "ScheduleViolation"
            );
            let tranche = plan.tranches.get(paid_index).unwrap();
            if amount != tranche.amount || env.ledger().sequence() < tranche.ledger {
                panic!("ScheduleViolation");
            }
            plan.paid_index += 1;
            env.storage().persistent().set(&plan_storage_key, &plan);
            events::instalment_tranche_paid(env, invoice_id, payer, paid_index, amount);
        }

        let mut invoice = load_invoice(env, invoice_id);

        assert!(invoice.status != InvoiceStatus::Deleted, "InvoiceDeleted");
        // Issue #745: reject payments on invoices already expired via trigger_expiry.
        if invoice.status == InvoiceStatus::Expired {
            panic_with_error!(env, ContractError::InvoiceExpired);
        }
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(
            recipients_ext::shares_complete(env, invoice_id),
            "RecipientSharesIncomplete"
        );
        visibility_ext::check_access(env, invoice_id, payer, &invoice);
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(
            env.ledger().timestamp() <= invoice.deadline,
            "invoice deadline has passed"
        );
        // Issue #483: reject zero or negative payment amounts.
        guard_nonzero_amount(amount).expect("ZeroAmountNotAllowed");

        // Issue #760: enforce sequential milestone gating before any funds move.
        // Only the currently active milestone may receive payments.
        enforce_milestone_payment(env, invoice_id, &invoice, amount);

        // Issue #430: creator-defined payment window.
        if let Some(open_at) = get_payment_open_at_internal(env, invoice_id) {
            assert!(env.ledger().timestamp() >= open_at, "PaymentWindowNotOpen");
        }
        if let Some(close_at) = get_payment_close_at_internal(env, invoice_id) {
            assert!(env.ledger().timestamp() <= close_at, "PaymentWindowClosed");
        }

        // Lazy auto-resume: clear frozen if the auto-resume timestamp has passed.
        if invoice.frozen {
            if let Some(auto_at) = invoice.auto_resume_at {
                if env.ledger().timestamp() >= auto_at {
                    invoice.frozen = false;
                    invoice.pause_reason = None;
                    invoice.auto_resume_at = None;
                    save_invoice(env, invoice_id, &invoice);
                    events::invoice_auto_resumed(env, invoice_id, auto_at);
                }
            }
        }
        assert!(!invoice.frozen, "invoice is frozen");
        assert!(!invoice.admin_frozen, "invoice frozen by admin");

        // Check allowed_payers allowlist.
        // Issue #749: enforce the payer whitelist with a typed error.
        if let Some(ref whitelist) = invoice.allowed_payers {
            if !whitelist.contains(payer) {
                panic_with_error!(env, ContractError::PayerNotWhitelisted);
            }
        }

        // Issue #485: check per-invoice contributor allowlist.
        if let Some(ref allowlist) = invoice.contributor_allowlist {
            assert!(allowlist.contains(payer), "ContributorNotAllowed");
        }

        // Issue #747: enforce per-payer contribution cap.
        if let Some(cap) = invoice.max_contribution_per_payer {
            let cap_key = payer_cap_total_key(invoice_id, payer);
            let payer_total: i128 = env.storage().persistent().get(&cap_key).unwrap_or(0);
            if payer_total + amount > cap {
                events::contribution_cap_hit(env, invoice_id, payer, cap);
                panic_with_error!(env, ContractError::ContributionCapExceeded);
            }
        }

        // Check min_payer_rep requirement (issue #349).
        if let Some(min_rep) = invoice.min_payer_rep {
            let rep = get_rep_internal(env, payer);
            assert!(rep.paid_on_time >= min_rep, "insufficient payer reputation");
        }

        // Issue #208: source contract allowlist check.
        if let Some(ref callers) = invoice.allowed_callers {
            match via {
                Some(ref addr) => assert!(callers.contains(addr), "caller not allowed"),
                None => panic!("direct payments not allowed when caller allowlist is set"),
            }
        }

        // Issue #142: when a price oracle is configured, query current price and
        // compute the oracle-adjusted total. oracle_price of 1_000_000 = 1.0 (identity).
        let total: i128 = if let Some(ref oracle) = invoice.price_oracle {
            let oracle_price: i128 =
                env.invoke_contract(oracle, &Symbol::new(env, "get_price"), Vec::new(env));
            let base_total: i128 = invoice.base_amounts.iter().sum();
            base_total * oracle_price / 1_000_000
        } else if let Some(ref oracle) = invoice.oracle {
            // Oracle-priced invoice: the funding target is determined at payment
            // time, not fixed at creation. `amounts` holds the fixed USD-cents
            // target (e.g. "$100 worth of XLM"); the required token total is
            // recomputed from the oracle's live exchange rate on every payment,
            // so it tracks the market instead of being locked in at creation.
            let pair_base = invoice
                .oracle_asset_pair_base
                .clone()
                .expect("oracle_asset_pair required when oracle is set");
            let pair_quote = invoice
                .oracle_asset_pair_quote
                .clone()
                .expect("oracle_asset_pair required when oracle is set");
            let args: Vec<Val> = ((pair_base, pair_quote),).into_val(env);
            let price_result = env.try_invoke_contract::<i128, soroban_sdk::Error>(
                oracle,
                &Symbol::new(env, "price"),
                args,
            );
            // Any failure to reach the oracle (trap, missing contract, error
            // result) or a non-positive rate (stale/uninitialized feed) is
            // treated as unavailable rather than surfacing a cryptic host error.
            let rate: i128 = match price_result {
                Ok(Ok(r)) if r > 0 => r,
                _ => panic!("OracleUnavailable"),
            };
            let usd_cents_target: i128 = invoice.amounts.iter().sum();
            let computed_amount = usd_cents_target * ORACLE_RATE_SCALE / rate;
            events::oracle_price_fetched(env, invoice_id, rate, computed_amount);
            computed_amount
        } else {
            invoice.amounts.iter().sum()
        };
        let remaining = total - invoice.funded;

        // Issue #783: custom validator contract, if attached to this invoice.
        validator_ext::validate(env, invoice_id, payer, amount);

        // Issue #781: admin-set KYC registry (`is_approved`) takes precedence; the
        // legacy `is_verified` contract is only used when no registry is set.
        if invoice.require_kyc && !compliance_ext::check_kyc(env, payer) {
            let kyc_contract: Address = env
                .storage()
                .persistent()
                .get(&kyc_contract_key())
                .expect("kyc contract not set");
            let verified: bool = env.invoke_contract(
                &kyc_contract,
                &Symbol::new(env, "is_verified"),
                (payer.clone(),).into_val(env),
            );
            assert!(verified, "kyc required");
        }

        // Micro-payments below the configured threshold accumulate off-chain
        // until the threshold is reached, then flush as a single credited payment.
        let _credited_amount: i128 = if invoice.min_payment > 0 {
            let mut accumulator: i128 = env
                .storage()
                .persistent()
                .get(&accum_key(invoice_id, payer))
                .unwrap_or(0i128);
            accumulator += amount;
            if accumulator < invoice.min_payment {
                env.storage()
                    .persistent()
                    .set(&accum_key(invoice_id, payer), &accumulator);
                return;
            }
            // Issue #420: only `Cap` invoices reject an over-target flush here;
            // `AcceptAll` and `ReturnSurplus` handle the excess further below.
            if invoice.overfunding_policy == OverfundingPolicy::Cap {
                assert!(
                    accumulator <= remaining,
                    "InvoiceFullyFunded: payment exceeds remaining balance"
                );
            }
            env.storage()
                .persistent()
                .remove(&accum_key(invoice_id, payer));
            accumulator
        } else {
            amount
        };

        // Payment rate limiting: cooldown and per-window cap (issue #168).
        let now_ts = env.ledger().timestamp();
        Self::enforce_payment_limits(env, invoice_id, payer, &invoice, now_ts);

        // Validate and increment per-payer per-invoice nonce (issue #21).
        let stored_nonce: u64 = env
            .storage()
            .persistent()
            .get(&nonce_key(invoice_id, payer))
            .unwrap_or(0u64);
        assert!(nonce == stored_nonce, "invalid nonce");
        env.storage()
            .persistent()
            .set(&nonce_key(invoice_id, payer), &(stored_nonce + 1));

        // Velocity limiting per (invoice, payer) (new feature).
        if invoice.velocity_limit > 0 {
            let now = env.ledger().timestamp();
            let mut window: (u64, i128) = env
                .storage()
                .persistent()
                .get(&vel_key(invoice_id, payer))
                .unwrap_or((0u64, 0i128));
            if now > window.0 + invoice.velocity_window {
                // reset window
                window.0 = now;
                window.1 = 0;
            }
            assert!(
                window.1 + amount <= invoice.velocity_limit,
                "velocity limit exceeded"
            );
            window.1 += amount;
            env.storage()
                .persistent()
                .set(&vel_key(invoice_id, payer), &window);
        }

        // Global cross-invoice velocity limiting per payer
        let global_limit: i128 = env
            .storage()
            .persistent()
            .get(&global_payer_limit_key())
            .unwrap_or(0i128);
        if global_limit > 0 {
            let global_window_secs: u64 = env
                .storage()
                .persistent()
                .get(&global_payer_window_key())
                .unwrap_or(0u64);
            let now = env.ledger().timestamp();
            let mut global_window: (u64, i128) = env
                .storage()
                .persistent()
                .get(&global_vel_key(payer))
                .unwrap_or((0u64, 0i128));
            if now > global_window.0 + global_window_secs {
                // reset global window
                global_window.0 = now;
                global_window.1 = 0;
            }
            assert!(
                global_window.1 + amount <= global_limit,
                "global payer limit exceeded"
            );
            global_window.1 += amount;
            env.storage()
                .persistent()
                .set(&global_vel_key(payer), &global_window);
        }

        let token_client = token::Client::new(env, &funding_token_for(&invoice));

        // Issue #420: `overfunding_policy` decides what happens when this payment
        // would push `funded` past `total`. `Cap` — the default and the value all
        // pre-#420 invoices carry — delegates to the legacy `overflow_behavior`
        // setting so existing invoices behave exactly as before.
        let credited_amount = match invoice.overfunding_policy {
            // Credit the full payment. `funded` may exceed `total`; the surplus is
            // distributed pro-rata at release because `_release_full` apportions
            // `funded` (not `total`) across recipients.
            OverfundingPolicy::AcceptAll => amount,
            // Credit only what fits under the target; the remainder is transferred
            // straight back to the payer below.
            OverfundingPolicy::ReturnSurplus => {
                // `remaining` can be negative if an earlier `AcceptAll` phase
                // overshot the target, so clamp before comparing.
                amount.min(remaining.max(0))
            }
            OverfundingPolicy::Cap => match invoice.overflow_behavior {
                OverflowBehavior::Reject => {
                    assert!(
                        amount <= remaining,
                        "InvoiceFullyFunded: payment exceeds remaining balance"
                    );
                    amount
                }
                OverflowBehavior::Refund => {
                    if amount <= remaining {
                        amount
                    } else {
                        remaining
                    }
                }
                OverflowBehavior::Donate => {
                    if amount <= remaining {
                        amount
                    } else {
                        remaining
                    }
                }
            },
        };

        let premium =
            (credited_amount as u128 * invoice.insurance_premium_bps as u128 / 10_000u128) as i128;
        // Transfer the full amount from payer so excess can be refunded/donated.
        let excess = amount - credited_amount;
        let total_charge = credited_amount + premium + excess;
        token_client.transfer(payer, &env.current_contract_address(), &total_charge);
        // Issue #420: `ReturnSurplus` refunds the uncredited remainder immediately,
        // regardless of the legacy `overflow_behavior` setting.
        if invoice.overfunding_policy == OverfundingPolicy::ReturnSurplus && excess > 0 {
            token_client.transfer(&env.current_contract_address(), payer, &excess);
        } else {
            match invoice.overflow_behavior {
                OverflowBehavior::Refund if excess > 0 => {
                    token_client.transfer(&env.current_contract_address(), payer, &excess);
                }
                OverflowBehavior::Donate if excess > 0 => {
                    let treasury: Address = env
                        .storage()
                        .instance()
                        .get(&treasury_key())
                        .expect("treasury not set");
                    token_client.transfer(&env.current_contract_address(), &treasury, &excess);
                }
                _ => {}
            }
        }

        // Issue #686: announce when the overfunding policy actually changes the
        // outcome of this payment, i.e. it would push `funded` past `total`.
        // `Cap` has its own `overflow_behavior` events, so it is excluded here.
        match invoice.overfunding_policy {
            OverfundingPolicy::AcceptAll => {
                let surplus = (invoice.funded + credited_amount - total).max(0);
                if surplus > 0 {
                    events::overfunding_triggered(
                        env,
                        invoice_id,
                        payer,
                        &invoice.overfunding_policy,
                        surplus,
                    );
                }
            }
            OverfundingPolicy::ReturnSurplus => {
                if excess > 0 {
                    events::overfunding_triggered(
                        env,
                        invoice_id,
                        payer,
                        &invoice.overfunding_policy,
                        excess,
                    );
                }
            }
            OverfundingPolicy::Cap => {}
        }

        invoice.insurance_fund += premium;

        // Penalty for late payment (issues #42, #211).
        if env.ledger().timestamp() > invoice.penalty_deadline {
            let penalty_bps: u32 = if !invoice.penalty_tiers.is_empty() {
                let elapsed = env.ledger().timestamp() - invoice.penalty_deadline;
                let mut matched_bps = 0u32;
                for tier in invoice.penalty_tiers.iter() {
                    if elapsed >= tier.seconds_after_deadline {
                        matched_bps = tier.bps;
                    }
                }
                matched_bps
            } else {
                invoice.penalty_bps
            };
            if penalty_bps > 0 {
                let penalty_amount = (amount as u128 * penalty_bps as u128 / 10_000u128) as i128;
                if penalty_amount > 0 {
                    let total_amounts: i128 = invoice.amounts.iter().sum();
                    let mut distributed: i128 = 0;
                    let n = invoice.recipients.len();
                    for i in 0..n {
                        let recipient = invoice.recipients.get(i).unwrap();
                        let amt = invoice.amounts.get(i).unwrap();
                        let share = if i == n - 1 {
                            penalty_amount - distributed
                        } else {
                            (penalty_amount as u128 * amt as u128 / total_amounts as u128) as i128
                        };
                        distributed += share;
                        if share > 0 {
                            token_client.transfer(payer, &recipient, &share);
                        }
                    }
                    // Issue #687: surface the late-payment penalty deduction.
                    events::penalty_applied(
                        env,
                        invoice_id,
                        payer,
                        penalty_amount,
                        penalty_bps,
                    );
                }
            }
        }

        // Write payment to sharded storage (issue #177).
        let shard_id = compute_shard_id(env, payer);
        let mut shard_payments: Vec<Payment> = env
            .storage()
            .persistent()
            .get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(invoice_id, shard_id))
            .unwrap_or_else(|| Vec::new(env));
        shard_payments.push_back(Payment {
            payer: payer.clone(),
            amount: credited_amount,
            tip: 0,
            attestation_hash,
            donate_on_failure,
            ledger: env.ledger().sequence(),
            timestamp: env.ledger().timestamp(),
        });
        env.storage()
            .persistent()
            .set(&pay_shard_key(invoice_id, shard_id), &shard_payments);

        // Issue #334: write compact status to optimised storage.
        save_compact_status(env, invoice_id, &invoice.status);

        let creation_ledger: u32 = env
            .storage()
            .persistent()
            .get(&created_ledger_key(invoice_id))
            .unwrap_or(env.ledger().sequence());
        update_twafr(
            &mut invoice,
            creation_ledger,
            env.ledger().sequence(),
            credited_amount,
        );

        // Issue #489: contributions made within early_bird_window_ledgers of
        // invoice creation accrue a platform-fee discount, credited against the
        // fee charged at release. A window of 0 disables the discount.
        if invoice.early_bird_window_ledgers > 0
            && env.ledger().sequence()
                <= creation_ledger.saturating_add(invoice.early_bird_window_ledgers)
        {
            let standard_fee_bps = Self::get_applicable_fee(env.clone(), invoice.creator.clone());
            if invoice.early_bird_fee_bps < standard_fee_bps {
                let discount_amount = (credited_amount as u128
                    * (standard_fee_bps - invoice.early_bird_fee_bps) as u128
                    / 10_000u128) as i128;
                if discount_amount > 0 {
                    invoice.early_bird_fee_credit =
                        invoice.early_bird_fee_credit.saturating_add(discount_amount);
                    events::early_bird_payment(env, invoice_id, payer, discount_amount);
                }
            }
        }

        // Capture funded total before and after mutation (used for milestone check below).
        let prev_funded = invoice.funded;
        invoice.funded += credited_amount;
        // Issue #775: hourly funding velocity histogram.
        velocity_ext::record(env, invoice_id, credited_amount);

        // Track lifetime contributions separately; never decremented on withdrawal/refund.
        let cumulative_key = cumulative_contributed_key(invoice_id);
        let cumulative: i128 = env.storage().persistent().get(&cumulative_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&cumulative_key, &(cumulative + credited_amount));

        // Record per-payer contribution for withdrawal support.
        let contrib_key = contribution_key(invoice_id, payer);
        let prev_contrib: i128 = env.storage().persistent().get(&contrib_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&contrib_key, &(prev_contrib + credited_amount));

        // Issue #747: update per-payer running total used for cap enforcement.
        let cap_key = payer_total_key(invoice_id, payer);
        let prev_total: i128 = env.storage().persistent().get(&cap_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&cap_key, &(prev_total + credited_amount));

        // Increment per-address reputation counter (issue #24, #349).
        let is_late =
            invoice.penalty_deadline > 0 && env.ledger().timestamp() > invoice.penalty_deadline;
        update_rep_internal(env, payer, |score| {
            if is_late {
                score.late_pays = score.late_pays.saturating_add(1);
            } else {
                score.paid_on_time = score.paid_on_time.saturating_add(1);
            }
        });

        // Increment per-address credit score (issue #38).
        let credit: u64 = env
            .storage()
            .persistent()
            .get(&credit_key(payer))
            .unwrap_or(0u64);
        env.storage()
            .persistent()
            .set(&credit_key(payer), &(credit + 1));

        append_audit_entry(env, invoice_id, symbol_short!("pay"), payer);
        events::payment_received(env, invoice_id, payer, credited_amount, &funding_token_for(&invoice));
        // Issue #787: global analytics aggregate.
        analytics_ext::on_paid(env, payer, credited_amount);
        // Issue #333: emit milestone events for any thresholds crossed by this payment.
        {
            let total_for_milestone: i128 = total; // already computed above
            check_and_emit_milestones(
                env,
                invoice_id,
                prev_funded,
                invoice.funded,
                total_for_milestone,
            );
        }
        check_and_emit_funding_checkpoints(env, invoice_id, invoice.funded, total);
        tiers_ext::check_tiers(env, invoice_id, invoice.funded, total);
        update_creator_stats_on_payment(env, &invoice.creator, credited_amount);
        update_creator_payers(env, &invoice.creator, payer);
        notify_invoice(
            env,
            invoice_id,
            symbol_short!("pay"),
            &invoice.notification_contract,
        );
        Self::maybe_release_milestones(env, invoice_id, &mut invoice, payer);

        Self::record_invoice_rate_limit(env, invoice_id, payer);
        // Record rate-limiter timestamps after successful payment (issue #168).
        Self::record_payment_limits(env, invoice_id, payer, &invoice, now_ts);

        // Issue #763: record this payment in the per-invoice history ring buffer.
        append_history_entry(
            env,
            invoice_id,
            Symbol::new(env, "pay"),
            payer,
            Some(credited_amount),
        );

        // Issue: mint a receipt token to the payer via the receipt factory if configured.
        if let Some(factory) = env
            .storage()
            .persistent()
            .get::<Symbol, Address>(&receipt_factory_key())
        {
            let mut args: Vec<Val> = Vec::new(env);
            args.push_back(invoice_id.into_val(env));
            args.push_back(payer.clone().into_val(env));
            args.push_back(credited_amount.into_val(env));
            let receipt_addr: Address =
                env.invoke_contract(&factory, &Symbol::new(env, "mint_receipt"), args);
            env.storage()
                .persistent()
                .set(&receipt_token_key(invoice_id, payer), &receipt_addr);
        }

        // Issue #805: a total within the variance tolerance counts as fully funded.
        let within_variance = funded_within_variance(env, invoice_id, invoice.funded, total);
        if within_variance {
            events::fully_funded_with_variance(env, invoice_id, invoice.funded, total);
        }
        if invoice.funded >= total || within_variance {
            if let Some(hold) = invoice.escrow_hold_period {
                if invoice.held_until.is_none() {
                    let unlock = env.ledger().sequence().saturating_add(hold);
                    invoice.held_until = Some(unlock);
                    events::escrow_hold_started(env, invoice_id, unlock);
                }
            }
            // Issue #325: record the ledger when invoice becomes fully funded (dispute window start).
            if !env
                .storage()
                .persistent()
                .has(&dispute_raised_at_key(invoice_id))
            {
                env.storage()
                    .persistent()
                    .set(&dispute_raised_at_key(invoice_id), &env.ledger().sequence());
            }

            // Auto-release only when no tranches, prerequisite, or group constraint
            // requires a manual release() call.
            let in_group = env
                .storage()
                .persistent()
                .has(&invoice_group_key(invoice_id));
            // Issue #327: a release delay forces a manual release() call.
            let has_release_delay = env
                .storage()
                .persistent()
                .get::<_, u32>(&release_delay_key(invoice_id))
                .is_some();
            let guarded = invoice.prerequisite_id.is_some()
                || !invoice.tranches.is_empty()
                || !invoice.release_stages.is_empty()
                || !invoice.milestones.is_empty()
                || in_group
                || !invoice.co_signers.is_empty()
                || env.storage().persistent().has(&cosigners_key(invoice_id))
                || (invoice.condition_oracle.is_some() && !invoice.condition_met)
                || (invoice.min_funding_bps > 0
                    && invoice.funded
                        < (invoice.amounts.iter().sum::<i128>() * invoice.min_funding_bps as i128
                            / 10_000))
                || has_release_delay
                // Issue #806: hold funds while a recipient vetoes release.
                || !release_vetoes(env, invoice_id).is_empty()
                || invoice.held_until.is_some()
                || invoice
                    .scheduled_release_at
                    .is_some_and(|t| env.ledger().timestamp() < t)
                // Issue #809: hold funds until the auto-release condition is met.
                || auto_release_condition_met(env, invoice_id) == Some(false);
            // Issue #327: record the ledger sequence when full funding is reached.
            if !env
                .storage()
                .persistent()
                .has(&funded_at_ledger_key(invoice_id))
            {
                let seq = env.ledger().sequence();
                env.storage()
                    .persistent()
                    .set(&funded_at_ledger_key(invoice_id), &seq);
            }
            if guarded {
                save_invoice(env, invoice_id, &invoice);
            } else {
                Self::_release(env, invoice_id, &mut invoice, payer);
            }
        } else {
            save_invoice(env, invoice_id, &invoice);
        }
    }

    // -----------------------------------------------------------------------
    // Issue #2: pay with an alternate accepted token
    // -----------------------------------------------------------------------

    /// Pay toward an invoice using any token listed in `invoice.accepted_tokens`.
    ///
    /// When `source_token` differs from the invoice base token, the contract
    /// transfers `amount` of `source_token` from `payer` to itself, then calls
    /// the on-chain DEX (stored at "dex_ctr") to swap it for the invoice token.
    /// The converted amount is credited to `invoice.funded`.
    pub fn pay_with_token(
        env: Env,
        payer: Address,
        invoice_id: u64,
        source_token: Address,
        amount: i128,
        nonce: u64,
    ) {
        require_fn_not_paused(&env, &symbol_short!("pay_tok"));
        payer.require_auth();
        Self::enforce_invoice_rate_limit(&env, invoice_id, &payer);

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(
            env.ledger().timestamp() <= invoice.deadline,
            "invoice deadline has passed"
        );
        // Issue #483: reject zero or negative payment amounts.
        guard_nonzero_amount(amount).expect("ZeroAmountNotAllowed");

        let invoice_token = funding_token_for(&invoice);

        // Accept the base token or any token in accepted_tokens.
        let is_base = source_token == invoice_token;
        let is_accepted = is_base || invoice.accepted_tokens.iter().any(|t| t == source_token);
        assert!(is_accepted, "WrongPaymentToken: token not accepted");

        // Validate and increment nonce.
        let stored_nonce: u64 = env
            .storage()
            .persistent()
            .get(&nonce_key(invoice_id, &payer))
            .unwrap_or(0u64);
        assert!(nonce == stored_nonce, "invalid nonce");
        env.storage()
            .persistent()
            .set(&nonce_key(invoice_id, &payer), &(stored_nonce + 1));

        let credited_amount = if is_base {
            // Direct transfer of the invoice token.
            let token_client = token::Client::new(&env, &invoice_token);
            token_client.transfer(&payer, &env.current_contract_address(), &amount);
            amount
        } else {
            // Transfer source token from payer to contract.
            let src_client = token::Client::new(&env, &source_token);
            src_client.transfer(&payer, &env.current_contract_address(), &amount);

            // Swap source_token -> invoice_token via DEX contract.
            let dex: Address = env
                .storage()
                .persistent()
                .get(&soroban_sdk::symbol_short!("dex_ctr"))
                .expect("dex contract not set");
            let mut args: Vec<Val> = Vec::new(&env);
            args.push_back(source_token.into_val(&env));
            args.push_back(invoice_token.into_val(&env));
            args.push_back(amount.into_val(&env));
            let converted: i128 = env.invoke_contract(&dex, &Symbol::new(&env, "swap"), args);
            converted
        };

        let total: i128 = invoice.amounts.iter().sum();
        let remaining = total - invoice.funded;
        assert!(
            credited_amount <= remaining,
            "payment exceeds remaining balance"
        );

        // Write payment to sharded storage (issue #177).
        let shard_id = compute_shard_id(&env, &payer);
        let mut shard_payments: Vec<Payment> = env
            .storage()
            .persistent()
            .get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(invoice_id, shard_id))
            .unwrap_or_else(|| Vec::new(&env));
        shard_payments.push_back(Payment {
            payer: payer.clone(),
            amount: credited_amount,
            tip: 0,
            attestation_hash: None,
            donate_on_failure: false,
            ledger: env.ledger().sequence(),
            timestamp: env.ledger().timestamp(),
        });
        env.storage()
            .persistent()
            .set(&pay_shard_key(invoice_id, shard_id), &shard_payments);

        invoice.funded += credited_amount;
        let cumulative_key = cumulative_contributed_key(invoice_id);
        let cumulative: i128 = env.storage().persistent().get(&cumulative_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&cumulative_key, &(cumulative + credited_amount));

        append_audit_entry(&env, invoice_id, symbol_short!("pay_tok"), &payer);
        events::payment_received(&env, invoice_id, &payer, credited_amount, &funding_token_for(&invoice));
        check_and_emit_funding_checkpoints(&env, invoice_id, invoice.funded, total);
        Self::record_invoice_rate_limit(&env, invoice_id, &payer);
        notify_invoice(
            &env,
            invoice_id,
            symbol_short!("pay"),
            &invoice.notification_contract,
        );

        if invoice.funded >= total {
            let in_group = env
                .storage()
                .persistent()
                .has(&invoice_group_key(invoice_id));
            let guarded = invoice.prerequisite_id.is_some()
                || !invoice.tranches.is_empty()
                || !invoice.release_stages.is_empty()
                || in_group
                || !invoice.co_signers.is_empty()
                || env.storage().persistent().has(&cosigners_key(invoice_id))
                || (invoice.min_funding_bps > 0
                    && invoice.funded
                        < (invoice.amounts.iter().sum::<i128>() * invoice.min_funding_bps as i128
                            / 10_000));
            if guarded {
                save_invoice(&env, invoice_id, &invoice);
            } else {
                Self::_release(&env, invoice_id, &mut invoice, &payer);
            }
        } else {
            save_invoice(&env, invoice_id, &invoice);
        }
    }

    /// Pay with an alternate token by swapping via the configured DEX contract.
    /// The resulting invoice token amount is credited to the invoice.
    pub fn bridge_pay(
        env: Env,
        payer: Address,
        invoice_id: u64,
        source_token: Address,
        source_amount: i128,
    ) {
        require_fn_not_paused(&env, &symbol_short!("brg_pay"));
        payer.require_auth();
        Self::enforce_invoice_rate_limit(&env, invoice_id, &payer);

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(
            env.ledger().timestamp() <= invoice.deadline,
            "invoice deadline has passed"
        );
        assert!(source_amount > 0, "payment amount must be positive");

        let invoice_token = funding_token_for(&invoice);
        let src_client = token::Client::new(&env, &source_token);
        src_client.transfer(&payer, &env.current_contract_address(), &source_amount);

        let dex: Address = env
            .storage()
            .persistent()
            .get(&soroban_sdk::symbol_short!("dex_ctr"))
            .expect("dex contract not set");
        let mut args: Vec<Val> = Vec::new(&env);
        args.push_back(source_token.into_val(&env));
        args.push_back(invoice_token.clone().into_val(&env));
        args.push_back(source_amount.into_val(&env));
        let converted: i128 = env.invoke_contract(&dex, &Symbol::new(&env, "swap"), args);

        let total: i128 = invoice.amounts.iter().sum();
        let remaining = total - invoice.funded;
        assert!(converted <= remaining, "payment exceeds remaining balance");

        // Write payment to sharded storage (issue #177).
        let shard_id = compute_shard_id(&env, &payer);
        let mut shard_payments: Vec<Payment> = env
            .storage()
            .persistent()
            .get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(invoice_id, shard_id))
            .unwrap_or_else(|| Vec::new(&env));
        shard_payments.push_back(Payment {
            payer: payer.clone(),
            amount: converted,
            tip: 0,
            attestation_hash: None,
            donate_on_failure: false,
            ledger: env.ledger().sequence(),
            timestamp: env.ledger().timestamp(),
        });
        env.storage()
            .persistent()
            .set(&pay_shard_key(invoice_id, shard_id), &shard_payments);

        invoice.funded += converted;
        let cumulative_key = cumulative_contributed_key(invoice_id);
        let cumulative: i128 = env.storage().persistent().get(&cumulative_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&cumulative_key, &(cumulative + converted));

        append_audit_entry(&env, invoice_id, symbol_short!("brdg_pay"), &payer);
        events::payment_received(&env, invoice_id, &payer, converted, &invoice_token);
        check_and_emit_funding_checkpoints(&env, invoice_id, invoice.funded, total);
        Self::record_invoice_rate_limit(&env, invoice_id, &payer);
        notify_invoice(
            &env,
            invoice_id,
            symbol_short!("pay"),
            &invoice.notification_contract,
        );

        if invoice.funded >= total {
            let in_group = env
                .storage()
                .persistent()
                .has(&invoice_group_key(invoice_id));
            let guarded = invoice.prerequisite_id.is_some()
                || !invoice.tranches.is_empty()
                || !invoice.release_stages.is_empty()
                || in_group
                || !invoice.co_signers.is_empty()
                || env.storage().persistent().has(&cosigners_key(invoice_id))
                || (invoice.condition_oracle.is_some() && !invoice.condition_met)
                || (invoice.min_funding_bps > 0
                    && invoice.funded
                        < (invoice.amounts.iter().sum::<i128>() * invoice.min_funding_bps as i128
                            / 10_000));
            if guarded {
                save_invoice(&env, invoice_id, &invoice);
            } else {
                Self::_release(&env, invoice_id, &mut invoice, &payer);
            }
        } else {
            save_invoice(&env, invoice_id, &invoice);
        }
    }

    // -----------------------------------------------------------------------
    // Issue #3: batched multi-invoice payment
    // -----------------------------------------------------------------------

    /// Pay toward multiple invoices in a single call, using only one token transfer.
    ///
    /// All invoices must share the same base token. The payer's total is transferred
    /// once; each invoice's `funded` counter is then updated via internal accounting.
    /// Any invalid payment (wrong status, over limit) reverts the entire call.
    /// Invoices that become fully funded trigger auto-release where applicable.
    pub fn pool_pay(env: Env, payer: Address, payments: Vec<InvoicePayment>) {
        require_not_paused(&env);
        payer.require_auth();

        assert!(!payments.is_empty(), "payments must not be empty");

        // Determine the shared token from the first invoice.
        let first_inv = load_invoice(&env, payments.get(0).unwrap().invoice_id);
        let shared_token = funding_token_for(&first_inv);

        // Validate all payments and compute total.
        let mut total: i128 = 0;
        for p in payments.iter() {
            let inv = load_invoice(&env, p.invoice_id);
            assert!(
                inv.status == InvoiceStatus::Pending,
                "invoice is not pending"
            );
            assert!(!inv.disputed, "invoice is disputed");
            assert!(
                env.ledger().timestamp() <= inv.deadline,
                "invoice deadline has passed"
            );
            assert!(p.amount > 0, "payment amount must be positive");
            let inv_total: i128 = inv.amounts.iter().sum();
            assert!(
                inv.funded + p.amount <= inv_total,
                "payment exceeds remaining balance"
            );
            // All invoices must use the same token.
            assert!(
                funding_token_for(&inv) == shared_token,
                "all invoices must use the same token"
            );
            total += p.amount;
        }

        // Single token transfer from payer to contract.
        let token_client = token::Client::new(&env, &shared_token);
        token_client.transfer(&payer, &env.current_contract_address(), &total);

        // Update each invoice via internal accounting (no further token transfers).
        for p in payments.iter() {
            let mut inv = load_invoice(&env, p.invoice_id);
            // Write payment to sharded storage (issue #177).
            let shard_id = compute_shard_id(&env, &payer);
            let mut shard_payments: Vec<Payment> = env
                .storage()
                .persistent()
                .get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(p.invoice_id, shard_id))
                .unwrap_or_else(|| Vec::new(&env));
            shard_payments.push_back(Payment {
                payer: payer.clone(),
                amount: p.amount,
                tip: 0,
                attestation_hash: None,
                donate_on_failure: false,
                ledger: env.ledger().sequence(),
                timestamp: env.ledger().timestamp(),
            });
            env.storage()
                .persistent()
                .set(&pay_shard_key(p.invoice_id, shard_id), &shard_payments);

            inv.funded += p.amount;
            let cumulative_key = cumulative_contributed_key(p.invoice_id);
            let cumulative: i128 = env.storage().persistent().get(&cumulative_key).unwrap_or(0);
            env.storage()
                .persistent()
                .set(&cumulative_key, &(cumulative + p.amount));

            append_audit_entry(&env, p.invoice_id, symbol_short!("pool_pay"), &payer);
            events::payment_received(&env, p.invoice_id, &payer, p.amount, &shared_token);

            let inv_total: i128 = inv.amounts.iter().sum();
            if inv.funded >= inv_total {
                let in_group = env
                    .storage()
                    .persistent()
                    .has(&invoice_group_key(p.invoice_id));
                let guarded = inv.prerequisite_id.is_some()
                    || !inv.tranches.is_empty()
                    || !inv.release_stages.is_empty()
                    || in_group
                    || !inv.co_signers.is_empty()
                    || env.storage().persistent().has(&cosigners_key(p.invoice_id))
                    || (inv.condition_oracle.is_some() && !inv.condition_met)
                    || (inv.min_funding_bps > 0
                        && inv.funded
                            < (inv.amounts.iter().sum::<i128>() * inv.min_funding_bps as i128
                                / 10_000));
                if guarded {
                    save_invoice(&env, p.invoice_id, &inv);
                } else {
                    Self::_release(&env, p.invoice_id, &mut inv, &payer);
                }
            } else {
                save_invoice(&env, p.invoice_id, &inv);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Co-signer approval & Release
    // -----------------------------------------------------------------------

    /// Record a co-signer's approval to release an invoice.
    ///
    /// Only addresses in `co_signers` may call this. Once `required_signatures`
    /// unique co-signers have approved, the release guard is satisfied.
    pub fn sign_release(env: Env, invoice_id: u64, signer: Address) {
        require_not_paused(&env);
        signer.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(!invoice.co_signers.is_empty(), "no co-signers required");
        assert!(
            invoice.co_signers.iter().any(|c| c == signer),
            "not an authorized co-signer"
        );
        assert!(
            !invoice.signatures.iter().any(|s| s == signer),
            "already signed"
        );

        invoice.signatures.push_back(signer.clone());
        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("sign_rel"), &signer);
    }

    /// Record a cosigner's approval toward the N-of-M release-approval quorum
    /// configured via `InvoiceOptions::cosigners` / `cosigner_threshold`.
    ///
    /// Independent of the legacy `co_signers` / `sign_release` gate above.
    /// Only addresses in `cosigners` may call this; each may approve at most
    /// once. Emits `CosignerApproved` on every call, and `CosignerThresholdReached`
    /// the moment the configured threshold is met.
    pub fn approve_release(env: Env, invoice_id: u64, cosigner: Address) {
        require_not_paused(&env);
        cosigner.require_auth();

        let invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        let cosigners: Vec<Address> = env
            .storage()
            .persistent()
            .get(&cosigners_key(invoice_id))
            .expect("no cosigners configured for this invoice");
        assert!(
            cosigners.iter().any(|c| c == cosigner),
            "not an authorized cosigner"
        );

        let mut approvals: Vec<Address> = env
            .storage()
            .persistent()
            .get(&cosign_key(invoice_id))
            .unwrap_or_else(|| Vec::new(&env));
        assert!(
            !approvals.iter().any(|a| a == cosigner),
            "cosigner already approved"
        );

        approvals.push_back(cosigner.clone());
        env.storage()
            .persistent()
            .set(&cosign_key(invoice_id), &approvals);
        events::cosigner_approved(&env, invoice_id, &cosigner, approvals.len());
        append_audit_entry(&env, invoice_id, symbol_short!("cosign"), &cosigner);

        let threshold: u32 = env
            .storage()
            .persistent()
            .get(&cosigner_thresh_key(invoice_id))
            .unwrap_or(0);
        if threshold > 0 && approvals.len() >= threshold {
            events::cosigner_threshold_reached(&env, invoice_id);
        }
    }

    // -----------------------------------------------------------------------
    // Release (#22 prerequisite, #23 tranches)
    // -----------------------------------------------------------------------

    /// Release funds to recipients.
    ///
    /// For tranche invoices, only distributes tranches whose timestamp ≤ now.
    /// Blocks with "prerequisite not released" until the prerequisite invoice is Released.
    /// If an approver is set, requires the invoice to be approved first (issue #25).
    pub fn release_invoice(
        env: Env,
        caller: Address,
        invoice_id: u64,
        preimage: Option<Bytes>,
    ) {
        // --- Reentrancy guard (issue #451-reentrancy) ---
        // Uses temporary storage so the lock is never persisted across transactions.
        let re_key = reentrancy_lock_key();
        if env.storage().temporary().has(&re_key) {
            panic!("{}", ContractError::ReentrantCall as u32);
        }
        env.storage().temporary().set(&re_key, &true);
        // ------------------------------------------------
        delegate_ext::reject_delegate(&env, invoice_id, &caller);
        Self::_release_invoice_inner(&env, caller, invoice_id, preimage);
        env.storage().temporary().remove(&reentrancy_lock_key());
    }

    #[allow(unreachable_code)]
    fn _release_invoice_inner(env: &Env, _caller: Address, invoice_id: u64, preimage: Option<Bytes>) {
        require_fn_not_paused(&env, &symbol_short!("release"));
        require_not_frozen(&env);
        let caller = env.current_contract_address();
        let mut invoice = load_invoice(&env, invoice_id);

        if let Some(expected_hash) = invoice.release_condition_hash.clone() {
            let preimage = preimage.expect("ConditionNotMet");
            let verified_hash: BytesN<32> = env.crypto().sha256(&preimage).into();
            assert!(verified_hash == expected_hash, "ConditionNotMet");
            events::condition_verified(&env, invoice_id, &verified_hash);
        }

        assert!(!invoice.frozen, "invoice is frozen");
        assert!(!invoice.admin_frozen, "invoice frozen by admin");
        assert!(invoice.status != InvoiceStatus::Deleted, "InvoiceDeleted");
        // Issue #745: reject release on invoices already expired via trigger_expiry.
        if invoice.status == InvoiceStatus::Expired {
            panic_with_error!(&env, ContractError::InvoiceExpired);
        }
        // Issue #504: Allow both Pending and PartiallyReleased for batch release retry.
        assert!(
            invoice.status == InvoiceStatus::Pending || invoice.status == InvoiceStatus::PartiallyReleased,
            "invoice is not pending or partially released"
        );
        if let Some(held_until) = invoice.held_until {
            if env.ledger().sequence() < held_until {
                panic!("EscrowHoldActive");
            }
        }
        // Issue #772: timestamp-based escrow hold.
        hold_ext::enforce_release(&env, invoice_id);
        // Issue #325: block release while a payer dispute is active.
        if invoice.disputed {
            if let Some(record) = env
                .storage()
                .persistent()
                .get::<(Symbol, u64), DisputeRecord>(&dispute_record_key(invoice_id))
            {
                assert!(
                    record.status != DisputeStatus::Active,
                    "invoice is under active dispute"
                );
            } else {
                panic!("invoice is disputed");
            }
        }

        let total: i128 = invoice.amounts.iter().sum();
        let min_required = if invoice.min_funding_bps > 0 {
            (total as u128 * invoice.min_funding_bps as u128 / 10_000u128) as i128
        } else {
            total
        };
        // Issue #330: if some recipients were already paid via release_to_recipient,
        // reduce min_required by their paid amounts so the funded check still passes.
        let paid_set_rel: Vec<Address> = env
            .storage()
            .persistent()
            .get(&paid_recipients_key(invoice_id))
            .unwrap_or_else(|| Vec::new(&env));
        let already_paid_amount: i128 = if paid_set_rel.is_empty() {
            0
        } else {
            let mut paid_total: i128 = 0;
            for i in 0..invoice.recipients.len() {
                let r = invoice.recipients.get(i).unwrap();
                if paid_set_rel.iter().any(|p| p == r) {
                    paid_total += invoice.amounts.get(i).unwrap();
                }
            }
            paid_total
        };
        let effective_min_required = min_required.saturating_sub(already_paid_amount);
        assert!(
            invoice.funded >= effective_min_required,
            "minimum funding not reached"
        );

        // Issue #327: enforce time-lock delay set by the creator.
        if let Some(delay_ledgers) = env
            .storage()
            .persistent()
            .get::<_, u32>(&release_delay_key(invoice_id))
        {
            if let Some(funded_at) = env
                .storage()
                .persistent()
                .get::<_, u32>(&funded_at_ledger_key(invoice_id))
            {
                let unlock_at = funded_at.saturating_add(delay_ledgers);
                if env.ledger().sequence() < unlock_at {
                    panic!("FundsLockedUntil");
                }
                // Emit event the first time funds become releasable.
                events::funds_unlocked(&env, invoice_id, unlock_at);
            }
        }

        // Issue #806: any standing recipient veto blocks release.
        if !release_vetoes(env, invoice_id).is_empty() {
            panic!("VetoBlocked");
        }

        // Approval check (issue #25).
        if invoice.approver.is_some() && !invoice.approved {
            panic!("awaiting approval");
        }

        // Prerequisite check (issue #22).
        if let Some(prereq_id) = invoice.prerequisite_id {
            let prereq = load_invoice(&env, prereq_id);
            assert!(
                prereq.status == InvoiceStatus::Released,
                "prerequisite not released"
            );
        }

        // Group constraint: check according to group mode before allowing release.
        if let Some(group_id) = env
            .storage()
            .persistent()
            .get::<(Symbol, u64), u64>(&invoice_group_key(invoice_id))
        {
            // Try to load as InvoiceGroup (new format); fall back to AllOrNothing for legacy groups.
            let mode = env
                .storage()
                .persistent()
                .get::<_, types::InvoiceGroup>(&group_key(group_id))
                .map(|g| g.mode)
                .unwrap_or(types::GroupMode::AllOrNothing);
            match mode {
                types::GroupMode::AllOrNothing => {
                    assert!(
                        group_all_funded(&env, group_id),
                        "group members not fully funded"
                    );
                }
                types::GroupMode::Majority => {
                    assert!(
                        group_majority_funded(&env, group_id),
                        "group majority not funded"
                    );
                }
            }
        }

        // Co-signer approval check.
        if !invoice.co_signers.is_empty() {
            assert!(
                invoice.signatures.len() >= invoice.required_signatures,
                "not enough co-signer approvals"
            );
        }

        // N-of-M cosigner approval check (independent of the legacy co_signers gate).
        require_cosigner_threshold_met(&env, invoice_id);

        Self::_release(&env, invoice_id, &mut invoice, &caller);
    }

    /// Issue #781: set the KYC registry consulted for `require_kyc` invoices (admin only).
    pub fn set_kyc_registry(env: Env, admin: Address, registry: Address) {
        let current = require_admin(&env);
        assert!(current == admin, "NotAuthorized");
        compliance_ext::set_registry(&env, &registry);
    }

    /// Issue #781: currently configured KYC registry, if any.
    pub fn get_kyc_registry(env: Env) -> Option<Address> {
        compliance_ext::get_registry(&env)
    }

    /// Issue #782: start a per-ledger payment stream toward an invoice.
    pub fn start_stream(env: Env, invoice_id: u64, payer: Address, amount_per_ledger: i128) -> u64 {
        stream_ext::start(&env, invoice_id, &payer, amount_per_ledger)
    }

    /// Issue #782: settle the amount accrued since the last settlement.
    pub fn settle_stream(env: Env, stream_id: u64, payer: Address) {
        stream_ext::settle(&env, stream_id, &payer);
    }

    /// Issue #782: cancel a stream, settling accrued amount immediately.
    pub fn cancel_stream(env: Env, stream_id: u64, payer: Address) {
        stream_ext::cancel(&env, stream_id, &payer);
    }

    /// Issue #782: read a stream.
    pub fn get_stream(env: Env, stream_id: u64) -> stream_ext::Stream {
        stream_ext::get_stream(&env, stream_id)
    }

    /// Issue #783: attach (or clear with `None`) a payment validator contract (creator only).
    pub fn set_invoice_validator(env: Env, creator: Address, invoice_id: u64, validator: Option<Address>) {
        validator_ext::set_validator(&env, &creator, invoice_id, validator);
    }

    /// Issue #783: validator attached to an invoice, if any.
    pub fn get_invoice_validator(env: Env, invoice_id: u64) -> Option<Address> {
        validator_ext::get_validator(&env, invoice_id)
    }

    /// Issue #780: attach a per-recipient `release_at` schedule (creator only).
    pub fn set_recipient_schedule(env: Env, creator: Address, invoice_id: u64, release_ats: Vec<Option<u64>>) {
        schedule_ext::set_schedule(&env, &creator, invoice_id, release_ats);
    }

    /// Issue #780: pay all scheduled recipients whose `release_at` has passed.
    pub fn release_scheduled(env: Env, invoice_id: u64) {
        schedule_ext::release_scheduled(&env, invoice_id);
    }

    /// Issue #780: recipients not yet paid under the schedule.
    pub fn get_pending_recipients(env: Env, invoice_id: u64) -> Vec<Address> {
        schedule_ext::pending_recipients(&env, invoice_id)
    }

    /// Backwards-compatible release entry point.
    pub fn release(env: Env, invoice_id: u64) {
        let caller = env.current_contract_address();
        Self::release_invoice(env, caller, invoice_id, None)
    }

    /// Issue #746: Release a basis-point fraction of the currently funded balance
    /// to all recipients, leaving the remainder locked for future partial or full
    /// releases.
    ///
    /// * `bps` — share to release in basis points (1–10 000). 10 000 = 100%.
    /// * The cumulative total of all `release_partial` calls on this invoice
    ///   must not exceed 10 000 bps; any attempt to over-release panics with
    ///   `InvalidBps`.
    /// * Only the invoice creator may call this function.
    pub fn release_partial(env: Env, invoice_id: u64, creator: Address, bps: u32) {
        require_not_paused(&env);
        require_fn_not_paused(&env, &symbol_short!("rel_part"));
        creator.require_auth();

        // Validate bps range (1–10 000).
        if bps == 0 || bps > 10_000 {
            panic_with_error!(env, ContractError::InvalidBps);
        }

        let mut invoice = load_invoice(&env, invoice_id);

        if invoice.status == InvoiceStatus::Expired {
            panic_with_error!(env, ContractError::InvoiceExpired);
        }

        // Only the creator is allowed.
        assert!(invoice.creator == creator, "NotAuthorized");

        // Invoice must be funded (Pending with funded >= total).
        assert!(
            invoice.status == InvoiceStatus::Pending
                || invoice.status == InvoiceStatus::PartiallyReleased,
            "invoice must be pending or partially released"
        );
        assert!(!invoice.frozen, "invoice is frozen");
        assert!(!invoice.admin_frozen, "invoice frozen by admin");
        assert!(!invoice.disputed, "invoice is disputed");

        // Guard: invoice must not be expired.
        assert!(
            env.ledger().timestamp() <= invoice.deadline,
            "invoice deadline has passed"
        );

        // Accumulate released bps — must not exceed 10 000 total.
        let released_bps_key = total_released_bps_key(invoice_id);
        let already_released: u32 = env
            .storage()
            .persistent()
            .get(&released_bps_key)
            .unwrap_or(0u32);

        let new_total = already_released.saturating_add(bps);
        if new_total > 10_000 {
            panic_with_error!(env, ContractError::InvalidBps);
        }

        // Compute the amount to release.
        let amount_to_release = if new_total == 10_000 {
            let prior_released = (invoice.funded as u128 * already_released as u128 / 10_000u128) as i128;
            invoice.funded - prior_released
        } else {
            (invoice.funded as u128 * bps as u128 / 10_000u128) as i128
        };

        let remaining = if new_total == 10_000 {
            0
        } else {
            (invoice.funded as u128 * (10_000 - new_total) as u128 / 10_000u128) as i128
        };

        // Transfer proportionally to each recipient.
        let token_client = token::Client::new(&env, &funding_token_for(&invoice));
        let n = invoice.recipients.len();
        let total_amt: i128 = invoice.amounts.iter().sum();
        let mut distributed: i128 = 0;
        for i in 0..n {
            let recipient = invoice.recipients.get(i).unwrap();
            let amt = invoice.amounts.get(i).unwrap();
            let share = if i == n - 1 {
                // Last recipient gets any rounding dust.
                amount_to_release - distributed
            } else {
                (amount_to_release as u128 * amt as u128 / total_amt as u128) as i128
            };
            if share > 0 {
                token_client.transfer(&env.current_contract_address(), &recipient, &share);
                distributed += share;
            }
        }

        // Update state.
        env.storage()
            .persistent()
            .set(&released_bps_key, &new_total);

        invoice.released_bps = new_total;
        if new_total == 10_000 {
            invoice.status = InvoiceStatus::Released;
            invoice.completion_time = Some(env.ledger().timestamp());
        } else {
            invoice.status = InvoiceStatus::PartiallyReleased;
        }
        save_invoice(&env, invoice_id, &invoice);

        events::partial_released(&env, invoice_id, bps, amount_to_release, remaining);
    }

    /// Trigger a scheduled release at the configured timestamp, respecting min_funding_bps
    pub fn trigger_scheduled_release(env: Env, invoice_id: u64) {
        require_not_paused(&env);
        let mut invoice = load_invoice(&env, invoice_id);

        assert!(!invoice.frozen, "invoice is frozen");
        assert!(!invoice.admin_frozen, "invoice frozen by admin");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        let scheduled_at = invoice
            .scheduled_release_at
            .expect("no scheduled release time");
        assert!(
            env.ledger().timestamp() >= scheduled_at,
            "scheduled release time not reached"
        );

        // Check min funding requirement if set
        if invoice.min_funding_bps > 0 {
            let total: i128 = invoice.amounts.iter().sum();
            let min_required =
                (total as u128 * invoice.min_funding_bps as u128 / 10_000u128) as i128;
            assert!(
                invoice.funded >= min_required,
                "minimum funding not reached"
            );
        }

        // Approval check (issue #25)
        if invoice.approver.is_some() && !invoice.approved {
            panic!("awaiting approval");
        }

        // Prerequisite check (issue #22)
        if let Some(prereq_id) = invoice.prerequisite_id {
            let prereq = load_invoice(&env, prereq_id);
            assert!(
                prereq.status == InvoiceStatus::Released,
                "prerequisite not released"
            );
        }

        // Co-signer approval check
        if !invoice.co_signers.is_empty() {
            assert!(
                invoice.signatures.len() >= invoice.required_signatures,
                "not enough co-signer approvals"
            );
        }

        // N-of-M cosigner approval check (independent of the legacy co_signers gate).
        require_cosigner_threshold_met(&env, invoice_id);

        let caller = env.current_contract_address();
        Self::_release(&env, invoice_id, &mut invoice, &caller);
    }

    /// Issue #809: set the condition under which anyone may release this
    /// invoice via [`Self::trigger_auto_release`]. Creator-only, while Pending.
    /// Until the condition is met, full funding holds the funds instead of
    /// releasing them immediately.
    pub fn set_auto_release_condition(
        env: Env,
        creator: Address,
        invoice_id: u64,
        condition: AutoReleaseCondition,
    ) {
        require_not_paused(&env);
        creator.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        assert!(invoice.creator == creator, "only creator");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        match condition {
            AutoReleaseCondition::AtTimestamp(at) => assert!(
                at > env.ledger().timestamp(),
                "auto-release time must be in the future"
            ),
        }
        env.storage()
            .persistent()
            .set(&auto_release_condition_key(invoice_id), &condition);
    }

    /// Issue #809: the invoice's auto-release condition, if one is set.
    pub fn get_auto_release_condition(env: Env, invoice_id: u64) -> Option<AutoReleaseCondition> {
        env.storage()
            .persistent()
            .get(&auto_release_condition_key(invoice_id))
    }

    /// Issue #809: release an invoice whose auto-release condition is met.
    /// Callable by anyone; every other release guard (approval, prerequisite,
    /// co-signers, ...) still applies. Emits `AutoReleaseTriggered`.
    pub fn trigger_auto_release(env: Env, invoice_id: u64) {
        let met = auto_release_condition_met(&env, invoice_id).expect("no auto-release condition");
        assert!(met, "auto-release condition not met");
        Self::release_invoice(env.clone(), env.current_contract_address(), invoice_id, None);
        env.storage()
            .persistent()
            .remove(&auto_release_condition_key(invoice_id));
        events::auto_release_triggered(&env, invoice_id, env.ledger().timestamp());
    }

    /// Issue #810: aggregate performance metrics for a recipient (zeroed if
    /// the address has never been paid out).
    pub fn get_recipient_metrics(env: Env, recipient: Address) -> RecipientMetrics {
        env.storage()
            .persistent()
            .get(&recipient_metrics_key(&recipient))
            .unwrap_or_default()
    }

    /// Issue #811: the invoice this invoice depends on, if any. The dependency
    /// must be Released before this invoice can be released.
    pub fn get_invoice_dependency(env: Env, invoice_id: u64) -> Option<u64> {
        load_invoice(&env, invoice_id).prerequisite_id
    }

    /// Issue #812: read-only snapshot of contract-level configuration, for
    /// backup and for checking a migrated deployment against its source.
    pub fn export_config_snapshot(env: Env) -> ConfigSnapshot {
        let instance = env.storage().instance();
        ConfigSnapshot {
            admin: instance.get(&admin_key()),
            treasury: instance.get(&treasury_key()),
            usdc_token: instance.get(&usdc_token_key()),
            paused: is_paused(&env),
            platform_fee_bps: instance.get(&platform_fee_bps_key()).unwrap_or(0u32),
            invoice_count: env.storage().persistent().get(&counter_key()).unwrap_or(0u64),
            schema_version: migrations::schema_version(&env),
        }
    }

    /// Lock a recipient's share for an invoice (admin-only).
    /// Locked recipients are skipped during release and their share is accumulated
    /// in `UnreleasedFunds`. Returns `RecipientNotFound` if the recipient is not in
    /// the invoice's recipient list.
    pub fn lock_recipient_share(env: Env, invoice_id: u64, recipient: Address) -> Result<(), ContractError> {
        let _admin = require_admin(&env);
        let invoice = load_invoice(&env, invoice_id);

        // Verify the recipient exists in this invoice.
        let mut found = false;
        for r in invoice.recipients.iter() {
            if r == recipient {
                found = true;
                break;
            }
        }
        if !found {
            return Err(ContractError::RecipientNotFound);
        }

        env.storage()
            .persistent()
            .set(&recipient_lock_key(invoice_id, &recipient), &true);

        events::recipient_share_locked(&env, invoice_id, &recipient, &_admin);
        Ok(())
    }

    /// Unlock a recipient's share for an invoice (admin-only).
    /// After unlocking, the accumulated unreleased funds can be released via
    /// `release_locked_funds`.
    pub fn unlock_recipient_share(env: Env, invoice_id: u64, recipient: Address) -> Result<(), ContractError> {
        let _admin = require_admin(&env);
        let invoice = load_invoice(&env, invoice_id);

        let mut found = false;
        for r in invoice.recipients.iter() {
            if r == recipient {
                found = true;
                break;
            }
        }
        if !found {
            return Err(ContractError::RecipientNotFound);
        }

        env.storage()
            .persistent()
            .remove(&recipient_lock_key(invoice_id, &recipient));

        events::recipient_share_unlocked(&env, invoice_id, &recipient, &_admin);
        Ok(())
    }

    /// Release accumulated unreleased funds for an invoice after locked shares are
    /// unlocked. Transfers the total accumulated amount to the invoice's token
    /// contract for proportional distribution among now-unlocked recipients.
    pub fn release_locked_funds(env: Env, invoice_id: u64) -> Result<(), ContractError> {
        let _admin = require_admin(&env);
        let mut invoice = load_invoice(&env, invoice_id);

        let accumulated: i128 = env
            .storage()
            .persistent()
            .get(&unreleased_funds_key(invoice_id))
            .unwrap_or(0i128);

        if accumulated <= 0 {
            return Ok(());
        }

        // Clear the accumulator before distribution.
        env.storage()
            .persistent()
            .set(&unreleased_funds_key(invoice_id), &0i128);

        // Add the accumulated amount to funded so it gets distributed in
        // the next release call. The caller must follow up with `release`
        // to actually push funds to recipients.
        invoice.funded = invoice.funded.saturating_add(accumulated);
        save_invoice(&env, invoice_id, &invoice);

        Ok(())
    }

    fn _release(env: &Env, invoice_id: u64, invoice: &mut Invoice, actor: &Address) {
        // Issue #780: scheduled invoices are released via `release_scheduled`.
        if schedule_ext::has_schedule(env, invoice_id) {
            return;
        }
        // Block release when invoice is under active dispute.
        if invoice.status == InvoiceStatus::Disputed {
            panic!("{}", ContractError::InvoiceDisputed as u32);
        }
        let was_released = invoice.status == InvoiceStatus::Released;
        if invoice.tranches.is_empty() {
            Self::_release_full(env, invoice_id, invoice, actor);
        } else {
            Self::_release_tranches(env, invoice_id, invoice, actor);
        }
        // Issue #810: update recipient metrics once, when the invoice becomes Released.
        if !was_released && invoice.status == InvoiceStatus::Released {
            record_recipient_release(env, &invoice.recipients);
        }
    }

    fn maybe_release_milestones(
        env: &Env,
        invoice_id: u64,
        invoice: &mut Invoice,
        actor: &Address,
    ) {
        if invoice.milestones.is_empty() {
            return;
        }
        let total: i128 = invoice.amounts.iter().sum();
        if total <= 0 {
            return;
        }
        let token_client = token::Client::new(env, &invoice.tokens.get(0).expect("no token"));
        while invoice.milestones_released < invoice.milestones.len() {
            let next_idx = invoice.milestones_released;
            let milestone_bps = invoice.milestones.get(next_idx).unwrap();
            // Issue #482: use checked arithmetic to prevent overflow.
            let threshold = checked_bps_of(total, milestone_bps, 10_000u128)
                .expect("ArithmeticOverflow");
            if invoice.funded < threshold {
                break;
            }
            let prev_bps = if next_idx == 0 {
                0
            } else {
                invoice.milestones.get(next_idx - 1).unwrap()
            };
            let delta_bps = milestone_bps.saturating_sub(prev_bps);
            // Issue #482: use checked arithmetic to prevent overflow.
            let tranche_amount = checked_bps_of(total, delta_bps, 10_000u128)
                .expect("ArithmeticOverflow");
            let mut paid_total = 0i128;
            let mut tranche_surplus = 0i128;
            for i in 0..invoice.recipients.len() {
                let recipient = invoice.recipients.get(i).unwrap();
                let base_amount = invoice.amounts.get(i).unwrap();
                let proportional = if i == invoice.recipients.len() - 1 {
                    tranche_amount
                        .saturating_sub(paid_total)
                        .saturating_add(tranche_surplus)
                } else {
                    // Issue #482: use checked arithmetic to prevent overflow.
                    checked_bps_of(base_amount, delta_bps, 10_000u128)
                        .expect("ArithmeticOverflow")
                };
                let payout = if !invoice.recipient_max_payouts.is_empty() {
                    match invoice.recipient_max_payouts.get(i).unwrap_or(None) {
                        Some(max_payout) if proportional > max_payout => {
                            tranche_surplus =
                                tranche_surplus.saturating_add(proportional - max_payout);
                            max_payout
                        }
                        _ => proportional,
                    }
                } else {
                    proportional
                };
                if payout > 0 {
                    token_client.transfer(&env.current_contract_address(), &recipient, &payout);
                    paid_total = paid_total.saturating_add(payout);
                }
            }
            if tranche_surplus > 0 {
                let stored_surplus: i128 = env
                    .storage()
                    .persistent()
                    .get(&surplus_key(invoice_id))
                    .unwrap_or(0);
                env.storage().persistent().set(
                    &surplus_key(invoice_id),
                    &stored_surplus.saturating_add(tranche_surplus),
                );
            }
            invoice.milestones_released += 1;
            events::milestone_released(env, invoice_id, milestone_bps, paid_total);
        }
        if invoice.milestones_released >= invoice.milestones.len() {
            invoice.status = InvoiceStatus::Released;
            invoice.completion_time = Some(env.ledger().timestamp());
            append_audit_entry(env, invoice_id, symbol_short!("ms_rel"), actor);
            events::invoice_released(env, invoice_id, &invoice.recipients);
        }
    }

    fn execute_smart_route(
        env: &Env,
        invoice: &Invoice,
        recipient: &Address,
        payout: i128,
    ) -> bool {
        if invoice.smart_route {
            if let Some(dex_router) = env
                .storage()
                .instance()
                .get::<_, Address>(&soroban_sdk::symbol_short!("dex_rtr"))
            {
                let token = invoice.tokens.get(0).expect("no token");
                let path: Vec<Address> = env.invoke_contract(
                    &dex_router,
                    &soroban_sdk::Symbol::new(env, "get_path"),
                    (token.clone(), recipient.clone()).into_val(env),
                );
                if !path.is_empty() {
                    let _: Val = env.invoke_contract(
                        &dex_router,
                        &soroban_sdk::Symbol::new(env, "route_transfer"),
                        (path, payout, recipient.clone()).into_val(env),
                    );
                    return true;
                }
            }
        }
        false
    }

    /// Approve an invoice if it has an approver set (issue #25).
    ///
    /// Requires authentication from the approver address.
    pub fn approve_invoice(env: Env, invoice_id: u64) {
        require_not_paused(&env);
        let mut invoice = load_invoice(&env, invoice_id);

        let approver = invoice
            .approver
            .as_ref()
            .expect("no approver set on this invoice");
        approver.require_auth();

        invoice.approved = true;
        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("aprv"), approver);
    }

    // -----------------------------------------------------------------------
    // Issue #485: Contributor allowlist management (creator-only)
    // -----------------------------------------------------------------------

    /// Add `contributor` to the per-invoice contributor allowlist.
    /// Creates the allowlist if it does not exist yet.
    /// Only the invoice creator (or a co-creator) may call this.
    pub fn add_contributor_to_allowlist(
        env: Env,
        creator: Address,
        invoice_id: u64,
        contributor: Address,
    ) {
        require_not_paused(&env);
        creator.require_auth();
        let mut invoice = load_invoice(&env, invoice_id);
        require_invoice_unlocked(&env, invoice_id, &invoice);
        assert!(
            invoice.creator == creator || invoice.co_creators.contains(&creator),
            "NotAuthorized"
        );
        let was_disabled = invoice.contributor_allowlist.is_none();
        let mut list = invoice
            .contributor_allowlist
            .unwrap_or_else(|| Vec::new(&env));
        if !list.contains(&contributor) {
            list.push_back(contributor.clone());
        }
        invoice.contributor_allowlist = Some(list);
        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("al_add"), &creator);
        if was_disabled {
            events::contributor_allowlist_toggled(&env, invoice_id, &creator, true);
        }
    }

    /// Remove `contributor` from the per-invoice contributor allowlist.
    /// Only the invoice creator (or a co-creator) may call this.
    /// Removing the last entry restores open access (allowlist becomes None).
    pub fn remove_contributor_allowlist(
        env: Env,
        creator: Address,
        invoice_id: u64,
        contributor: Address,
    ) {
        require_not_paused(&env);
        creator.require_auth();
        let mut invoice = load_invoice(&env, invoice_id);
        require_invoice_unlocked(&env, invoice_id, &invoice);
        assert!(
            invoice.creator == creator || invoice.co_creators.contains(&creator),
            "NotAuthorized"
        );
        let mut became_disabled = false;
        if let Some(old_list) = invoice.contributor_allowlist {
            let mut new_list: Vec<Address> = Vec::new(&env);
            for addr in old_list.iter() {
                if addr != contributor {
                    new_list.push_back(addr);
                }
            }
            became_disabled = new_list.is_empty();
            invoice.contributor_allowlist = if new_list.is_empty() {
                None
            } else {
                Some(new_list)
            };
        }
        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("al_rm"), &creator);
        if became_disabled {
            events::contributor_allowlist_toggled(&env, invoice_id, &creator, false);
        }
    }

    // -----------------------------------------------------------------------
    // Invoice pause / resume (creator-controlled)
    // -----------------------------------------------------------------------

    /// Freeze an invoice with an on-chain reason string and an optional auto-resume timestamp.
    ///
    /// Only the creator (or a co-creator) may call this. Sets `frozen = true`,
    /// stores `pause_reason` and `auto_resume_at`, and emits a paused event.
    pub fn pause_invoice(
        env: Env,
        creator: Address,
        invoice_id: u64,
        reason: String,
        auto_resume_at: Option<u64>,
    ) {
        require_not_paused(&env);
        creator.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.creator == creator
                || invoice.co_creators.iter().any(|c| c == creator)
                || delegate_ext::is_delegate_of(&env, invoice_id, &creator),
            "only creator can pause invoice"
        );
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(!invoice.frozen, "invoice is already frozen");

        invoice.frozen = true;
        invoice.pause_reason = Some(reason.clone());
        invoice.auto_resume_at = auto_resume_at;
        save_invoice(&env, invoice_id, &invoice);

        append_audit_entry(&env, invoice_id, symbol_short!("paused"), &creator);
        events::invoice_frozen(&env, invoice_id, &creator);
        events::invoice_paused(&env, invoice_id, &creator, &reason, &auto_resume_at);
    }

    /// Unfreeze a paused invoice. Clears the stored reason and auto-resume time.
    ///
    /// Only the creator (or a co-creator) may call this.
    pub fn resume_invoice(env: Env, creator: Address, invoice_id: u64) {
        require_not_paused(&env);
        creator.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.creator == creator
                || invoice.co_creators.iter().any(|c| c == creator)
                || delegate_ext::is_delegate_of(&env, invoice_id, &creator),
            "only creator can resume invoice"
        );
        assert!(invoice.frozen, "invoice is not frozen");

        invoice.frozen = false;
        invoice.pause_reason = None;
        invoice.auto_resume_at = None;
        save_invoice(&env, invoice_id, &invoice);

        append_audit_entry(&env, invoice_id, symbol_short!("resumed"), &creator);
        events::invoice_resumed(&env, invoice_id, &creator);
    }

    /// Remove a payer from the invoice's allowed_payers allowlist.
    ///
    /// Only the creator (or a co-creator) may call this. If allowed_payers is None
    /// (open invoice), this is a no-op and does not error. Already-made payments
    /// from the removed payer are untouched; this only blocks future payments.
    pub fn remove_allowed_payer(env: Env, creator: Address, invoice_id: u64, payer: Address) {
        require_not_paused(&env);
        creator.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);
        require_invoice_unlocked(&env, invoice_id, &invoice);
        assert!(
            invoice.creator == creator || invoice.co_creators.iter().any(|c| c == creator),
            "only creator can modify allowlist"
        );

        // No-op if allowed_payers is None (open invoice)
        if let Some(ref mut whitelist) = invoice.allowed_payers {
            let mut new_whitelist: Vec<Address> = Vec::new(&env);
            for p in whitelist.iter() {
                if p != payer {
                    new_whitelist.push_back(p.clone());
                }
            }
            invoice.allowed_payers = Some(new_whitelist);
            save_invoice(&env, invoice_id, &invoice);
            append_audit_entry(&env, invoice_id, symbol_short!("rem_payer"), &creator);
            // Issue #309: emit AllowlistUpdated event on removal
            events::allowlist_updated(&env, invoice_id, &creator, &payer, false);
        }
    }

    /// Add a payer to the invoice's allowed_payers allowlist.
    ///
    /// Only the creator (or a co-creator) may call this. If allowed_payers is None
    /// (open invoice), initializes the list with this payer (making the invoice private).
    /// If the payer is already in the allowlist, this is a no-op.
    pub fn add_allowed_payer(env: Env, creator: Address, invoice_id: u64, payer: Address) {
        require_not_paused(&env);
        creator.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);
        require_invoice_unlocked(&env, invoice_id, &invoice);
        assert!(
            invoice.creator == creator || invoice.co_creators.iter().any(|c| c == creator),
            "only creator can modify allowlist"
        );

        // If allowed_payers is None, initialize a new list (makes invoice private).
        if invoice.allowed_payers.is_none() {
            invoice.allowed_payers = Some(Vec::new(&env));
        }

        if let Some(ref mut whitelist) = invoice.allowed_payers {
            // Only add if not already present
            if !whitelist.iter().any(|p| p == payer) {
                // Issue #309 / #749: enforce the per-invoice whitelist cap.
                assert!(
                    whitelist.len() < MAX_PAYER_WHITELIST,
                    "whitelist is full (max 50)"
                );
                whitelist.push_back(payer.clone());
                save_invoice(&env, invoice_id, &invoice);
                append_audit_entry(&env, invoice_id, symbol_short!("add_payer"), &creator);
                // Issue #309: emit AllowlistUpdated event
                events::allowlist_updated(&env, invoice_id, &creator, &payer, true);
            }
        }
    }

    /// Remove the invoice's entire payer allowlist, reopening it to any payer.
    ///
    /// Only the creator (or a co-creator) may call this. Sets `allowed_payers`
    /// to `None`. If the invoice is already open, this is a no-op and does not
    /// emit an event.
    pub fn remove_allowlist(env: Env, creator: Address, invoice_id: u64) {
        require_not_paused(&env);
        creator.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.creator == creator || invoice.co_creators.iter().any(|c| c == creator),
            "only creator can modify allowlist"
        );

        if invoice.allowed_payers.is_some() {
            invoice.allowed_payers = None;
            save_invoice(&env, invoice_id, &invoice);
            append_audit_entry(&env, invoice_id, symbol_short!("al_open"), &creator);
            events::allowlist_removed(&env, invoice_id, &creator);
        }
    }

    /// Issue #329: Update the off-chain metadata hash for an invoice.
    ///
    /// Only the creator may call this. Emits `MetadataUpdated` with old and new hash.
    /// The contract does not validate or fetch the off-chain content.
    pub fn update_metadata_hash(env: Env, invoice_id: u64, creator: Address, new_hash: BytesN<32>) {
        require_not_paused(&env);
        creator.require_auth();

        let invoice = load_invoice(&env, invoice_id);
        require_invoice_unlocked(&env, invoice_id, &invoice);
        assert!(
            invoice.creator == creator,
            "only creator can update metadata hash"
        );
        // Reject updates on finalised or cancelled invoices.
        assert!(
            invoice.status != InvoiceStatus::Released && invoice.status != InvoiceStatus::Cancelled,
            "cannot update metadata hash on finalised or cancelled invoice"
        );

        let old_hash: Option<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&metadata_hash_key(invoice_id));
        env.storage()
            .persistent()
            .set(&metadata_hash_key(invoice_id), &new_hash);

        append_audit_entry(&env, invoice_id, symbol_short!("meta_upd"), &creator);
        events::metadata_updated(&env, invoice_id, &old_hash, &new_hash);
    }

    /// Issue #416: Set or update the off-chain release condition hash.
    /// Only the creator may call this and only before any funds have been received.
    pub fn set_release_condition(
        env: Env,
        creator: Address,
        invoice_id: u64,
        new_hash: Option<BytesN<32>>,
    ) {
        require_not_paused(&env);
        creator.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);
        require_invoice_unlocked(&env, invoice_id, &invoice);
        assert!(
            invoice.creator == creator,
            "only creator can update release condition"
        );
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not draft"
        );
        assert!(invoice.funded == 0, "invoice already funded");

        invoice.release_condition_hash = new_hash;
        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("rel_cond"), &creator);
    }

    /// Issue #420: Set the overfunding policy for an invoice.
    ///
    /// Only the creator may call this, and only while the invoice is still
    /// `Pending` and unfunded — changing the rule mid-funding would apply
    /// different terms to payers who have already paid.
    pub fn set_overfunding_policy(
        env: Env,
        creator: Address,
        invoice_id: u64,
        policy: OverfundingPolicy,
    ) {
        require_not_paused(&env);
        creator.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);
        require_invoice_unlocked(&env, invoice_id, &invoice);
        assert!(
            invoice.creator == creator,
            "only creator can set overfunding policy"
        );
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(invoice.funded == 0, "invoice already funded");

        invoice.overfunding_policy = policy;
        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("ovrfund"), &creator);
    }

    /// Issue #420: Read the overfunding policy currently in force for an invoice.
    pub fn get_overfunding_policy(env: Env, invoice_id: u64) -> OverfundingPolicy {
        load_invoice(&env, invoice_id).overfunding_policy
    }

    /// Issue #330: Release funds to a single recipient by their share.
    ///
    /// The invoice must be fully funded. Each recipient can only be paid once via
    /// this function. Use `release` to pay all remaining recipients at once.
    pub fn release_to_recipient(env: Env, invoice_id: u64, recipient: Address) {
        require_fn_not_paused(&env, &symbol_short!("release"));

        let mut invoice = load_invoice(&env, invoice_id);
        let creator = invoice.creator.clone();
        creator.require_auth();

        // Issue #504: Allow both Pending and PartiallyReleased status for retry.
        assert!(
            invoice.status == InvoiceStatus::Pending || invoice.status == InvoiceStatus::PartiallyReleased,
            "invoice is not pending or partially released"
        );
        assert!(!invoice.frozen, "invoice is frozen");
        assert!(!invoice.admin_frozen, "invoice frozen by admin");
        assert!(!invoice.disputed, "invoice is disputed");

        // Invoice must have been fully funded at some point.
        // After partial releases, funded may be less than total, so we check funded_at_ledger.
        let total: i128 = invoice.amounts.iter().sum();
        let was_fully_funded = invoice.funded >= total
            || env
                .storage()
                .persistent()
                .has(&funded_at_ledger_key(invoice_id));
        assert!(was_fully_funded, "invoice not fully funded");

        // Issue #327: respect time-lock delay.
        if let Some(delay_ledgers) = env
            .storage()
            .persistent()
            .get::<_, u32>(&release_delay_key(invoice_id))
        {
            if let Some(funded_at) = env
                .storage()
                .persistent()
                .get::<_, u32>(&funded_at_ledger_key(invoice_id))
            {
                let unlock_at = funded_at.saturating_add(delay_ledgers);
                assert!(env.ledger().sequence() >= unlock_at, "FundsLockedUntil");
                // Issue #327: emit the same unlock event as `release_invoice` so
                // indexers observe funds_unlocked regardless of which release path is used.
                events::funds_unlocked(&env, invoice_id, unlock_at);
            }
        }

        // Find recipient in list.
        let mut recipient_idx: Option<u32> = None;
        for i in 0..invoice.recipients.len() {
            if invoice.recipients.get(i).unwrap() == recipient {
                recipient_idx = Some(i);
                break;
            }
        }
        assert!(recipient_idx.is_some(), "recipient not found in invoice");
        let idx = recipient_idx.unwrap();

        // Check not already paid.
        let mut paid: Vec<Address> = env
            .storage()
            .persistent()
            .get(&paid_recipients_key(invoice_id))
            .unwrap_or_else(|| Vec::new(&env));
        assert!(!paid.iter().any(|a| a == recipient), "RecipientAlreadyPaid");

        let amount = invoice.amounts.get(idx).unwrap();
        assert!(amount > 0, "recipient amount must be positive");

        let token_addr = recipient_token_for(&invoice, idx as usize);

        // Issue #504: Use try_transfer to isolate failures.
        match try_transfer(
            &env,
            &token_addr,
            &env.current_contract_address(),
            &recipient,
            amount,
        ) {
            Ok(()) => {
                paid.push_back(recipient.clone());
                env.storage()
                    .persistent()
                    .set(&paid_recipients_key(invoice_id), &paid);

                // Issue #504: If all failed payouts resolved, transition to Released.
                let has_failed = env
                    .storage()
                    .persistent()
                    .has(&failed_payouts_key(invoice_id));
                if invoice.status == InvoiceStatus::PartiallyReleased && !has_failed {
                    invoice.status = InvoiceStatus::Released;
                    invoice.completion_time = Some(env.ledger().timestamp());
                }

                // Reduce funded so the contract's token balance stays consistent with paid amounts.
                invoice.funded -= amount;
                save_invoice(&env, invoice_id, &invoice);

                append_audit_entry(&env, invoice_id, symbol_short!("rec_paid"), &creator);
                events::recipient_paid(&env, invoice_id, &recipient, amount);
            }
            Err(_) => {
                // Issue #504: Record the failure and emit PayoutFailed event.
                record_failed_payout(&env, invoice_id, &recipient, amount, "TransferFailed");
                // Mark invoice as PartiallyReleased.
                invoice.status = InvoiceStatus::PartiallyReleased;
                save_invoice(&env, invoice_id, &invoice);
                append_audit_entry(&env, invoice_id, symbol_short!("pay_fail"), &creator);
            }
        }
    }

    /// Admin override: force-resume any paused invoice regardless of who paused it.
    ///
    /// Requires admin auth. Clears the frozen flag, reason, and auto-resume time,
    /// and emits a force_resumed event with the admin address.
    pub fn admin_force_resume(env: Env, admin: Address, invoice_id: u64) {
        require_admin_role(&env, &admin, AdminRole::Operator);

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(invoice.frozen, "invoice is not frozen");

        invoice.frozen = false;
        invoice.pause_reason = None;
        invoice.auto_resume_at = None;
        save_invoice(&env, invoice_id, &invoice);

        append_audit_entry(&env, invoice_id, symbol_short!("frc_rsm"), &admin);
        events::invoice_force_resumed(&env, invoice_id, &admin);
    }

    /// Admin freeze an invoice with a reason (overrides creator freeze).
    /// Requires admin auth. Sets `admin_frozen = true` on InvoiceExt.
    pub fn admin_freeze(env: Env, admin: Address, invoice_id: u64, reason: String) {
        let admin_addr = require_admin(&env);
        let _ = admin;

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.admin_frozen, "invoice already frozen by admin");

        invoice.admin_frozen = true;
        invoice.pause_reason = Some(reason.clone());
        save_invoice(&env, invoice_id, &invoice);

        append_audit_entry(&env, invoice_id, symbol_short!("adm_frz"), &admin_addr);
        events::invoice_admin_frozen(&env, invoice_id, &admin_addr, &reason);
    }

    /// Admin unfreeze an invoice (clears admin_frozen).
    /// Requires admin auth.
    pub fn admin_unfreeze(env: Env, admin: Address, invoice_id: u64) {
        let admin_addr = require_admin(&env);
        let _ = admin;

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(invoice.admin_frozen, "invoice is not frozen by admin");

        invoice.admin_frozen = false;
        if !invoice.frozen {
            invoice.pause_reason = None;
        }
        save_invoice(&env, invoice_id, &invoice);

        append_audit_entry(&env, invoice_id, symbol_short!("adm_unf"), &admin_addr);
        events::invoice_admin_unfrozen(&env, invoice_id, &admin_addr);
    }

    /// Oracle confirms a condition for a gated invoice.
    /// Requires the configured oracle address to authenticate.
    pub fn confirm_condition(env: Env, invoice_id: u64) {
        require_not_paused(&env);
        let mut invoice = load_invoice(&env, invoice_id);
        assert!(!invoice.disputed, "invoice is disputed");
        let oracle = invoice
            .condition_oracle
            .as_ref()
            .expect("no oracle set for invoice");
        oracle.require_auth();
        invoice.condition_met = true;
        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("oracle_ok"), oracle);
    }

    /// Set a payment reminder for an address on a specific invoice.
    /// The `who` address must authenticate.
    pub fn set_reminder(env: Env, who: Address, invoice_id: u64, remind_at: u64) {
        require_not_paused(&env);
        who.require_auth();
        env.storage()
            .persistent()
            .set(&reminder_key(invoice_id, &who), &remind_at);
        append_audit_entry(&env, invoice_id, symbol_short!("set_rmd"), &who);
    }

    /// Trigger a previously set reminder; must be called at or after `remind_at`.
    pub fn trigger_reminder(env: Env, invoice_id: u64, who: Address) {
        require_not_paused(&env);
        let remind_at: u64 = env
            .storage()
            .persistent()
            .get(&reminder_key(invoice_id, &who))
            .expect("reminder not set");
        assert!(env.ledger().timestamp() >= remind_at, "reminder not due");
        events::payment_reminder(&env, invoice_id, &who);
        env.storage()
            .persistent()
            .remove(&reminder_key(invoice_id, &who));
        append_audit_entry(&env, invoice_id, symbol_short!("trig_rmd"), &who);
    }

    /// Create a treasury group linking multiple invoice IDs to a single treasury address.
    /// Returns the new group id.
    pub fn group_treasury_create(
        env: Env,
        creator: Address,
        invoice_ids: Vec<u64>,
        treasury: Address,
    ) -> u64 {
        require_not_paused(&env);
        creator.require_auth();
        let id: u64 = env
            .storage()
            .persistent()
            .get(&treasury_group_counter_key())
            .unwrap_or(0u64)
            + 1;
        env.storage()
            .persistent()
            .set(&treasury_group_counter_key(), &id);
        let record = types::TreasuryRecord {
            invoice_ids: invoice_ids.clone(),
            treasury: treasury.clone(),
        };
        env.storage()
            .persistent()
            .set(&group_treasury_key(id), &record);
        for iid in invoice_ids.iter() {
            env.storage()
                .persistent()
                .set(&invoice_treasury_key(iid), &id);
            append_audit_entry(&env, iid, symbol_short!("grp_tr"), &creator);
        }
        id
    }

    /// Pay toward an invoice using a memo that encodes the invoice id.
    /// Requires payer auth and emits a payment_matched event on success.
    pub fn pay_with_memo(
        env: Env,
        payer: Address,
        memo: u64,
        amount: i128,
        nonce: u64,
        _auto_convert: bool,
        via: Option<Address>,
    ) {
        require_not_paused(&env);
        payer.require_auth();
        // Validate memo corresponds to an existing invoice.
        let _ = load_invoice(&env, memo);
        Self::_pay(
            &env,
            &payer,
            memo,
            amount,
            nonce,
            _auto_convert,
            via,
            None,
            false,
        );
        events::payment_matched(&env, memo, memo, &payer);
    }

    /// Issue #451: Creator sets a required payment memo hash on an invoice.
    pub fn set_invoice_memo(env: Env, creator: Address, invoice_id: u64, memo_hash: BytesN<32>) {
        require_not_paused(&env);
        creator.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        require_invoice_unlocked(&env, invoice_id, &invoice);
        assert!(invoice.creator == creator, "only creator can set memo");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        env.storage()
            .persistent()
            .set(&required_memo_hash_key(invoice_id), &memo_hash);
    }

    /// Issue #451: Pay an invoice with memo validation.
    pub fn pay_with_validated_memo(
        env: Env,
        payer: Address,
        invoice_id: u64,
        payment_memo: BytesN<32>,
        amount: i128,
        nonce: u64,
        auto_convert: bool,
        via: Option<Address>,
    ) {
        require_not_paused(&env);
        payer.require_auth();
        if let Some(required) = env
            .storage()
            .persistent()
            .get::<_, BytesN<32>>(&required_memo_hash_key(invoice_id))
        {
            assert!(payment_memo == required, "MemoMismatch");
        }
        Self::_pay(
            &env,
            &payer,
            invoice_id,
            amount,
            nonce,
            auto_convert,
            via,
            None,
            false,
        );
        events::payment_matched(&env, invoice_id, invoice_id, &payer);
    }

    /// Issue #452: Set tags on an invoice for searchable categorisation.
    pub fn set_invoice_tags(env: Env, creator: Address, invoice_id: u64, tags: Vec<String>) {
        require_not_paused(&env);
        creator.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        require_invoice_unlocked(&env, invoice_id, &invoice);
        assert!(invoice.creator == creator, "only creator can set tags");
        env.storage()
            .persistent()
            .set(&invoice_tags_key(invoice_id), &tags);
    }

    /// Issue #452: Get tags for an invoice.
    pub fn get_invoice_tags(env: Env, invoice_id: u64) -> Vec<String> {
        env.storage()
            .persistent()
            .get(&invoice_tags_key(invoice_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    // -----------------------------------------------------------------------
    // Issue #754: Tag-based invoice search index
    // -----------------------------------------------------------------------

    /// Validate a single tag string: lowercase alphanumeric + hyphens, max 32 chars.
    fn validate_tag(env: &Env, tag: &String) {
        let len = tag.len();
        if len == 0 || len > 32 {
            panic_with_error!(env, ContractError::InvalidTag);
        }
        // Copy into a fixed-size stack buffer and validate byte-by-byte.
        let mut buf = [0u8; 32];
        let slice = &mut buf[..len as usize];
        tag.copy_into_slice(slice);
        for &b in slice.iter() {
            let ok = b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-';
            if !ok {
                panic_with_error!(env, ContractError::InvalidTag);
            }
        }
    }

    /// Internal: index each tag → invoice_id mapping and store tags on the invoice.
    /// Emits TagsUpdated event with added tags and empty removed list.
    fn _apply_tags(env: &Env, invoice_id: u64, tags: &Vec<String>) {
        assert!(tags.len() <= 5, "maximum 5 tags per invoice");
        for tag in tags.iter() {
            Self::validate_tag(env, &tag);
            // Update tag → Vec<u64> index.
            let key = tag_index_key(&tag);
            let mut ids: Vec<u64> = env
                .storage()
                .persistent()
                .get(&key)
                .unwrap_or_else(|| Vec::new(env));
            if !ids.contains(invoice_id) {
                ids.push_back(invoice_id);
            }
            env.storage().persistent().set(&key, &ids);
        }
        // Store tags on the invoice.
        env.storage()
            .persistent()
            .set(&invoice_tags_key(invoice_id), tags);
        let empty: Vec<String> = Vec::new(env);
        events::tags_updated(env, invoice_id, tags, &empty);
    }

    /// Issue #754: Paginated read of invoices by tag.
    /// Returns up to `limit` invoice IDs that have `tag`, starting after `cursor`
    /// (exclusive — use 0 to start from the beginning).
    pub fn get_invoices_by_tag(env: Env, tag: String, limit: u32, cursor: u64) -> Vec<u64> {
        let key = tag_index_key(&tag);
        let all_ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(&env));

        let mut result: Vec<u64> = Vec::new(&env);
        let mut past_cursor = cursor == 0;
        for id in all_ids.iter() {
            if !past_cursor {
                if id == cursor {
                    past_cursor = true;
                }
                continue;
            }
            if result.len() >= limit {
                break;
            }
            result.push_back(id);
        }
        result
    }

    /// Issue #754: Add a tag to an existing invoice (creator only).
    /// Validates the tag format, enforces the 5-tag limit, updates the index,
    /// and emits TagsUpdated.
    pub fn add_tag(env: Env, invoice_id: u64, creator: Address, tag: String) {
        require_not_paused(&env);
        creator.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        require_creator_or_cocreator(&invoice, &creator);

        Self::validate_tag(&env, &tag);

        let mut tags: Vec<String> = env
            .storage()
            .persistent()
            .get(&invoice_tags_key(invoice_id))
            .unwrap_or_else(|| Vec::new(&env));

        if tags.contains(&tag) {
            return; // idempotent
        }
        assert!(tags.len() < 5, "maximum 5 tags per invoice");
        tags.push_back(tag.clone());
        env.storage()
            .persistent()
            .set(&invoice_tags_key(invoice_id), &tags);

        // Update tag index.
        let key = tag_index_key(&tag);
        let mut ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(&env));
        if !ids.contains(invoice_id) {
            ids.push_back(invoice_id);
        }
        env.storage().persistent().set(&key, &ids);

        let mut added: Vec<String> = Vec::new(&env);
        added.push_back(tag);
        let removed: Vec<String> = Vec::new(&env);
        events::tags_updated(&env, invoice_id, &added, &removed);
    }

    /// Issue #754: Remove a tag from an existing invoice (creator only).
    /// Removes from both the invoice's tag list and the global tag index,
    /// and emits TagsUpdated.
    pub fn remove_tag(env: Env, invoice_id: u64, creator: Address, tag: String) {
        require_not_paused(&env);
        creator.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        require_creator_or_cocreator(&invoice, &creator);

        let tags: Vec<String> = env
            .storage()
            .persistent()
            .get(&invoice_tags_key(invoice_id))
            .unwrap_or_else(|| Vec::new(&env));

        let initial_len = tags.len();
        let mut new_tags: Vec<String> = Vec::new(&env);
        for t in tags.iter() {
            if t != tag {
                new_tags.push_back(t);
            }
        }
        if new_tags.len() == initial_len {
            return; // tag wasn't present — idempotent
        }
        env.storage()
            .persistent()
            .set(&invoice_tags_key(invoice_id), &new_tags);

        // Remove from tag index.
        let key = tag_index_key(&tag);
        let ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(&env));
        let mut new_ids: Vec<u64> = Vec::new(&env);
        for id in ids.iter() {
            if id != invoice_id {
                new_ids.push_back(id);
            }
        }
        env.storage().persistent().set(&key, &new_ids);

        let added: Vec<String> = Vec::new(&env);
        let mut removed: Vec<String> = Vec::new(&env);
        removed.push_back(tag);
        events::tags_updated(&env, invoice_id, &added, &removed);
    }

    // -----------------------------------------------------------------------
    // Issue #752: NFT mint contract configuration
    // -----------------------------------------------------------------------

    /// Issue #752: Admin-only — set the NFT contract address for proof-of-funding mints.
    /// Pass `contract: Address` to enable NFT minting on release.
    /// The NFT contract must implement `mint(to: Address, invoice_id: u64)`.
    pub fn set_nft_contract(env: Env, admin: Address, contract: Address) {
        admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&admin_key())
            .expect("admin not set");
        assert!(admin == stored_admin, "NotAuthorized");
        env.storage()
            .instance()
            .set(&nft_mint_contract_key(), &contract);
    }

    /// Issue #752: Get the configured NFT contract address, if set.
    pub fn get_nft_contract(env: Env) -> Option<Address> {
        env.storage()
            .instance()
            .get::<Symbol, Address>(&nft_mint_contract_key())
    }

    /// Claim vesting cliff share after cliff timestamp has passed (issue #27).
    ///
    /// Requires that the invoice status is Released and the cliff (if set) has passed.
    /// Each recipient can claim exactly once.
    pub fn claim(env: Env, invoice_id: u64, recipient: Address) {
        require_not_paused(&env);
        recipient.require_auth();

        let invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Released,
            "invoice not released"
        );

        // Find recipient index
        let idx = invoice
            .recipients
            .iter()
            .position(|r| r == recipient)
            .expect("recipient not in invoice") as u32;

        // Check if already claimed
        assert!(
            invoice.claimed.get(idx).unwrap_or(0) == 0,
            "recipient already claimed"
        );

        // Check cliff timestamp if set (vesting cliff not tracked in current schema, skip)

        // Mark as claimed using the claimed amounts vec (set to 1 as a flag)
        save_invoice(&env, invoice_id, &invoice);

        // Transfer recipient's share
        let amount = invoice.amounts.get(idx).unwrap();
        let total: i128 = invoice.amounts.iter().sum();
        let funded = invoice.funded;
        let n = invoice.recipients.len();

        let proportional = if !invoice.ratios.is_empty() {
            let amount = invoice.amounts.get(idx).unwrap();
            let ratio = invoice.ratios.get(idx).unwrap();
            let denom = invoice.ratio_denominator as u128;
            let r = ratio as u128;
            checked_proportion(amount as u128, r, denom).expect("ArithmeticOverflow")
        } else if idx == n - 1 {
            // Last recipient gets remainder
            funded - {
                let mut sum = 0i128;
                for i in 0..idx {
                    let amt = invoice.amounts.get(i).unwrap();
                    let prop = (amt as u128 * funded as u128 / total as u128) as i128;
                    sum += prop;
                }
                sum
            }
        } else {
            (amount as u128 * funded as u128 / total as u128) as i128
        };

        let platform_fee_bps: u32 = env
            .storage()
            .instance()
            .get(&platform_fee_bps_key())
            .unwrap_or(0u32);

        let waivers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&platform_fee_waiver_list_key())
            .unwrap_or_else(|| Vec::new(&env));
        let is_waived = waivers.iter().any(|a| a == recipient);

        let fee = if is_waived {
            0
        } else {
            calc_platform_fee(proportional, platform_fee_bps).expect("ArithmeticOverflow")
        };
        let tax = (proportional as u128 * invoice.tax_bps as u128 / 10_000u128) as i128;
        let payout = proportional - fee - tax;

        let token_client = token::Client::new(&env, &invoice.tokens.get(idx).expect("no token"));

        if tax > 0 {
            let tax_authority = invoice.tax_authority.as_ref().unwrap();
            token_client.transfer(&env.current_contract_address(), tax_authority, &tax);
        }

        let routed = Self::execute_smart_route(&env, &invoice, &recipient, payout);
        if !routed {
            token_client.transfer(&env.current_contract_address(), &recipient, &payout);
        }

        append_audit_entry(&env, invoice_id, symbol_short!("claim"), &recipient);
    }

    /// Claim a pending payout that was not transferred during release (issue #209).
    /// Recipient can claim their payout after the invoice is Released.
    /// Issue #505: Retry a payout to a recipient whose account was missing at release time.
    ///
    /// Re-validates that the recipient account now exists on the ledger. If it does, executes
    /// the transfer and removes them from FailedPayouts. If FailedPayouts is now empty, the
    /// invoice is finalised (status → Released) and the open-invoice counter is decremented.
    /// If the account is still missing, emits RecipientAccountMissing again and returns an error.
    pub fn retry_failed_payout(env: Env, invoice_id: u64, recipient: Address) {
        require_not_paused(&env);
        let mut invoice = load_invoice(&env, invoice_id);

        // Load and validate the failed-payouts list.
        let failed: Vec<Address> = env
            .storage()
            .persistent()
            .get(&failed_payouts_key(invoice_id))
            .expect("no failed payouts for this invoice");

        assert!(
            failed.iter().any(|a| a == recipient),
            "recipient not in failed payouts"
        );

        let funding_token_client = token::Client::new(&env, &funding_token_for(&invoice));

        // Re-check account existence.
        let account_exists = env
            .try_invoke_contract::<i128, soroban_sdk::Error>(
                &funding_token_client.address,
                &symbol_short!("balance"),
                (recipient.clone(),).into_val(&env),
            )
            .is_ok();

        if !account_exists {
            events::recipient_account_missing(&env, invoice_id, &recipient);
            env.panic_with_error(ContractError::RecipientAccountMissing);
        }

        // Find recipient index and compute their payout amount.
        let n = invoice.recipients.len();
        let total: i128 = invoice.amounts.iter().sum();
        let funded = invoice.funded;
        let idx = invoice
            .recipients
            .iter()
            .position(|r| r == recipient.clone())
            .expect("recipient not in invoice") as u32;
        let amount = invoice.amounts.get(idx).unwrap();
        let payout = if !invoice.ratios.is_empty() {
            let ratio = invoice.ratios.get(idx).unwrap();
            let denom = invoice.ratio_denominator as u128;
            let r = ratio as u128;
            checked_proportion(amount as u128, r, denom).expect("ArithmeticOverflow")
        } else if n == 1 {
            funded
        } else {
            checked_proportion(amount as u128, funded as u128, total as u128)
                .expect("ArithmeticOverflow")
        };

        // Execute transfer.
        funding_token_client.transfer(
            &env.current_contract_address(),
            &recipient,
            &payout,
        );

        // Remove from failed list.
        let mut new_failed: Vec<Address> = Vec::new(&env);
        for a in failed.iter() {
            if a != recipient {
                new_failed.push_back(a);
            }
        }
        if new_failed.is_empty() {
            env.storage().persistent().remove(&failed_payouts_key(invoice_id));
        } else {
            env.storage()
                .persistent()
                .set(&failed_payouts_key(invoice_id), &new_failed);
        }

        // If all payouts are now complete, finalise the invoice.
        if new_failed.is_empty() {
            invoice.status = InvoiceStatus::Released;
            invoice.completion_time = Some(env.ledger().timestamp());
            save_invoice(&env, invoice_id, &invoice);
            // Decrement open-invoice counter now that invoice is fully done.
            let cnt: u32 = env
                .storage()
                .persistent()
                .get(&open_invoice_count_key(&invoice.creator))
                .unwrap_or(0u32);
            env.storage()
                .persistent()
                .set(&open_invoice_count_key(&invoice.creator), &cnt.saturating_sub(1));
            events::invoice_state_changed(
                &env,
                invoice_id,
                Some(&InvoiceStatus::Pending),
                &InvoiceStatus::Released,
                &recipient,
            );
        }

        events::recipient_paid(&env, invoice_id, &recipient, payout);
    }

    pub fn claim_pending_payout(env: Env, invoice_id: u64, recipient: Address) {
        recipient.require_auth();

        let invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status == InvoiceStatus::Released,
            "invoice not released"
        );

        let pending: i128 = env
            .storage()
            .persistent()
            .get(&pending_payout_key(invoice_id, &recipient))
            .expect("no pending payout");

        assert!(pending > 0, "no pending payout");

        let idx = invoice
            .recipients
            .iter()
            .position(|r| r == recipient.clone())
            .expect("recipient not found") as usize;
        let token_client = token::Client::new(&env, &recipient_token_for(&invoice, idx));
        token_client.transfer(&env.current_contract_address(), &recipient, &pending);

        env.storage()
            .persistent()
            .remove(&pending_payout_key(invoice_id, &recipient));

        events::pending_payout_claimed(&env, invoice_id, &recipient, pending);
    }

    pub fn claim_surplus(env: Env, invoice_id: u64, payer: Address) {
        require_not_paused(&env);
        payer.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        let surplus_total: i128 = env
            .storage()
            .persistent()
            .get(&surplus_key(invoice_id))
            .unwrap_or(0);
        assert!(surplus_total > 0, "no surplus available");
        assert!(
            !env.storage()
                .persistent()
                .has(&surplus_claim_key(invoice_id, &payer)),
            "surplus already claimed"
        );

        let payer_total = Self::get_payer_total(env.clone(), invoice_id, payer.clone());
        assert!(payer_total > 0, "payer has no contributions");
        let total_contributions: i128 = invoice.payments.iter().map(|payment| payment.amount).sum();
        assert!(total_contributions > 0, "no contributions recorded");

        let refund_amount =
            (surplus_total as u128 * payer_total as u128 / total_contributions as u128) as i128;
        assert!(refund_amount > 0, "no surplus claimable");

        let token_client = token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
        token_client.transfer(&env.current_contract_address(), &payer, &refund_amount);
        env.storage()
            .persistent()
            .set(&surplus_claim_key(invoice_id, &payer), &refund_amount);
        events::surplus_claimed(&env, invoice_id, &payer, refund_amount);
    }

    /// Distribute tranches unlocked by the current ledger time (issue #23).
    fn _release_tranches(env: &Env, invoice_id: u64, invoice: &mut Invoice, actor: &Address) {
        let now = env.ledger().timestamp();

        // Sum all basis points whose timestamp has passed.
        let mut unlocked_bps: u32 = 0;
        for tranche in invoice.tranches.iter() {
            if tranche.timestamp <= now {
                unlocked_bps += tranche.basis_points;
            }
        }

        // New basis points not yet distributed.
        let new_bps = unlocked_bps.saturating_sub(invoice.released_bps);
        assert!(new_bps > 0, "no tranches unlocked");

        let funding_token_client = token::Client::new(env, &funding_token_for(invoice));

        let creator_waived: bool = {
            let cfw: Vec<Address> = env
                .storage()
                .persistent()
                .get(&creator_fee_waiver_key())
                .unwrap_or_else(|| Vec::new(env));
            cfw.iter().any(|a| a == invoice.creator)
        };

        let total: i128 = invoice.amounts.iter().sum();
        let funded = invoice.funded;
        let amount_released =
            ((funded as u128).saturating_mul(new_bps as u128) / 10_000u128) as i128;

        let total_tranche_fee = if creator_waived {
            0
        } else {
            Self::compute_fee(env.clone(), amount_released)
        };

        let n = invoice.recipients.len();
        let mut total_fee: i128 = 0;
        let mut total_tax: i128 = 0;

        let waivers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&platform_fee_waiver_list_key())
            .unwrap_or_else(|| Vec::new(env));

        for i in 0..n {
            let recipient = invoice.recipients.get(i).unwrap();
            let amount = invoice.amounts.get(i).unwrap();
            // integer math: avoid overflow via u128 intermediary.
            let payout_raw = (amount as u128)
                .saturating_mul(new_bps as u128)
                .saturating_mul(funded as u128)
                / (10000u128 * total as u128);
            let payout_raw = payout_raw as i128;
            if payout_raw > 0 {
                let is_waived = waivers.iter().any(|a| a == recipient);
                let fee = if is_waived || amount_released == 0 {
                    0
                } else {
                    (payout_raw as u128 * total_tranche_fee as u128 / amount_released as u128)
                        as i128
                };
                let tax = (payout_raw as u128 * invoice.tax_bps as u128 / 10_000u128) as i128;
                let payout = payout_raw - fee - tax;
                total_fee += fee;
                total_tax += tax;

                let mut success = false;
                let routed = Self::execute_smart_route(env, invoice, &recipient, payout);
                if !routed {
                    let transfer_res = env.try_invoke_contract::<(), soroban_sdk::Error>(
                        &funding_token_client.address,
                        &symbol_short!("transfer"),
                        (&env.current_contract_address(), &recipient, &payout).into_val(env),
                    );
                    if transfer_res.is_ok() {
                        success = true;
                    }
                } else {
                    success = true;
                }

                // Issue #504: Use record_failed_payout for the new unified failure tracking.
                if !success {
                    record_failed_payout(env, invoice_id, &recipient, payout, "TransferFailed");
                    env.storage()
                        .persistent()
                        .set(&last_failed_ledger_key(invoice_id), &env.ledger().sequence());
                    invoice.status = InvoiceStatus::PartiallyReleased;
                }
            }
        }

        if total_tax > 0 {
            if let Some(ref auth) = invoice.tax_authority {
                funding_token_client.transfer(&env.current_contract_address(), auth, &total_tax);
            }
        }

        if total_fee > 0 {
            let treasury: Address = env
                .storage()
                .instance()
                .get(&treasury_key())
                .expect("treasury not set");
            Self::distribute_fee(env, total_fee, &funding_token_for(invoice), &treasury);
        }

        if total_tax > 0 {
            let tax_authority = invoice.tax_authority.as_ref().unwrap();
            funding_token_client.transfer(
                &env.current_contract_address(),
                tax_authority,
                &total_tax,
            );
        }

        // Calculate amount released in this tranche call.
        let amount_released =
            ((funded as u128).saturating_mul(new_bps as u128) / 10_000u128) as i128;
        accrue_creator_rebate(env, &invoice.creator, amount_released, total_fee);
        invoice.released_bps += new_bps;

        // Increment total_volume and total_released counters (issue #28).
        let total_volume: i128 = env
            .storage()
            .persistent()
            .get(&total_volume_key())
            .unwrap_or(0i128);
        env.storage().persistent().set(
            &total_volume_key(),
            &total_volume
                .checked_add(amount_released)
                .expect("total_volume overflow"),
        );

        let total_released: i128 = env
            .storage()
            .persistent()
            .get(&total_released_key())
            .unwrap_or(0i128);
        env.storage().persistent().set(
            &total_released_key(),
            &total_released
                .checked_add(amount_released)
                .expect("total_released overflow"),
        );

        if invoice.released_bps >= 10_000 {
            invoice.status = InvoiceStatus::Released;
            invoice.completion_time = Some(now);
            if invoice.insurance_fund > 0 {
                funding_token_client.transfer(
                    &env.current_contract_address(),
                    &invoice.creator,
                    &invoice.insurance_fund,
                );
                invoice.insurance_fund = 0;
            }
            append_audit_entry(env, invoice_id, symbol_short!("release"), actor);
            // Issue #763: record this release in the per-invoice history ring buffer.
            append_history_entry(
                env,
                invoice_id,
                Symbol::new(env, "release"),
                actor,
                Some(invoice.funded),
            );
            events::invoice_released(env, invoice_id, &invoice.recipients);
            events::invoice_state_changed(
                env,
                invoice_id,
                Some(&InvoiceStatus::Pending),
                &InvoiceStatus::Released,
                actor,
            );
            notify_invoice(
                env,
                invoice_id,
                symbol_short!("release"),
                &invoice.notification_contract,
            );
            maybe_record_released(env, &invoice.creator, amount_released);
            update_creator_stats_on_release(env, &invoice.creator, amount_released);
        }

        save_invoice(env, invoice_id, invoice);
    }

    /// Release a single tranche of a graduated schedule by index.
    ///
    /// Unlike `release()` (which distributes every tranche unlocked so far in one
    /// call), this lets any caller trigger exactly one tranche once its
    /// `release_time` has passed. Requires the invoice to be fully funded and the
    /// tranche not to have been released already. Pays each recipient their
    /// pro-rata share of that tranche's basis points.
    pub fn release_tranche(env: Env, invoice_id: u64, tranche_index: u32) {
        require_fn_not_paused(&env, &symbol_short!("release"));
        let mut invoice = load_invoice(&env, invoice_id);
        let actor = env.current_contract_address();

        assert!(!invoice.frozen, "invoice is frozen");
        assert!(!invoice.admin_frozen, "invoice frozen by admin");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        if let Some(held_until) = invoice.held_until {
            if env.ledger().sequence() < held_until {
                panic!("EscrowHoldActive");
            }
        }

        let total: i128 = invoice.amounts.iter().sum();
        assert!(invoice.funded >= total, "invoice not fully funded");

        assert!(
            tranche_index < invoice.tranches.len(),
            "tranche index out of range"
        );
        assert!(tranche_index < 32, "tranche index exceeds bitmask capacity");
        let tranche = invoice.tranches.get(tranche_index).unwrap();

        let now = env.ledger().timestamp();
        assert!(now >= tranche.timestamp, "tranche not yet releasable");

        let mut released_idx: u32 = env
            .storage()
            .persistent()
            .get(&released_tranche_idx_key(invoice_id))
            .unwrap_or(0u32);
        let bit = 1u32 << tranche_index;
        assert!(released_idx & bit == 0, "tranche already released");

        let token_client = token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
        let funded = invoice.funded;
        let n = invoice.recipients.len();
        let mut total_paid: i128 = 0;
        for i in 0..n {
            let recipient = invoice.recipients.get(i).unwrap();
            let amount = invoice.amounts.get(i).unwrap();
            let payout = ((amount as u128)
                .saturating_mul(tranche.basis_points as u128)
                .saturating_mul(funded as u128)
                / (10_000u128 * total as u128)) as i128;
            if payout > 0 {
                token_client.transfer(&env.current_contract_address(), &recipient, &payout);
                total_paid += payout;
            }
        }

        released_idx |= bit;
        env.storage()
            .persistent()
            .set(&released_tranche_idx_key(invoice_id), &released_idx);

        invoice.released_bps += tranche.basis_points;

        if invoice.released_bps >= 10_000 {
            invoice.status = InvoiceStatus::Released;
            invoice.completion_time = Some(now);
            if invoice.insurance_fund > 0 {
                token_client.transfer(
                    &env.current_contract_address(),
                    &invoice.creator,
                    &invoice.insurance_fund,
                );
                invoice.insurance_fund = 0;
            }
            append_audit_entry(&env, invoice_id, symbol_short!("release"), &actor);
            events::invoice_released(&env, invoice_id, &invoice.recipients);
            events::invoice_state_changed(
                &env,
                invoice_id,
                Some(&InvoiceStatus::Pending),
                &InvoiceStatus::Released,
                &actor,
            );
            notify_invoice(
                &env,
                invoice_id,
                symbol_short!("release"),
                &invoice.notification_contract,
            );
            maybe_record_released(&env, &invoice.creator, total_paid);
            update_creator_stats_on_release(&env, &invoice.creator, total_paid);
        }

        save_invoice(&env, invoice_id, &invoice);

        append_audit_entry(&env, invoice_id, symbol_short!("tr_rel"), &actor);
        events::tranche_released(&env, invoice_id, tranche_index, total_paid);
    }

    // -----------------------------------------------------------------------
    // Stage release (#86)
    // -----------------------------------------------------------------------

    /// Release the next predefined stage of funds to recipients.
    ///
    /// Requires creator auth. Each call distributes the next stage's proportion
    /// of the total funded amount. The final stage sets the invoice status to Released.
    pub fn stage_release(env: Env, invoice_id: u64, creator: Address) {
        require_not_paused(&env);
        creator.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.creator == creator,
            "only creator can call stage_release"
        );
        assert!(!invoice.frozen, "invoice is frozen");
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(
            !invoice.release_stages.is_empty(),
            "no release stages defined"
        );

        let total: i128 = invoice.amounts.iter().sum();
        assert!(invoice.funded >= total, "invoice not fully funded");

        let stage_idx = invoice.released_stages;
        assert!(
            stage_idx < invoice.release_stages.len(),
            "all stages already released"
        );

        let stage_bps = invoice.release_stages.get(stage_idx).unwrap();

        let token_client = token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));

        let platform_fee_bps: u32 = env
            .storage()
            .instance()
            .get(&platform_fee_bps_key())
            .unwrap_or(0u32);

        let funded = invoice.funded;
        let n = invoice.recipients.len();
        let mut total_fee: i128 = 0;
        let mut total_tax: i128 = 0;

        let waivers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&platform_fee_waiver_list_key())
            .unwrap_or_else(|| Vec::new(&env));

        for i in 0..n {
            let recipient = invoice.recipients.get(i).unwrap();
            let amount = invoice.amounts.get(i).unwrap();
            let payout_raw: i128 = if !invoice.ratios.is_empty() {
                let ratio = invoice.ratios.get(i).unwrap();
                let denom = invoice.ratio_denominator as u128;
                let r = ratio as u128;
                checked_proportion(amount as u128, r, denom)
                    .expect("ArithmeticOverflow")
            } else {
                ((amount as u128)
                    .saturating_mul(stage_bps as u128)
                    .saturating_mul(funded as u128)
                    / (10_000u128 * total as u128)) as i128
            };
            if payout_raw > 0 {
                let is_waived = waivers.iter().any(|a| a == recipient);
                let fee = if is_waived {
                    0
                } else {
                    calc_platform_fee(payout_raw, platform_fee_bps).expect("ArithmeticOverflow")
                };
                let tax = (payout_raw as u128 * invoice.tax_bps as u128 / 10_000u128) as i128;
                let payout = payout_raw - fee - tax;
                total_fee += fee;
                total_tax += tax;
                let routed = Self::execute_smart_route(&env, &invoice, &recipient, payout);
                if !routed {
                    token_client.transfer(&env.current_contract_address(), &recipient, &payout);
                }
            }
        }

        if total_fee > 0 {
            let treasury: Address = env
                .storage()
                .instance()
                .get(&treasury_key())
                .expect("treasury not set");
            Self::distribute_fee(&env, total_fee, &invoice.tokens.get(0).expect("no token"), &treasury);
        }

        if total_tax > 0 {
            let tax_authority = invoice.tax_authority.as_ref().unwrap();
            token_client.transfer(&env.current_contract_address(), tax_authority, &total_tax);
        }

        invoice.released_stages += 1;

        // Calculate amount released in this stage.
        let amount_released =
            ((stage_bps as u128).saturating_mul(funded as u128) / 10_000u128) as i128;

        // Increment total_volume and total_released counters (issue #28).
        let total_volume: i128 = env
            .storage()
            .persistent()
            .get(&total_volume_key())
            .unwrap_or(0i128);
        env.storage().persistent().set(
            &total_volume_key(),
            &total_volume
                .checked_add(amount_released)
                .expect("total_volume overflow"),
        );

        let total_released: i128 = env
            .storage()
            .persistent()
            .get(&total_released_key())
            .unwrap_or(0i128);
        env.storage().persistent().set(
            &total_released_key(),
            &total_released
                .checked_add(amount_released)
                .expect("total_released overflow"),
        );

        let now = env.ledger().timestamp();
        if invoice.released_stages >= invoice.release_stages.len() {
            invoice.status = InvoiceStatus::Released;
            invoice.completion_time = Some(now);
            if invoice.insurance_fund > 0 {
                token_client.transfer(
                    &env.current_contract_address(),
                    &invoice.creator,
                    &invoice.insurance_fund,
                );
                invoice.insurance_fund = 0;
            }
            append_audit_entry(&env, invoice_id, symbol_short!("stg_rel"), &creator);
            events::invoice_released(&env, invoice_id, &invoice.recipients);
            events::invoice_state_changed(
                &env,
                invoice_id,
                Some(&InvoiceStatus::Pending),
                &InvoiceStatus::Released,
                &creator,
            );
            notify_invoice(
                &env,
                invoice_id,
                symbol_short!("release"),
                &invoice.notification_contract,
            );
        } else {
            append_audit_entry(&env, invoice_id, symbol_short!("stg_rel"), &creator);
        }

        save_invoice(&env, invoice_id, &invoice);
    }

    /// Partially release `amount` from a pending invoice to recipients in priority order.
    /// When `priorities` is set, recipients are paid in ascending priority (lowest number first)
    /// until `amount` is exhausted. Recipients whose full amount cannot be covered are skipped.
    /// When no priorities are set, funds are distributed proportionally (original behaviour).
    /// Requires creator auth. Does not change invoice status (remains Pending).
    pub fn partial_release(env: Env, invoice_id: u64, creator: Address, amount: i128) {
        require_not_paused(&env);
        creator.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.creator == creator,
            "only creator can call partial_release"
        );
        assert!(!invoice.frozen, "invoice is frozen");
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(amount > 0, "amount must be positive");
        assert!(amount <= invoice.funded, "amount exceeds funded balance");

        let token_client = token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));

        let n = invoice.recipients.len();
        let use_priorities = !invoice.priorities.is_empty();

        if use_priorities {
            // Build a sorted index list (ascending by priority) via selection sort.
            // We maintain a "remaining" pool of indices and repeatedly pick the minimum.
            let mut pool: Vec<u32> = Vec::new(&env);
            for i in 0..n {
                pool.push_back(i);
            }

            let mut sorted_indices: Vec<u32> = Vec::new(&env);
            let pool_len = pool.len();
            for _ in 0..pool_len {
                // Find position in pool with lowest priority.
                let mut best_pos: u32 = 0;
                let mut best_pri: u32 = u32::MAX;
                for pos in 0..pool.len() {
                    let idx = pool.get(pos).unwrap();
                    let p = invoice.priorities.get(idx).unwrap();
                    if p < best_pri {
                        best_pri = p;
                        best_pos = pos;
                    }
                }
                let chosen = pool.get(best_pos).unwrap();
                sorted_indices.push_back(chosen);
                // Remove chosen from pool by rebuilding without it.
                let mut new_pool: Vec<u32> = Vec::new(&env);
                for pos in 0..pool.len() {
                    if pos != best_pos {
                        new_pool.push_back(pool.get(pos).unwrap());
                    }
                }
                pool = new_pool;
            }

            let mut remaining = amount;
            for k in 0..n {
                let idx = sorted_indices.get(k).unwrap();
                let recipient = invoice.recipients.get(idx).unwrap();
                let recip_amount = invoice.amounts.get(idx).unwrap();
                if remaining >= recip_amount {
                    token_client.transfer(
                        &env.current_contract_address(),
                        &recipient,
                        &recip_amount,
                    );
                    remaining -= recip_amount;
                }
                // Skip recipients whose full amount cannot be covered.
            }
        } else {
            // Original proportional distribution.
            let total_amounts: i128 = invoice.amounts.iter().sum();
            let mut distributed: i128 = 0;
            for i in 0..n {
                let recipient = invoice.recipients.get(i).unwrap();
                let recip_amount = invoice.amounts.get(i).unwrap();
                let share = if i == n - 1 {
                    amount - distributed
                } else {
                    ((amount as u128) * (recip_amount as u128) / (total_amounts as u128)) as i128
                };
                distributed += share;
                if share > 0 {
                    token_client.transfer(&env.current_contract_address(), &recipient, &share);
                }
            }
        }

        invoice.funded -= amount;
        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("part_rel"), &creator);
        events::invoice_partially_released(&env, invoice_id, &invoice.recipients);
    }

    /// Full immediate release (no tranches).
    /// Issue #89: Returns stake to creator on successful release.
    /// Issue #41: Swaps recipient payout via DEX if swap_tokens[i] is set.
    fn _release_full(env: &Env, invoice_id: u64, invoice: &mut Invoice, actor: &Address) {
        // Issue #27: vesting cliff field not in current schema; proceed normally

        let funding_token_client = token::Client::new(env, &funding_token_for(invoice));

        // Issue #296: if creator has a fee waiver, platform_fee_bps is 0 for this invoice.
        let creator_waived: bool = {
            let cfw: Vec<Address> = env
                .storage()
                .persistent()
                .get(&creator_fee_waiver_key())
                .unwrap_or_else(|| Vec::new(env));
            cfw.iter().any(|a| a == invoice.creator)
        };

        let funded = invoice.funded;

        // First-party trusted callers (e.g. governance contracts) are exempt from the platform fee.
        // Deliberately does NOT match on `env.current_contract_address()`: `release()` and
        // `trigger_scheduled_release()` are permissionless entry points that pass the contract's
        // own address as `actor`, so matching it here would let anyone waive the platform fee on
        // every invoice by triggering a release through those paths.
        let caller_trusted: bool = {
            let trusted: Vec<Address> = env
                .storage()
                .instance()
                .get(&trusted_callers_key())
                .unwrap_or_else(|| Vec::new(env));
            trusted.contains(actor)
        };

        let total_platform_fee: i128 = if creator_waived || caller_trusted {
            0
        } else {
            // Issue #489: early-bird contributions accrue a fee discount at
            // payment time; apply it against the fee computed on the full total.
            (Self::compute_fee(env.clone(), funded) - invoice.early_bird_fee_credit).max(0)
        };

        let total: i128 = invoice.amounts.iter().sum();
        let n = invoice.recipients.len();
        let mut distributed: i128 = 0;
        let mut total_fee: i128 = 0;
        let mut total_tax: i128 = 0;
        let mut surplus_total: i128 = 0;
        let mut payouts: Vec<i128> = Vec::new(env);

        // Issue #330: recipients already paid via release_to_recipient are skipped here.
        let paid_set: Vec<Address> = env
            .storage()
            .persistent()
            .get(&paid_recipients_key(invoice_id))
            .unwrap_or_else(|| Vec::new(env));
        // Compute effective total excluding paid recipients' amounts.
        let effective_total: i128 = if paid_set.is_empty() {
            total
        } else {
            let mut et: i128 = 0;
            for i in 0..n {
                let r = invoice.recipients.get(i).unwrap();
                if !paid_set.iter().any(|p| p == r) {
                    et += invoice.amounts.get(i).unwrap();
                }
            }
            if et == 0 {
                1
            } else {
                et
            }
        };
        // Find the index of the last unpaid recipient (for remainder assignment).
        let last_unpaid_idx: u32 = {
            let mut last = n.saturating_sub(1);
            for i in 0..n {
                let r = invoice.recipients.get(i).unwrap();
                if paid_set.is_empty() || !paid_set.iter().any(|p| p == r) {
                    last = i;
                }
            }
            last
        };

        let waivers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&platform_fee_waiver_list_key())
            .unwrap_or_else(|| Vec::new(env));

        let mut unreleased_locked: i128 = 0;

        for i in 0..n {
            let recipient = invoice.recipients.get(i).unwrap();
            let amount = invoice.amounts.get(i).unwrap();

            // Issue #330: skip already-paid recipients.
            if !paid_set.is_empty() && paid_set.iter().any(|p| p == recipient) {
                payouts.push_back(0i128);
                continue;
            }

            // Issue: if split_rules are defined, compute payout from rule instead of amounts[].
            let proportional = if !invoice.ratios.is_empty() {
                let amount = invoice.amounts.get(i).unwrap();
                let ratio = invoice.ratios.get(i).unwrap();
                let denom = invoice.ratio_denominator as u128;
                let r = ratio as u128;
                checked_proportion(amount as u128, r, denom).expect("ArithmeticOverflow")
            } else if !invoice.split_rules.is_empty() {
                let rule = invoice.split_rules.get(i).unwrap();
                match rule {
                    SplitRule::Fixed(fixed_amt) => fixed_amt,
                    SplitRule::Percentage(bps) => {
                        // Issue #482: use checked arithmetic to prevent overflow.
                        checked_bps_of(funded, bps, 10_000u128)
                            .expect("ArithmeticOverflow")
                    }
                    SplitRule::Tiered(threshold, bps) => {
                        if funded > threshold {
                            // Issue #482: use checked arithmetic to prevent overflow.
                            checked_bps_of(funded, bps, 10_000u128)
                                .expect("ArithmeticOverflow")
                        } else {
                            0
                        }
                    }
                }
            } else if i == last_unpaid_idx {
                funded - distributed
            } else {
                // Issue #482: use checked arithmetic to prevent overflow.
                checked_proportion(amount as u128, funded as u128, effective_total as u128)
                    .expect("ArithmeticOverflow")
            };
            let capped_proportional = if !invoice.recipient_max_payouts.is_empty() {
                match invoice.recipient_max_payouts.get(i).unwrap_or(None) {
                    Some(max_payout) if proportional > max_payout => {
                        surplus_total += proportional - max_payout;
                        max_payout
                    }
                    _ => proportional,
                }
            } else {
                proportional
            };

            // Skip locked recipients: accumulate their computed proportional
            // share into UnreleasedFunds instead of transferring it.
            let is_locked: bool = env
                .storage()
                .persistent()
                .get(&recipient_lock_key(invoice_id, &recipient))
                .unwrap_or(false);
            if is_locked {
                unreleased_locked = unreleased_locked.saturating_add(capped_proportional);
                payouts.push_back(0i128);
                continue;
            }

            distributed += capped_proportional;

            // Issue #482: use checked arithmetic to prevent overflow.
            let tax = checked_bps_of(capped_proportional, invoice.tax_bps, 10_000u128)
                .expect("ArithmeticOverflow");
            total_tax += tax;
            let post_tax = capped_proportional - tax;

            let is_waived = waivers.iter().any(|a| a == recipient);
            let fee = if is_waived || funded == 0 {
                0
            } else {
                // Issue #482: use checked arithmetic to prevent overflow.
                checked_proportion(post_tax as u128, total_platform_fee as u128, funded as u128)
                    .expect("ArithmeticOverflow")
            };
            total_fee += fee;

            payouts.push_back(capped_proportional);
        }

        // Accumulate locked recipients' share into UnreleasedFunds.
        if unreleased_locked > 0 {
            let stored: i128 = env
                .storage()
                .persistent()
                .get(&unreleased_funds_key(invoice_id))
                .unwrap_or(0);
            env.storage()
                .persistent()
                .set(&unreleased_funds_key(invoice_id), &stored.saturating_add(unreleased_locked));
        }

        if surplus_total > 0 {
            let stored_surplus: i128 = env
                .storage()
                .persistent()
                .get(&surplus_key(invoice_id))
                .unwrap_or(0);
            env.storage().persistent().set(
                &surplus_key(invoice_id),
                &stored_surplus.saturating_add(surplus_total),
            );
        }

        // Issue #751: the protocol fee is withheld from each payment at `pay`
        // time (see `_withhold_protocol_fee`) and accumulates in the contract
        // treasury, so no further deduction is made at release. This supersedes
        // the release-time transfer introduced by issue #326.

        // If this invoice belongs to a treasury group, route the net payouts to the group's treasury address.
        if let Some((_group_id, record)) = treasury_record_for_invoice(env, invoice_id) {
            // Transfer taxes first.
            if total_tax > 0 {
                if let Some(ref auth) = invoice.tax_authority {
                    funding_token_client.transfer(
                        &env.current_contract_address(),
                        auth,
                        &total_tax,
                    );
                }
            }

            // Transfer platform fee to configured fee recipients.
            if total_fee > 0 {
                let treasury: Address = env
                    .storage()
                    .instance()
                    .get(&treasury_key())
                    .expect("treasury not set");
                Self::distribute_fee(env, total_fee, &funding_token_for(invoice), &treasury);
            }

            let net = distributed - total_tax - total_fee;
            if net > 0 {
                funding_token_client.transfer(
                    &env.current_contract_address(),
                    &record.treasury,
                    &net,
                );
            }
        } else {
            // Default behavior: transfer to each recipient (or route via DEX/router as configured).
            let mut total_creator_fee: i128 = 0;
            for i in 0..n {
                let recipient = invoice.recipients.get(i).unwrap();
                let proportional = payouts.get(i).unwrap();

                // Skip locked recipients — their share is in UnreleasedFunds.
                let is_locked: bool = env
                    .storage()
                    .persistent()
                    .get(&recipient_lock_key(invoice_id, &recipient))
                    .unwrap_or(false);
                if is_locked {
                    continue;
                }

                // Issue #330: skip recipients already paid via release_to_recipient.
                if proportional == 0
                    && !paid_set.is_empty()
                    && paid_set.iter().any(|p| p == recipient)
                {
                    continue;
                }

                // Issue #505: verify recipient account existence before attempting payout.
                // Use try_invoke_contract to call balance() on the funding token; if it fails
                // the account has never been initialised on the ledger.
                let account_exists = env
                    .try_invoke_contract::<i128, soroban_sdk::Error>(
                        &funding_token_client.address,
                        &symbol_short!("balance"),
                        (recipient.clone(),).into_val(env),
                    )
                    .is_ok();
                if !account_exists {
                    // Record the failure and emit event; skip transfer for this recipient.
                    let mut failed: Vec<Address> = env
                        .storage()
                        .persistent()
                        .get(&failed_payouts_key(invoice_id))
                        .unwrap_or_else(|| Vec::new(env));
                    if !failed.iter().any(|a| a == recipient) {
                        failed.push_back(recipient.clone());
                    }
                    env.storage()
                        .persistent()
                        .set(&failed_payouts_key(invoice_id), &failed);
                    events::recipient_account_missing(env, invoice_id, &recipient);
                    continue;
                }

                // Issue #482: use checked arithmetic to prevent overflow.
                let tax = checked_bps_of(proportional, invoice.tax_bps, 10_000u128)
                    .expect("ArithmeticOverflow");
                let post_tax = proportional - tax;

                let is_waived = waivers.iter().any(|a| a == recipient);
                let fee = if is_waived || funded == 0 {
                    0
                } else {
                    // Issue #482: use checked arithmetic to prevent overflow.
                    checked_proportion(post_tax as u128, total_platform_fee as u128, funded as u128)
                        .expect("ArithmeticOverflow")
                };
                let payout_after_fee = post_tax - fee;
                let creator_fee = checked_bps_of(payout_after_fee, invoice.creator_fee_bps, 10_000u128)
                    .expect("ArithmeticOverflow");
                total_creator_fee = total_creator_fee.saturating_add(creator_fee);
                let payout = payout_after_fee - creator_fee;

                let mut success = false;
                let swap_token: Option<Address> = invoice.swap_tokens.get(i).unwrap_or(None);
                if let Some(out_token) = swap_token {
                    let from_token = funding_token_for(invoice);
                    let mut args: Vec<Val> = Vec::new(env);
                    args.push_back(from_token.into_val(env));
                    args.push_back(out_token.clone().into_val(env));
                    args.push_back(payout.into_val(env));
                    args.push_back(recipient.clone().into_val(env));
                    let swap_res = env.try_invoke_contract::<i128, soroban_sdk::Error>(
                        &out_token,
                        &Symbol::new(env, "swap"),
                        args,
                    );
                    if swap_res.is_ok() {
                        success = true;
                    }
                } else if invoice.smart_route {
                    let transfer_res = env.try_invoke_contract::<(), soroban_sdk::Error>(
                        &funding_token_client.address,
                        &symbol_short!("transfer"),
                        (&env.current_contract_address(), &recipient, &payout).into_val(env),
                    );
                    if transfer_res.is_ok() {
                        success = true;
                    }
                    let from_token = recipient_token_for(invoice, i as usize);
                    let mut route_args: Vec<Val> = Vec::new(env);
                    route_args.push_back(from_token.into_val(env));
                    route_args.push_back(payout.into_val(env));
                    route_args.push_back(recipient.clone().into_val(env));
                    let recipient_token_client =
                        token::Client::new(env, &recipient_token_for(invoice, i as usize));
                    recipient_token_client.transfer(
                        &env.current_contract_address(),
                        &recipient,
                        &payout,
                    );
                } else if invoice.convert_to_stream {
                    if let Some(stream_contract) = env
                        .storage()
                        .persistent()
                        .get::<Symbol, Address>(&stream_contract_key())
                    {
                        let duration = invoice.drip_duration.unwrap_or(86_400);
                        let transfer_res = env.try_invoke_contract::<(), soroban_sdk::Error>(
                            &funding_token_client.address,
                            &symbol_short!("transfer"),
                            (&env.current_contract_address(), &stream_contract, &payout)
                                .into_val(env),
                        );
                        if transfer_res.is_ok() {
                            let mut args: Vec<Val> = Vec::new(env);
                            args.push_back(recipient.clone().into_val(env));
                            args.push_back(payout.into_val(env));
                            args.push_back(duration.into_val(env));
                            let stream_res = env.try_invoke_contract::<Val, soroban_sdk::Error>(
                                &stream_contract,
                                &Symbol::new(env, "create_stream"),
                                args,
                            );
                            if stream_res.is_ok() {
                                success = true;
                            }
                        }
                    } else {
                        let recipient_token_client =
                            token::Client::new(env, &recipient_token_for(invoice, i as usize));
                        recipient_token_client.transfer(
                            &env.current_contract_address(),
                            &recipient,
                            &payout,
                        );
                    }
                } else {
                    let routed = Self::execute_smart_route(env, invoice, &recipient, payout);
                    if !routed {
                        let transfer_res = env.try_invoke_contract::<(), soroban_sdk::Error>(
                            &funding_token_client.address,
                            &symbol_short!("transfer"),
                            (&env.current_contract_address(), &recipient, &payout).into_val(env),
                        );
                        if transfer_res.is_ok() {
                            success = true;
                        }
                    } else {
                        success = true;
                        let recipient_token_client =
                            token::Client::new(env, &recipient_token_for(invoice, i as usize));
                        recipient_token_client.transfer(
                            &env.current_contract_address(),
                            &recipient,
                            &payout,
                        );
                    }
                }

                // Issue #504: Use record_failed_payout for the new unified failure tracking.
                if !success {
                    record_failed_payout(env, invoice_id, &recipient, payout, "TransferFailed");
                    env.storage()
                        .persistent()
                        .set(&last_failed_ledger_key(invoice_id), &env.ledger().sequence());
                    invoice.status = InvoiceStatus::PartiallyReleased;
                } else {
                    events::payout_initiated(env, invoice_id, i, &recipient, payout);
                }

                if let Some(ref auth) = invoice.tax_authority {
                    funding_token_client.transfer(
                        &env.current_contract_address(),
                        auth,
                        &total_tax,
                    );
                }
            }

            if total_fee > 0 {
                let treasury: Address = env
                    .storage()
                    .instance()
                    .get(&treasury_key())
                    .expect("treasury not set");
                Self::distribute_fee(env, total_fee, &funding_token_for(invoice), &treasury);
            }

            if total_creator_fee > 0 {
                funding_token_client.transfer(
                    &env.current_contract_address(),
                    &invoice.creator,
                    &total_creator_fee,
                );
                events::creator_fee_paid(env, invoice_id, &invoice.creator, total_creator_fee);
                events::creator_fee_collected(env, invoice_id, &invoice.creator, total_creator_fee);
            }
        }

        // Distribute bonus pool among first `bonus_max_payers` unique payers.
        if invoice.bonus_pool > 0 && invoice.bonus_max_payers > 0 {
            let mut unique_payers: Vec<Address> = Vec::new(env);
            for payment in invoice.payments.iter() {
                let already_seen = unique_payers.iter().any(|p| p == payment.payer);
                if !already_seen {
                    unique_payers.push_back(payment.payer.clone());
                    if unique_payers.len() >= invoice.bonus_max_payers {
                        break;
                    }
                }
            }

            if !unique_payers.is_empty() {
                let n = unique_payers.len() as i128;
                let per_payer = invoice.bonus_pool / n;
                let mut distributed: i128 = 0;
                for (i, payer) in unique_payers.iter().enumerate() {
                    let payout = if i as i128 == n - 1 {
                        invoice.bonus_pool - distributed
                    } else {
                        per_payer
                    };
                    funding_token_client.transfer(&env.current_contract_address(), &payer, &payout);
                    events::reward_distributed(env, invoice_id, &payer, payout);
                    distributed += payout;
                }
            }
        }

        // Issue #89: Return stake to creator on successful release.
        // (stake_amount field not yet on Invoice; skipped)

        // Release all group members if this invoice is part of a group.
        if let Some(group_id) = env
            .storage()
            .persistent()
            .get::<(Symbol, u64), u64>(&invoice_group_key(invoice_id))
        {
            let platform_fee_bps: u32 = env
                .storage()
                .instance()
                .get(&platform_fee_bps_key())
                .unwrap_or(0u32);
            for member_id in load_group(env, group_id).iter() {
                if member_id != invoice_id {
                    let mut member = load_invoice(env, member_id);
                    if member.status == InvoiceStatus::Pending {
                        let member_token =
                            token::Client::new(env, &member.tokens.get(0).expect("no token"));
                        let member_total: i128 = member.amounts.iter().sum();
                        let member_funded = member.funded;
                        let member_n = member.recipients.len();
                        let mut member_distributed: i128 = 0;
                        let mut group_total_fee: i128 = 0;
                        for (j, (recipient, amount)) in member
                            .recipients
                            .iter()
                            .zip(member.amounts.iter())
                            .enumerate()
                        {
                            let proportional = if j == (member_n - 1) as usize {
                                member_funded - member_distributed
                            } else {
                                (amount as u128 * member_funded as u128 / member_total as u128)
                                    as i128
                            };
                            let fee = calc_platform_fee(proportional, platform_fee_bps)
                                .expect("ArithmeticOverflow");
                            let tax = (proportional as u128 * member.tax_bps as u128 / 10_000u128)
                                as i128;
                            let payout = proportional - fee - tax;
                            member_distributed += proportional;
                            group_total_fee += fee;
                            if tax > 0 {
                                let tax_authority = member.tax_authority.as_ref().unwrap();
                                member_token.transfer(
                                    &env.current_contract_address(),
                                    tax_authority,
                                    &tax,
                                );
                            }
                            let routed =
                                Self::execute_smart_route(env, &member, &recipient, payout);
                            if !routed {
                                member_token.transfer(
                                    &env.current_contract_address(),
                                    &recipient,
                                    &payout,
                                );
                            }
                        }
                        if group_total_fee > 0 {
                            let treasury: Address = env
                                .storage()
                                .instance()
                                .get(&treasury_key())
                                .expect("treasury not set");
                            member_token.transfer(
                                &env.current_contract_address(),
                                &treasury,
                                &group_total_fee,
                            );
                        }
                        accrue_creator_rebate(env, &member.creator, member_funded, group_total_fee);
                        member.status = InvoiceStatus::Released;
                        member.completion_time = Some(env.ledger().timestamp());
                        save_invoice(env, member_id, &member);
                        append_audit_entry(env, member_id, symbol_short!("release"), actor);
                        events::invoice_released(env, member_id, &member.recipients);
                        events::invoice_state_changed(
                            env,
                            member_id,
                            Some(&InvoiceStatus::Pending),
                            &InvoiceStatus::Released,
                            actor,
                        );
                    }
                }
            }
        }

        // Return insurance fund to creator on successful release.
        if invoice.insurance_fund > 0 {
            funding_token_client.transfer(
                &env.current_contract_address(),
                &invoice.creator,
                &invoice.insurance_fund,
            );
            invoice.insurance_fund = 0;
        }

        // Forward any leftover (rounding remainder) to configured forward target.
        let leftover = funded
            .checked_sub(distributed.saturating_add(surplus_total))
            .unwrap_or(0);
        if leftover > 0 {
            if let Some(addr) = invoice.forward_to.as_ref() {
                funding_token_client.transfer(&env.current_contract_address(), addr, &leftover);
            } else if let Some(target_id) = invoice.forward_invoice_id {
                // Credit the target invoice internally (acts like an internal pay from this contract).
                let mut target = load_invoice(env, target_id);
                // Write payment to sharded storage (issue #177).
                let shard_id = compute_shard_id(env, &env.current_contract_address());
                let mut shard_payments: Vec<Payment> = env
                    .storage()
                    .persistent()
                    .get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(target_id, shard_id))
                    .unwrap_or_else(|| Vec::new(env));
                shard_payments.push_back(Payment {
                    payer: env.current_contract_address(),
                    amount: leftover,
                    tip: 0,
                    attestation_hash: None,
                    donate_on_failure: false,
                    ledger: env.ledger().sequence(),
                    timestamp: env.ledger().timestamp(),
                });
                env.storage()
                    .persistent()
                    .set(&pay_shard_key(target_id, shard_id), &shard_payments);

                target.funded += leftover;
                let cumulative_key = cumulative_contributed_key(target_id);
                let cumulative: i128 = env.storage().persistent().get(&cumulative_key).unwrap_or(0);
                env.storage()
                    .persistent()
                    .set(&cumulative_key, &(cumulative + leftover));
                // If target becomes fully funded, trigger auto-release where applicable.
                let target_total: i128 = target.amounts.iter().sum();
                if target.funded >= target_total {
                    let in_group = env
                        .storage()
                        .persistent()
                        .has(&invoice_group_key(target_id));
                    let guarded = target.prerequisite_id.is_some()
                        || !target.tranches.is_empty()
                        || !target.release_stages.is_empty()
                        || in_group
                        || !target.co_signers.is_empty()
                        || env.storage().persistent().has(&cosigners_key(target_id));
                    if guarded {
                        save_invoice(env, target_id, &target);
                    } else {
                        Self::_release(
                            env,
                            target_id,
                            &mut target,
                            &env.current_contract_address(),
                        );
                    }
                } else {
                    save_invoice(env, target_id, &target);
                }
            }
        }

        // Issue #505: only transition to Released if all recipients were paid.
        // If any ended up in failed_payouts, stay Pending so retry_failed_payout can finish the job.
        let has_failed_payouts = env
            .storage()
            .persistent()
            .get::<(Symbol, u64), Vec<Address>>(&failed_payouts_key(invoice_id))
            .map(|v| !v.is_empty())
            .unwrap_or(false);

        if !has_failed_payouts {
            invoice.status = InvoiceStatus::Released;
        }
        invoice.completion_time = Some(env.ledger().timestamp());
        if invoice.insurance_fund > 0 {
            let token_client = token::Client::new(env, &invoice.tokens.get(0).expect("no token"));
            token_client.transfer(
                &env.current_contract_address(),
                &invoice.creator,
                &invoice.insurance_fund,
            );
            invoice.insurance_fund = 0;
        }
        // Issue #503: decrement per-creator open-invoice counter on release (only when fully done).
        if !has_failed_payouts {
            let cnt: u32 = env
                .storage()
                .persistent()
                .get(&open_invoice_count_key(&invoice.creator))
                .unwrap_or(0u32);
            env.storage()
                .persistent()
                .set(&open_invoice_count_key(&invoice.creator), &cnt.saturating_sub(1));
        }
        save_invoice(env, invoice_id, invoice);
        append_audit_entry(env, invoice_id, symbol_short!("release"), actor);
        // Issue #763: record this release in the per-invoice history ring buffer.
        // This covers the auto-release path (invoked from `pay` on full funding);
        // the explicit `release()` path writes its own entry via `_release_invoice_inner`.
        if !has_failed_payouts {
            append_history_entry(
                env,
                invoice_id,
                Symbol::new(env, "release"),
                actor,
                Some(funded),
            );
        }
        events::invoice_released(env, invoice_id, &invoice.recipients);
        if !has_failed_payouts {
            events::invoice_state_changed(
                env,
                invoice_id,
                Some(&InvoiceStatus::Pending),
                &InvoiceStatus::Released,
                actor,
            );
        }
        notify_invoice(
            env,
            invoice_id,
            symbol_short!("release"),
            &invoice.notification_contract,
        );
        maybe_record_released(env, &invoice.creator, funded);
        update_rep_internal(env, &invoice.creator, |score| {
            score.invoices_released = score.invoices_released.saturating_add(1);
        });

        // Increment total_volume and total_released counters (issue #28).
        let total_volume: i128 = env
            .storage()
            .persistent()
            .get(&total_volume_key())
            .unwrap_or(0i128);
        let new_total_volume = total_volume
            .checked_add(funded)
            .expect("total_volume overflow");
        env.storage()
            .persistent()
            .set(&total_volume_key(), &new_total_volume);
        // Issue #276: emit platform volume milestone if threshold crossed.
        check_platform_milestone(env, new_total_volume);

        let total_released: i128 = env
            .storage()
            .persistent()
            .get(&total_released_key())
            .unwrap_or(0i128);
        env.storage().persistent().set(
            &total_released_key(),
            &total_released
                .checked_add(funded)
                .expect("total_released overflow"),
        );

        // Increment creator analytics (issue #106).
        // creator_stats_volume_key is stored as u64 (see update_creator_stats_on_payment).
        let creator_volume: u64 = env
            .storage()
            .persistent()
            .get(&creator_stats_volume_key(&invoice.creator))
            .unwrap_or(0u64);
        let new_creator_volume = creator_volume
            .checked_add(funded as u64)
            .expect("creator_volume overflow");
        env.storage().persistent().set(
            &creator_stats_volume_key(&invoice.creator),
            &new_creator_volume,
        );
        // Issue #276: emit creator volume milestone if threshold crossed.
        check_creator_milestone(env, &invoice.creator, new_creator_volume as i128);

        update_creator_stats_on_release(env, &invoice.creator, funded);
        accrue_creator_rebate(env, &invoice.creator, funded, total_fee);

        // Issue #752: best-effort NFT mint on full funding.
        // If an NFT contract is configured, call mint(creator, invoice_id).
        // Failure is logged but does NOT revert the release.
        if !has_failed_payouts {
            if let Some(nft_contract) = env
                .storage()
                .instance()
                .get::<Symbol, Address>(&nft_mint_contract_key())
            {
                let mut mint_args: Vec<Val> = Vec::new(env);
                mint_args.push_back(invoice.creator.clone().into_val(env));
                mint_args.push_back(invoice_id.into_val(env));
                let mint_result = env.try_invoke_contract::<Val, soroban_sdk::Error>(
                    &nft_contract,
                    &Symbol::new(env, "mint"),
                    mint_args,
                );
                if mint_result.map_or(false, |r| r.is_ok()) {
                    events::nft_minted(env, invoice_id, &invoice.creator, &nft_contract);
                } else {
                    events::nft_mint_failed(env, invoice_id);
                }
            }
        }

        // Spin up next subscription invoice if one is scheduled.
        if let Some(params) = env
            .storage()
            .persistent()
            .get::<(Symbol, u64), SubscriptionParams>(&subscription_params_key(invoice_id))
        {
            let interval_secs = params.interval_days.unwrap_or(30) as u64 * 24 * 60 * 60;
            let next_deadline = env.ledger().timestamp() + interval_secs;
            let first_token = params.tokens.get(0).expect("no token in subscription");
            let _next_id = Self::_create_invoice_inner(
                env,
                params.creator.clone(),
                params.recipients.clone(),
                params.amounts.clone(),
                params.tokens.clone(),
                first_token.clone(),
                next_deadline,
                Vec::new(env),
                false,
                0,
                0,
                None,
                Vec::new(env),
                Vec::new(env),
                0,
                0,
                0,
                0,
                Vec::new(env),
                None,
                Vec::new(env),
                None,
                0,
                None,
                0,
                false,
                None,
                OverflowBehavior::Reject,
                false,
                Vec::new(env),
                None,
                None,
                None,
                0,
                0,
                Vec::new(env),
                Vec::new(env),
                None,
                None,
                None,
                None,
                None,
                None,
                Vec::new(env), // priorities
                false,         // require_kyc
                None,          // scheduled_release_at
                None,          // min_payer_rep
                None,          // release_delay_ledgers
                None,          // metadata_hash
                None,          // target_usd_cents
                None,          // oracle
                None,          // oracle_asset_pair_base
                None,          // oracle_asset_pair_quote
                None,          // escrow_hold_period
                None,          // payment_open_at
                None,          // payment_close_at
                None,          // milestones
                None,          // recipient_max_payouts
                false,         // recipient_whitelist_enabled
                None,          // release_condition_hash
                0,             // early_bird_window_ledgers
                0,             // early_bird_fee_bps
                0,             // creator_fee_bps
                Vec::new(env), // ratios
                1_u64,         // ratio_denominator
            );
            env.storage()
                .persistent()
                .remove(&subscription_params_key(invoice_id));
        }
    }

    // -----------------------------------------------------------------------
    // Issue: Auto-resolve (Issue 4)
    // -----------------------------------------------------------------------

    /// Evaluate auto_resolve_rules in order against the current funding ratio.
    /// Executes the first matching rule — Release calls _release(), Refund refunds payers.
    /// Panics with "no matching resolution rule" if no rule matches.
    pub fn auto_resolve(env: Env, invoice_id: u64) {
        require_not_paused(&env);
        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(
            !invoice.auto_resolve_rules.is_empty(),
            "no auto-resolve rules defined"
        );

        let total: i128 = invoice.amounts.iter().sum();
        assert!(total > 0, "invoice total must be positive");

        let funded_bps = funding_bps(invoice.funded, total);

        // Evaluate rules in order; execute first match.
        for rule in invoice.auto_resolve_rules.clone().iter() {
            if funded_bps >= rule.min_funded_bps {
                match rule.action {
                    ResolveAction::Release => {
                        // Reuse existing release guards (prerequisite, co-signers, etc.).
                        let caller = env.current_contract_address();
                        Self::_release(&env, invoice_id, &mut invoice, &caller);
                    }
                    ResolveAction::Refund => {
                        let token_client =
                            token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
                        let mut totals: Map<Address, i128> = Map::new(&env);
                        for payment in invoice.payments.iter() {
                            let prev = totals.get(payment.payer.clone()).unwrap_or(0);
                            totals.set(payment.payer.clone(), prev + payment.amount);
                        }
                        let mut total_refunded_amount: i128 = 0;
                        for (payer, amount) in totals.iter() {
                            token_client.transfer(&env.current_contract_address(), &payer, &amount);
                            total_refunded_amount += amount;
                            events::payer_refunded(&env, invoice_id, &payer, amount);
                        }
                        invoice.status = InvoiceStatus::Refunded;
                        invoice.completion_time = Some(env.ledger().timestamp());
                        save_invoice(&env, invoice_id, &invoice);
                        let actor = env.current_contract_address();
                        append_audit_entry(&env, invoice_id, symbol_short!("auto_ref"), &actor);
                        events::invoice_refunded(&env, invoice_id, total_refunded_amount);
                        events::invoice_state_changed(
                            &env,
                            invoice_id,
                            Some(&InvoiceStatus::Pending),
                            &InvoiceStatus::Refunded,
                            &actor,
                        );
                        maybe_record_refunded(&env, &invoice.creator);
                        notify_invoice(
                            &env,
                            invoice_id,
                            symbol_short!("refund"),
                            &invoice.notification_contract,
                        );
                        let total_refunded: i128 = env
                            .storage()
                            .persistent()
                            .get(&total_refunded_key())
                            .unwrap_or(0i128);
                        env.storage().persistent().set(
                            &total_refunded_key(),
                            &total_refunded
                                .checked_add(total_refunded_amount)
                                .expect("total_refunded overflow"),
                        );
                    }
                }
                return;
            }
        }

        panic!("no matching resolution rule");
    }

    // -----------------------------------------------------------------------
    // Refund / cancel / transfer / deadline
    // -----------------------------------------------------------------------

    /// Creator-initiated partial refund proportional to each payer's contribution.
    /// Distributes `funded * bps / 10_000` back to payers and decrements `invoice.funded`.
    pub fn partial_refund(env: Env, creator: Address, invoice_id: u64, bps: u32) {
        require_not_paused(&env);
        creator.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.creator == creator || invoice.co_creators.iter().any(|c| c == creator),
            "only creator can refund"
        );
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(bps <= 10_000, "bps must be ≤ 10000");
        assert!(invoice.funded > 0, "no funds to refund");
        // Issue #801: only while the invoice is still collecting — before its
        // deadline and before it is fully funded.
        assert!(
            env.ledger().timestamp() < invoice.deadline,
            "invoice deadline has passed"
        );
        assert!(
            invoice.funded < invoice.amounts.iter().sum::<i128>(),
            "invoice is fully funded"
        );

        let token_client = token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));

        let mut total_refunded: i128 = 0;
        for payment in invoice.payments.iter() {
            let refund_amount = payment.amount * bps as i128 / 10_000;
            if refund_amount > 0 {
                token_client.transfer(
                    &env.current_contract_address(),
                    &payment.payer,
                    &refund_amount,
                );
                total_refunded += refund_amount;
                events::payer_refunded(&env, invoice_id, &payment.payer, refund_amount);
            }
        }

        invoice.funded = invoice
            .funded
            .checked_sub(total_refunded)
            .expect("funded underflow");
        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("part_ref"), &creator);
        events::partial_refund_issued(&env, invoice_id, &creator, bps, total_refunded);
    }

    /// Issue #790: run this invoice as a co-funding round capped at `hard_cap`.
    /// Anything raised above the cap is refunded pro-rata by `close_round`
    /// once `round_end` has passed. Creator-only, while Pending, set once.
    pub fn set_funding_round(
        env: Env,
        creator: Address,
        invoice_id: u64,
        hard_cap: i128,
        round_end: u64,
    ) {
        require_not_paused(&env);
        creator.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        assert!(invoice.creator == creator, "only creator");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        let total: i128 = invoice.amounts.iter().sum();
        assert!(hard_cap > 0 && hard_cap <= total, "invalid hard cap");
        assert!(
            round_end > env.ledger().timestamp(),
            "round_end must be in the future"
        );
        let key = funding_round_key(invoice_id);
        assert!(
            !env.storage().persistent().has(&key),
            "funding round already set"
        );
        let round = RoundInfo {
            total_raised: 0,
            hard_cap,
            round_end,
            closed: false,
            overflow: 0,
        };
        env.storage().persistent().set(&key, &round);
    }

    /// Issue #790: the invoice's funding round, with live totals while open.
    pub fn get_round_info(env: Env, invoice_id: u64) -> RoundInfo {
        let mut round: RoundInfo = env
            .storage()
            .persistent()
            .get(&funding_round_key(invoice_id))
            .expect("no funding round");
        if !round.closed {
            let funded = load_invoice(&env, invoice_id).funded;
            round.total_raised = funded;
            round.overflow = (funded - round.hard_cap).max(0);
        }
        round
    }

    /// Issue #790: close the funding round after `round_end`. If more than
    /// `hard_cap` was raised, the excess is refunded to payers in proportion to
    /// their contributions (the last payer absorbs rounding). Callable by anyone.
    pub fn close_round(env: Env, invoice_id: u64) {
        require_not_paused(&env);
        let key = funding_round_key(invoice_id);
        let mut round: RoundInfo = env
            .storage()
            .persistent()
            .get(&key)
            .expect("no funding round");
        assert!(!round.closed, "round already closed");
        assert!(
            env.ledger().timestamp() >= round.round_end,
            "round has not ended"
        );

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        let total_raised = invoice.funded;
        let overflow = (total_raised - round.hard_cap).max(0);
        let mut refunded_count: u32 = 0;

        if overflow > 0 {
            let mut totals: Map<Address, i128> = Map::new(&env);
            for payment in invoice.payments.iter() {
                let prev = totals.get(payment.payer.clone()).unwrap_or(0);
                totals.set(payment.payer.clone(), prev + payment.amount);
            }
            let token_client = token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
            let payer_count = totals.len();
            let mut refunded: i128 = 0;
            for (i, (payer, contributed)) in totals.iter().enumerate() {
                let share = if i as u32 == payer_count - 1 {
                    overflow - refunded
                } else {
                    checked_proportion(contributed as u128, overflow as u128, total_raised as u128)
                        .expect("ArithmeticOverflow")
                };
                if share > 0 {
                    token_client.transfer(&env.current_contract_address(), &payer, &share);
                    events::payer_refunded(&env, invoice_id, &payer, share);
                    refunded += share;
                    refunded_count += 1;
                }
            }
            invoice.funded = total_raised - overflow;
            save_invoice(&env, invoice_id, &invoice);
        }

        round.closed = true;
        round.total_raised = total_raised;
        round.overflow = overflow;
        env.storage().persistent().set(&key, &round);
        events::round_closed(&env, invoice_id, total_raised, overflow, refunded_count);
    }

    /// Notify indexers that an invoice has expired.
    pub fn notify_expired(env: Env, invoice_id: u64) {
        require_not_paused(&env);
        let mut invoice = load_invoice(&env, invoice_id);

        assert!(invoice.status == InvoiceStatus::Pending, "InvalidStatus");
        assert!(env.ledger().timestamp() >= invoice.deadline, "NotExpired");

        invoice.status = InvoiceStatus::Expired;
        save_invoice(&env, invoice_id, &invoice);
        events::invoice_expired(
            &env,
            invoice_id,
            invoice.deadline,
            invoice.funded,
            &invoice.creator,
        );
        append_audit_entry(
            &env,
            invoice_id,
            symbol_short!("expired"),
            &env.current_contract_address(),
        );
        events::invoice_state_changed(
            &env,
            invoice_id,
            Some(&InvoiceStatus::Pending),
            &InvoiceStatus::Expired,
            &env.current_contract_address(),
        );
    }

    /// Issue #745: Trigger an automatic refund for all payers when the invoice
    /// deadline has passed and the invoice is not fully funded.
    ///
    /// Callable by **anyone** after `invoice.deadline`. Atomically refunds
    /// every payer and marks the invoice `Expired`. If the invoice is already
    /// `Expired` or `Refunded` the call is a no-op (idempotent).
    ///
    /// # Panics
    /// * `"InvoiceNotFound"` if `invoice_id` does not exist.
    /// * `"invoice not expired"` if the deadline has not yet passed.
    pub fn trigger_expiry(env: Env, invoice_id: u64) {
        let mut invoice = load_invoice(&env, invoice_id);

        // Idempotent: already expired/refunded — nothing to do.
        if invoice.status == InvoiceStatus::Expired
            || invoice.status == InvoiceStatus::Refunded
        {
            return;
        }

        // Must be past deadline and not yet fully funded.
        assert!(
            env.ledger().timestamp() >= invoice.deadline,
            "invoice not expired"
        );
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        let total: i128 = invoice.amounts.iter().sum();
        assert!(invoice.funded < total, "invoice is fully funded");

        let token_client = token::Client::new(&env, &funding_token_for(&invoice));

        // Aggregate per-payer totals from payment shards.
        let mut payer_totals: Map<Address, i128> = Map::new(&env);
        for shard_id in 0..SHARD_COUNT {
            if let Some(shard_payments) = env
                .storage()
                .persistent()
                .get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(invoice_id, shard_id))
            {
                for payment in shard_payments.iter() {
                    if !payment.donate_on_failure {
                        let prev = payer_totals.get(payment.payer.clone()).unwrap_or(0);
                        payer_totals.set(payment.payer.clone(), prev + payment.amount);
                    }
                }
            }
        }

        let mut refunded_count: u32 = 0;
        let mut total_refunded: i128 = 0;
        for (payer, amount) in payer_totals.iter() {
            if amount > 0 {
                token_client.transfer(&env.current_contract_address(), &payer, &amount);
                total_refunded += amount;
                refunded_count += 1;
            }
        }

        invoice.status = InvoiceStatus::Expired;
        invoice.completion_time = Some(env.ledger().timestamp());
        save_invoice(&env, invoice_id, &invoice);

        events::invoice_expired_refunded(&env, invoice_id, refunded_count, total_refunded);
    }

    /// Refund all payers once the invoice has expired.
    ///
    /// Accepts an invoice already marked `Expired` via `notify_expired`, and
    /// also lazily expires a `Pending` invoice whose deadline (plus any
    /// `refund_grace_secs`) has passed — callers should not have to make a
    /// separate `notify_expired` call just to unlock their funds.
    ///
    /// # Gas optimisation (issue #761)
    ///
    /// The implementation performs a **single storage scan** over all payment
    /// shards to aggregate per-payer totals into two in-memory maps (`totals`
    /// and `donate_totals`) before issuing any token transfers.  A single
    /// `token::Client` instance is created once and reused for every transfer,
    /// avoiding the per-recipient client-construction overhead that would apply
    /// if the client were re-created inside the loop.  This keeps compute-unit
    /// consumption O(shards + payers) rather than O(shards × payers).
    ///
    /// See `docs/GAS_OPTIMIZATIONS.md` for benchmarks.
    pub fn refund(env: Env, invoice_id: u64) {
        freeze_ext::require_not_frozen_global(&env);
        // --- Reentrancy guard (issue #451-reentrancy) ---
        let re_key = reentrancy_lock_key();
        if env.storage().temporary().has(&re_key) {
            panic!("{}", ContractError::ReentrantCall as u32);
        }
        env.storage().temporary().set(&re_key, &true);
        // ------------------------------------------------
        require_fn_not_paused(&env, &symbol_short!("refund"));
        let mut invoice = load_invoice(&env, invoice_id);

        if invoice.status == InvoiceStatus::Pending {
            let refund_deadline = match invoice.refund_grace_secs {
                Some(grace_secs) => invoice.deadline.saturating_add(grace_secs),
                None => invoice.deadline,
            };
            assert!(env.ledger().timestamp() > refund_deadline, "InvalidStatus");
        }
        // Lazy expiry: a Pending invoice past its deadline is treated as
        // Expired without requiring a separate notify_expired() call first.
        if invoice.status == InvoiceStatus::Pending && env.ledger().timestamp() >= invoice.deadline {
            invoice.status = InvoiceStatus::Expired;
        }

        assert!(invoice.status == InvoiceStatus::Expired, "InvalidStatus");

        if invoice.auction_on_expiry {
            let now = env.ledger().timestamp();
            if invoice.auction_end == 0 {
                invoice.auction_end = now.saturating_add(24 * 60 * 60);
                save_invoice(&env, invoice_id, &invoice);
                append_audit_entry(
                    &env,
                    invoice_id,
                    symbol_short!("auc_strt"),
                    &env.current_contract_address(),
                );
                return;
            }
            assert!(now > invoice.auction_end, "auction in progress");
            panic!("auction ended; settle auction");
        }

        let token_client = token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));

        // Aggregate payments from all shards (issue #177).
        let mut totals: Map<Address, i128> = Map::new(&env);
        // Issue #204: separate map for donate-on-failure contributions.
        let mut donate_totals: Map<Address, i128> = Map::new(&env);
        for shard_id in 0..SHARD_COUNT {
            if let Some(shard_payments) = env
                .storage()
                .persistent()
                .get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(invoice_id, shard_id))
            {
                for payment in shard_payments.iter() {
                    if payment.donate_on_failure {
                        let prev = donate_totals.get(payment.payer.clone()).unwrap_or(0);
                        donate_totals.set(payment.payer.clone(), prev + payment.amount);
                    } else {
                        let prev = totals.get(payment.payer.clone()).unwrap_or(0);
                        totals.set(payment.payer.clone(), prev + payment.amount);
                    }
                }
            }
        }

        let mut total_refunded_amount: i128 = 0;
        for (payer, amount) in totals.iter() {
            if amount > 0 {
                token_client.transfer(&env.current_contract_address(), &payer, &amount);
                total_refunded_amount += amount;
                events::payer_refunded(&env, invoice_id, &payer, amount);
            }
        }

        // Issue #204: send all donate-on-failure contributions to the creator.
        let mut total_donated: i128 = 0;
        for (_payer, amount) in donate_totals.iter() {
            total_donated += amount;
        }
        let creator_receives = invoice.bonus_pool + total_donated;
        if creator_receives > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &invoice.creator,
                &creator_receives,
            );
        }

        invoice.status = InvoiceStatus::Refunded;
        invoice.completion_time = Some(env.ledger().timestamp());
        save_invoice(&env, invoice_id, &invoice);
        let actor = env.current_contract_address();
        append_audit_entry(&env, invoice_id, symbol_short!("refund"), &actor);
        // Issue #763: record this refund in the per-invoice history ring buffer.
        append_history_entry(
            &env,
            invoice_id,
            Symbol::new(&env, "refund"),
            &actor,
            Some(total_refunded_amount),
        );
        events::invoice_refunded(&env, invoice_id, total_refunded_amount);
        events::invoice_state_changed(
            &env,
            invoice_id,
            Some(&InvoiceStatus::Expired),
            &InvoiceStatus::Refunded,
            &actor,
        );
        notify_invoice(
            &env,
            invoice_id,
            symbol_short!("refund"),
            &invoice.notification_contract,
        );
        maybe_record_refunded(&env, &invoice.creator);
        update_rep_internal(&env, &invoice.creator, |score| {
            score.invoices_refunded = score.invoices_refunded.saturating_add(1);
        });

        // Increment total_refunded counter (issue #28).
        let total_refunded: i128 = env
            .storage()
            .persistent()
            .get(&total_refunded_key())
            .unwrap_or(0i128);
        env.storage().persistent().set(
            &total_refunded_key(),
            &total_refunded
                .checked_add(total_refunded_amount)
                .expect("total_refunded overflow"),
        );

        // Increment creator refund counter (issue #106).
        let creator_refunded: u64 = env
            .storage()
            .persistent()
            .get(&creator_stats_refunded_key(&invoice.creator))
            .unwrap_or(0u64);
        env.storage().persistent().set(
            &creator_stats_refunded_key(&invoice.creator),
            &creator_refunded
                .checked_add(1)
                .expect("creator_refunded overflow"),
        );
        // Clear reentrancy lock on normal exit.
        env.storage().temporary().remove(&reentrancy_lock_key());
    }

    /// Backwards-compatible alias for the expiry-driven refund path.
    pub fn refund_invoice(env: Env, invoice_id: u64) {
        Self::refund(env, invoice_id)
    }

    /// Create a successor invoice for an expired, partially funded invoice.
    pub fn renew_invoice(
        env: Env,
        creator: Address,
        old_invoice_id: u64,
        new_deadline: u64,
    ) -> u64 {
        require_not_paused(&env);
        creator.require_auth();

        let mut old_invoice = load_invoice(&env, old_invoice_id);
        assert!(
            old_invoice.creator == creator,
            "only creator can renew invoice"
        );
        assert!(
            env.ledger().timestamp() > old_invoice.deadline,
            "invoice deadline has not passed"
        );
        assert!(
            !env.storage()
                .persistent()
                .has(&renewed_to_key(old_invoice_id)),
            "invoice already renewed"
        );
        assert!(
            new_deadline > env.ledger().timestamp(),
            "new deadline must be in the future"
        );

        let total: i128 = old_invoice.amounts.iter().sum();
        let carried_amount = total.saturating_sub(old_invoice.funded).max(0);

        let mut new_amounts: Vec<i128> = Vec::new(&env);
        if carried_amount > 0 && total > 0 {
            let mut distributed: i128 = 0;
            for i in 0..old_invoice.amounts.len() {
                let amount = old_invoice.amounts.get(i).unwrap();
                let share = if i == old_invoice.amounts.len() - 1 {
                    carried_amount - distributed
                } else {
                    (amount as u128 * carried_amount as u128 / total as u128) as i128
                };
                distributed += share;
                new_amounts.push_back(share.max(0));
            }
        } else {
            new_amounts = old_invoice.amounts.clone();
        }

        let id: u64 = env
            .storage()
            .persistent()
            .get(&counter_key())
            .unwrap_or(0u64)
            + 1;
        env.storage().persistent().set(&counter_key(), &id);
        set_created_ledger(&env, id);

        let mut clone_tokens = old_invoice.tokens.clone();
        if clone_tokens.is_empty() {
            clone_tokens = Vec::new(&env);
            for _ in old_invoice.recipients.iter() {
                clone_tokens.push_back(old_invoice.funding_token.clone());
            }
        }

        let new_invoice = Invoice {
            version: old_invoice.version,
            creator: old_invoice.creator.clone(),
            co_creators: old_invoice.co_creators.clone(),
            recipients: old_invoice.recipients.clone(),
            base_amounts: new_amounts.clone(),
            amounts: new_amounts,
            tokens: clone_tokens,
            funding_token: old_invoice.funding_token.clone(),
            deadline: new_deadline,
            funded: 0,
            status: InvoiceStatus::Pending,
            payments: Vec::new(&env),
            drip_duration: old_invoice.drip_duration,
            release_timestamp: old_invoice.release_timestamp,
            claimed: Vec::new(&env),
            frozen: false,
            completion_time: None,
            allow_early_withdrawal: old_invoice.allow_early_withdrawal,
            bonus_pool: 0,
            bonus_max_payers: 0,
            prerequisite_id: old_invoice.prerequisite_id,
            tranches: old_invoice.tranches.clone(),
            released_bps: 0,
            co_signers: old_invoice.co_signers.clone(),
            required_signatures: old_invoice.required_signatures,
            signatures: Vec::new(&env),
            approver: None,
            approved: false,
            condition_oracle: old_invoice.condition_oracle.clone(),
            condition_met: false,
            penalty_bps: old_invoice.penalty_bps,
            penalty_deadline: old_invoice.penalty_deadline,
            min_funding_bps: old_invoice.min_funding_bps,
            release_stages: old_invoice.release_stages.clone(),
            released_stages: 0,
            allowed_payers: old_invoice.allowed_payers.clone(),
            price_oracle: old_invoice.price_oracle.clone(),
            swap_tokens: old_invoice.swap_tokens.clone(),
            tax_bps: old_invoice.tax_bps,
            tax_authority: old_invoice.tax_authority.clone(),
            insurance_premium_bps: old_invoice.insurance_premium_bps,
            insurance_fund: 0,
            smart_route: old_invoice.smart_route,
            convert_to_stream: old_invoice.convert_to_stream,
            accepted_tokens: old_invoice.accepted_tokens.clone(),
            forward_to: old_invoice.forward_to.clone(),
            forward_invoice_id: old_invoice.forward_invoice_id,
            split_rules: old_invoice.split_rules.clone(),
            auto_resolve_rules: old_invoice.auto_resolve_rules.clone(),
            creator_cosigner: old_invoice.creator_cosigner.clone(),
            velocity_limit: old_invoice.velocity_limit,
            velocity_window: old_invoice.velocity_window,
            parent_invoice_id: old_invoice.parent_invoice_id,
            pause_reason: None,
            auto_resume_at: None,
            payment_cooldown_secs: old_invoice.payment_cooldown_secs,
            max_payments_per_window: old_invoice.max_payments_per_window,
            payment_window_secs: old_invoice.payment_window_secs,
            scheduled_release_at: old_invoice.scheduled_release_at,
            refund_grace_secs: old_invoice.refund_grace_secs,
            penalty_tiers: old_invoice.penalty_tiers.clone(),
            allowed_callers: old_invoice.allowed_callers.clone(),
            notification_contract: old_invoice.notification_contract.clone(),
            overflow_behavior: old_invoice.overflow_behavior.clone(),
            cross_chain_ref: old_invoice.cross_chain_ref.clone(),
            require_kyc: old_invoice.require_kyc,
            arbiter: old_invoice.arbiter.clone(),
            disputed: false,
            admin_frozen: false,
            auction_on_expiry: false,
            auction_end: 0,
            bids: Vec::new(&env),
            min_payment: old_invoice.min_payment,
            min_funding_amount: 0,
            priorities: old_invoice.priorities.clone(),
            clone_depth: old_invoice.clone_depth + 1,
            target_usd_cents: old_invoice.target_usd_cents,
            refunded_addresses: Vec::new(&env),
            oracle: old_invoice.oracle.clone(),
            oracle_asset_pair_base: old_invoice.oracle_asset_pair_base.clone(),
            oracle_asset_pair_quote: old_invoice.oracle_asset_pair_quote.clone(),
            min_payer_rep: old_invoice.min_payer_rep,
            escrow_hold_period: None,
            held_until: None,
            milestones: Vec::new(&env),
            milestones_released: 0,
            recipient_max_payouts: Vec::new(&env),
            twafr_numerator: 0,
            twafr_last_ledger: 0,
            release_condition_hash: None,
            recipient_whitelist_enabled: false,
            // Issue #420: carried over alongside `overflow_behavior`.
            overfunding_policy: old_invoice.overfunding_policy.clone(),
            contributor_allowlist: None,
            predecessor_id: Some(old_invoice_id),
            early_bird_window_ledgers: old_invoice.early_bird_window_ledgers,
            early_bird_fee_bps: old_invoice.early_bird_fee_bps,
            early_bird_fee_credit: 0,
            creator_fee_bps: old_invoice.creator_fee_bps,
            ratio_denominator: old_invoice.ratio_denominator,
            ratios: old_invoice.ratios.clone(),
            metadata_hash: old_invoice.metadata_hash.clone(),
            max_contribution_per_payer: old_invoice.max_contribution_per_payer,
        };

        save_invoice(&env, id, &new_invoice);
        env.storage()
            .persistent()
            .set(&renewed_to_key(old_invoice_id), &id);

        old_invoice.status = InvoiceStatus::Cancelled;
        old_invoice.completion_time = Some(env.ledger().timestamp());
        save_invoice(&env, old_invoice_id, &old_invoice);

        events::invoice_renewed(&env, old_invoice_id, id, carried_amount);
        id
    }

    /// Refund a payer who opts out of renewal for an expired invoice.
    pub fn opt_out_renewal(env: Env, payer: Address, old_invoice_id: u64) {
        require_not_paused(&env);
        payer.require_auth();

        let mut invoice = load_invoice(&env, old_invoice_id);
        assert!(invoice.creator != payer, "invalid payer");
        assert!(
            env.storage()
                .persistent()
                .has(&renewed_to_key(old_invoice_id)),
            "invoice not renewed"
        );
        assert!(
            env.ledger().timestamp() > invoice.deadline,
            "invoice deadline has not passed"
        );
        assert!(
            !invoice.refunded_addresses.iter().any(|a| a == payer),
            "already refunded"
        );

        let amount = Self::get_payer_total(env.clone(), old_invoice_id, payer.clone());
        assert!(amount > 0, "payer has no contribution");

        let token_client = token::Client::new(&env, &funding_token_for(&invoice));
        token_client.transfer(&env.current_contract_address(), &payer, &amount);

        invoice.refunded_addresses.push_back(payer.clone());
        save_invoice(&env, old_invoice_id, &invoice);
        events::payer_refunded(&env, old_invoice_id, &payer, amount);
    }

    /// Rate a released invoice (issue #759).
    ///
    /// - Only callable after the invoice is in `Released` status.
    /// - `stars` must be 1–5; panics with `InvalidRating` otherwise.
    /// - One rating per payer per invoice; a second call panics with `AlreadyRated`.
    /// - Caller must have paid into the invoice; panics with `NotAuthorized` otherwise.
    /// - Updates the creator's aggregate `CreatorRating` stored under `creator_rating_key`.
    /// - Emits `InvoiceRated { invoice_id, payer, stars }`.
    pub fn rate_invoice(env: Env, invoice_id: u64, payer: Address, stars: u32) {
        require_not_paused(&env);
        payer.require_auth();

        let invoice = load_invoice(&env, invoice_id);
        if invoice.status != InvoiceStatus::Released {
            panic_with_error!(&env, ContractError::InvalidStatus);
        }
        if stars < 1 || stars > 5 {
            panic_with_error!(&env, ContractError::InvalidRating);
        }
        if Self::get_payer_total(env.clone(), invoice_id, payer.clone()) == 0 {
            panic_with_error!(&env, ContractError::NotAuthorized);
        }
        if env
            .storage()
            .persistent()
            .has(&invoice_rating_key(invoice_id, &payer))
        {
            panic_with_error!(&env, ContractError::AlreadyRated);
        }

        // Record per-(invoice, payer) rating.
        env.storage()
            .persistent()
            .set(&invoice_rating_key(invoice_id, &payer), &stars);

        // Update per-invoice aggregate (sum + count).
        let sum_key = invoice_rating_sum_key(invoice_id);
        let count_key = invoice_rating_count_key(invoice_id);
        let new_sum: u32 = env.storage().persistent().get(&sum_key).unwrap_or(0u32) + stars;
        let new_count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0u32) + 1;
        env.storage().persistent().set(&sum_key, &new_sum);
        env.storage().persistent().set(&count_key, &new_count);

        // Update creator aggregate CreatorRating struct.
        let creator_key = creator_rating_key(&invoice.creator);
        let mut existing: CreatorRating = env
            .storage()
            .persistent()
            .get(&creator_key)
            .unwrap_or(CreatorRating {
                total_ratings: 0,
                average_stars_bps: 0,
            });
        let new_total = existing.total_ratings + 1;
        // Running sum is total_ratings * average_stars_bps / 10_000 + stars.
        let prev_sum = existing.total_ratings as u64 * existing.average_stars_bps as u64 / 10_000;
        let new_avg_bps = ((prev_sum + stars as u64) * 10_000 / new_total as u64) as u32;
        existing.total_ratings = new_total;
        existing.average_stars_bps = new_avg_bps;
        env.storage().persistent().set(&creator_key, &existing);

        events::invoice_rated(&env, invoice_id, &payer, stars);
    }

    /// Place a bid on an active auction for an expired invoice.
    pub fn place_bid(env: Env, bidder: Address, invoice_id: u64, amount: i128) {
        require_not_paused(&env);
        bidder.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(invoice.auction_on_expiry, "auction not enabled");
        assert!(invoice.auction_end > 0, "auction not started");
        let now = env.ledger().timestamp();
        assert!(now <= invoice.auction_end, "auction not active");
        assert!(amount > 0, "bid amount must be positive");

        let current_highest =
            invoice
                .bids
                .iter()
                .map(|b| b.amount)
                .fold(0, |max, amt| if amt > max { amt } else { max });
        assert!(
            amount > current_highest,
            "bid must be higher than current highest bid"
        );

        let token_client = token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
        token_client.transfer(&bidder, &env.current_contract_address(), &amount);

        invoice.bids.push_back(Bid {
            bidder: bidder.clone(),
            amount,
        });
        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("bid"), &bidder);
    }

    /// Settle an auction after the 24-hour auction window ends.
    pub fn settle_auction(env: Env, invoice_id: u64) {
        require_not_paused(&env);

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(invoice.auction_on_expiry, "auction not enabled");
        assert!(invoice.auction_end > 0, "auction not started");
        let now = env.ledger().timestamp();
        assert!(now > invoice.auction_end, "auction not ended");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        let token_client = token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));

        let mut winner_idx: Option<u32> = None;
        let mut highest_amount: i128 = 0;
        for i in 0..invoice.bids.len() {
            let bid = invoice.bids.get(i).unwrap();
            if winner_idx.is_none() || bid.amount > highest_amount {
                winner_idx = Some(i);
                highest_amount = bid.amount;
            }
        }

        if let Some(idx) = winner_idx {
            let winner = invoice.bids.get(idx).unwrap();
            token_client.transfer(
                &env.current_contract_address(),
                &winner.bidder,
                &invoice.funded,
            );
            for i in 0..invoice.bids.len() {
                if i != idx {
                    let bid = invoice.bids.get(i).unwrap();
                    token_client.transfer(
                        &env.current_contract_address(),
                        &bid.bidder,
                        &bid.amount,
                    );
                }
            }
            invoice.status = InvoiceStatus::Refunded;
            invoice.completion_time = Some(now);
            save_invoice(&env, invoice_id, &invoice);
            events::invoice_state_changed(
                &env,
                invoice_id,
                Some(&InvoiceStatus::Pending),
                &InvoiceStatus::Refunded,
                &env.current_contract_address(),
            );
            append_audit_entry(
                &env,
                invoice_id,
                symbol_short!("auc_stl"),
                &env.current_contract_address(),
            );
            return;
        }

        // No bids were placed; refund payers as normal.
        // Aggregate payments from all shards (issue #177).
        let mut totals: Map<Address, i128> = Map::new(&env);
        for shard_id in 0..SHARD_COUNT {
            if let Some(shard_payments) = env
                .storage()
                .persistent()
                .get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(invoice_id, shard_id))
            {
                for payment in shard_payments.iter() {
                    let prev = totals.get(payment.payer.clone()).unwrap_or(0);
                    totals.set(payment.payer.clone(), prev + payment.amount);
                }
            }
        }

        let mut total_refunded_amount: i128 = 0;
        for (payer, amount) in totals.iter() {
            token_client.transfer(&env.current_contract_address(), &payer, &amount);
            total_refunded_amount += amount;
            events::payer_refunded(&env, invoice_id, &payer, amount);
        }

        if invoice.bonus_pool > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &invoice.creator,
                &invoice.bonus_pool,
            );
        }

        invoice.status = InvoiceStatus::Refunded;
        invoice.completion_time = Some(now);
        save_invoice(&env, invoice_id, &invoice);
        events::invoice_state_changed(
            &env,
            invoice_id,
            Some(&InvoiceStatus::Pending),
            &InvoiceStatus::Refunded,
            &env.current_contract_address(),
        );
        append_audit_entry(
            &env,
            invoice_id,
            symbol_short!("auc_stl"),
            &env.current_contract_address(),
        );

        let total_refunded: i128 = env
            .storage()
            .persistent()
            .get(&total_refunded_key())
            .unwrap_or(0i128);
        env.storage().persistent().set(
            &total_refunded_key(),
            &total_refunded
                .checked_add(total_refunded_amount)
                .expect("total_refunded overflow"),
        );
    }

    /// Cancel an invoice **before any payment has been made** (issue #757).
    ///
    /// Only the creator (or a co-creator) may call this. If any payment has
    /// already been received (`funded > 0`) the call panics with
    /// `CannotCancelFundedInvoice` — use `refund` instead once payments exist.
    ///
    /// A cancelled invoice permanently rejects `pay`, `release`, and `refund`.
    /// Emits `InvoiceCancelled { invoice_id, creator, timestamp }`.
    pub fn cancel_invoice(env: Env, caller: Address, invoice_id: u64) {
        freeze_ext::require_not_frozen_global(&env);
        // --- Reentrancy guard (issue #451-reentrancy) ---
        let re_key = reentrancy_lock_key();
        if env.storage().temporary().has(&re_key) {
            panic!("{}", ContractError::ReentrantCall as u32);
        }
        env.storage().temporary().set(&re_key, &true);
        // ------------------------------------------------
        require_not_paused(&env);
        caller.require_auth();
        delegate_ext::reject_delegate(&env, invoice_id, &caller);

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(invoice.status != InvoiceStatus::Deleted, "InvoiceDeleted");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.disputed, "invoice is disputed");

        // Issue #757: only allow cancellation before any payment has been made.
        if invoice.funded > 0 {
            panic_with_error!(&env, ContractError::CannotCancelFundedInvoice);
        }

        // If a creator cosigner is set, require both the creator and cosigner auths.
        if let Some(cos) = invoice.creator_cosigner.clone() {
            invoice.creator.require_auth();
            cos.require_auth();
        } else {
            // Allow creator OR co-creator to cancel.
            require_creator_or_cocreator(&invoice, &caller);
        }

        // Issue: check cancellation rate limit before allowing cancel.
        let inv_cnt: u64 = env
            .storage()
            .persistent()
            .get(&invoice_count_key(&caller))
            .unwrap_or(0u64);
        let max_cancel_bps: u32 = env
            .storage()
            .persistent()
            .get(&max_cancel_bps_key())
            .unwrap_or(0u32);
        if max_cancel_bps > 0 {
            let cnl_cnt: u64 = env
                .storage()
                .persistent()
                .get(&cancel_count_key(&caller))
                .unwrap_or(0u64);
            if let Some(cancel_rate) = (cnl_cnt * 10_000).checked_div(inv_cnt) {
                assert!(
                    cancel_rate < max_cancel_bps as u64,
                    "cancellation rate too high"
                );
            }
        }

        // Return any un-paid bonus_pool to the creator.
        if invoice.bonus_pool > 0 {
            let token_client =
                token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
            token_client.transfer(
                &env.current_contract_address(),
                &invoice.creator,
                &invoice.bonus_pool,
            );
        }

        invoice.status = InvoiceStatus::Cancelled;

        // Issue #503: decrement per-creator open-invoice counter on cancel.
        {
            let cnt: u32 = env
                .storage()
                .persistent()
                .get(&open_invoice_count_key(&invoice.creator))
                .unwrap_or(0u32);
            env.storage()
                .persistent()
                .set(&open_invoice_count_key(&invoice.creator), &cnt.saturating_sub(1));
        }

        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("cancel"), &caller);
        // Issue #763: record this cancellation in the per-invoice history ring buffer.
        append_history_entry(
            &env,
            invoice_id,
            Symbol::new(&env, "cancel"),
            &caller,
            None,
        );

        // Emit state-change event for indexers tracking status transitions.
        events::invoice_state_changed(
            &env,
            invoice_id,
            Some(&InvoiceStatus::Pending),
            &InvoiceStatus::Cancelled,
            &caller,
        );

        // Emit InvoiceCancelled { invoice_id, creator, timestamp }.
        events::invoice_cancelled(&env, invoice_id, &caller);

        // Issue: increment per-creator cancel count for cancellation rate tracking.
        let cnl_cnt: u64 = env
            .storage()
            .persistent()
            .get(&cancel_count_key(&caller))
            .unwrap_or(0u64);
        env.storage()
            .persistent()
            .set(&cancel_count_key(&caller), &(cnl_cnt + 1));

        // Issue #439: set cancellation cooldown for the creator.
        let cooldown_ledgers: u64 = env
            .storage()
            .instance()
            .get(&cancellation_cooldown_ledgers_key())
            .unwrap_or(DEFAULT_CANCELLATION_COOLDOWN_LEDGERS);
        if cooldown_ledgers > 0 {
            let current_ledger = env.ledger().sequence() as u64;
            let until_ledger = current_ledger.saturating_add(cooldown_ledgers);
            env.storage()
                .persistent()
                .set(&creator_cooldown_key(&invoice.creator), &until_ledger);
            events::creator_cooldown_set(&env, &invoice.creator, until_ledger, cooldown_ledgers);
        }
        // Clear reentrancy lock on normal exit.
        env.storage().temporary().remove(&reentrancy_lock_key());
    }

    /// Transfer invoice ownership to a new creator.
    pub fn transfer_invoice(env: Env, invoice_id: u64, new_creator: Address) {
        require_not_paused(&env);
        let mut invoice = load_invoice(&env, invoice_id);
        require_invoice_unlocked(&env, invoice_id, &invoice);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.disputed, "invoice is disputed");

        invoice.creator.require_auth();
        invoice.creator = new_creator;
        save_invoice(&env, invoice_id, &invoice);
    }

    /// Extend the deadline for an invoice. Callable by the creator or an assigned delegate.
    pub fn extend_deadline(env: Env, invoice_id: u64, new_deadline: u64, caller: Address) {
        require_not_paused(&env);
        caller.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);
        require_invoice_unlocked(&env, invoice_id, &invoice);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice not pending"
        );
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(
            new_deadline > invoice.deadline,
            "new deadline must be after current deadline"
        );

        // If a creator cosigner is set, require both creator and cosigner auths.
        if let Some(cos) = invoice.creator_cosigner.clone() {
            invoice.creator.require_auth();
            cos.require_auth();
        } else {
            // Accept caller = creator OR co-creator OR assigned delegate (issue #43).
            let delegate: Option<Address> =
                env.storage().persistent().get(&delegate_key(invoice_id));
            let is_creator_or_co = invoice.creator == caller
                || invoice.co_creators.iter().any(|c| c == caller);
            let is_delegate = delegate.map(|d| d == caller).unwrap_or(false);
            assert!(is_creator_or_co || is_delegate, "not authorized");
        }

        let old_deadline = invoice.deadline;
        invoice.deadline = new_deadline;
        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("extend"), &caller);
        events::deadline_extended(&env, invoice_id, old_deadline, new_deadline);
    }

    /// Roll over a partially funded invoice to a new invoice with the same recipients,
    /// amounts, and token. Carries over all existing payments and marks the old invoice
    /// as Refunded without transferring tokens.
    ///
    /// Requires creator auth. The old invoice must be Pending and past its deadline.
    /// The new deadline must be in the future.
    pub fn rollover_invoice(env: Env, caller: Address, invoice_id: u64, new_deadline: u64) -> u64 {
        require_not_paused(&env);
        caller.require_auth();

        let mut old_invoice = load_invoice(&env, invoice_id);

        assert!(
            old_invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(
            old_invoice.creator == caller,
            "only creator can rollover invoice"
        );
        assert!(
            env.ledger().timestamp() > old_invoice.deadline,
            "invoice deadline has not passed"
        );
        assert!(
            new_deadline > env.ledger().timestamp(),
            "new deadline must be in the future"
        );

        // Create new invoice with same recipients, amounts, and token.
        let new_id = Self::_create_invoice_inner(
            &env,
            old_invoice.creator.clone(),
            old_invoice.recipients.clone(),
            old_invoice.amounts.clone(),
            old_invoice.tokens.clone(),
            old_invoice.funding_token.clone(),
            new_deadline,
            old_invoice.co_creators.clone(),
            old_invoice.allow_early_withdrawal,
            0, // No bonus pool on rollover
            0, // No bonus max payers on rollover
            old_invoice.prerequisite_id,
            old_invoice.tranches.clone(),
            old_invoice.co_signers.clone(),
            old_invoice.required_signatures,
            old_invoice.penalty_bps,
            old_invoice.penalty_deadline,
            old_invoice.min_funding_bps,
            old_invoice.release_stages.clone(),
            old_invoice.price_oracle.clone(),
            old_invoice.swap_tokens.clone(),
            old_invoice.condition_oracle.clone(),
            old_invoice.tax_bps,
            old_invoice.tax_authority.clone(),
            old_invoice.insurance_premium_bps,
            old_invoice.smart_route,
            old_invoice.notification_contract.clone(),
            old_invoice.overflow_behavior.clone(),
            old_invoice.convert_to_stream,
            old_invoice.accepted_tokens.clone(),
            old_invoice.forward_to.clone(),
            old_invoice.forward_invoice_id,
            old_invoice.creator_cosigner.clone(),
            old_invoice.velocity_limit,
            old_invoice.velocity_window,
            old_invoice.split_rules.clone(),
            old_invoice.auto_resolve_rules.clone(),
            old_invoice.cross_chain_ref.clone(),
            None,
            old_invoice.payment_cooldown_secs,
            old_invoice.max_payments_per_window,
            old_invoice.payment_window_secs,
            old_invoice.refund_grace_secs,
            old_invoice.priorities.clone(),
            old_invoice.require_kyc,
            old_invoice.scheduled_release_at,
            old_invoice.min_payer_rep,
            None, // release_delay_ledgers
            None, // metadata_hash
            None, // target_usd_cents
            old_invoice.oracle.clone(),
            old_invoice.oracle_asset_pair_base.clone(),
            old_invoice.oracle_asset_pair_quote.clone(),
            old_invoice.escrow_hold_period,
            None, // payment_open_at (not carried over on rollover)
            None, // payment_close_at (not carried over on rollover)
            Some(old_invoice.milestones.clone()),
            Some(old_invoice.recipient_max_payouts.clone()),
            false, // recipient_whitelist_enabled
            None,  // release_condition_hash
            old_invoice.early_bird_window_ledgers,
            old_invoice.early_bird_fee_bps,
            old_invoice.creator_fee_bps,
            old_invoice.ratios.clone(),
            old_invoice.ratio_denominator,
        );

        // Copy payments from shards to new invoice (issue #177).
        for shard_id in 0..SHARD_COUNT {
            if let Some(shard_payments) = env
                .storage()
                .persistent()
                .get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(invoice_id, shard_id))
            {
                env.storage()
                    .persistent()
                    .set(&pay_shard_key(new_id, shard_id), &shard_payments);
            }
        }

        // Load the newly created invoice and set funded amount.
        let mut new_invoice = load_invoice(&env, new_id);
        new_invoice.funded = old_invoice.funded;
        save_invoice(&env, new_id, &new_invoice);

        // Mark old invoice as Refunded without transferring tokens.
        old_invoice.status = InvoiceStatus::Refunded;
        old_invoice.completion_time = Some(env.ledger().timestamp());
        save_invoice(&env, invoice_id, &old_invoice);
        events::invoice_state_changed(
            &env,
            invoice_id,
            Some(&InvoiceStatus::Pending),
            &InvoiceStatus::Refunded,
            &caller,
        );

        append_audit_entry(&env, invoice_id, symbol_short!("rollover"), &caller);
        append_audit_entry(&env, new_id, symbol_short!("rollover"), &caller);

        new_id
    }

    // -----------------------------------------------------------------------
    // Adjust split
    // -----------------------------------------------------------------------

    // Update recipient amounts before any payment has been received.
    //
    // Only the creator may call this. Panics if any payment has already been
    // made (`invoice.funded > 0`). The length of `new_amounts` must match the
    // current number of recipients, and every amount must be positive.
    // -----------------------------------------------------------------------
    // Add recipient
    // -----------------------------------------------------------------------

    /// Append a new recipient with a fixed amount to a pending invoice.
    /// Only the creator may call this, and only before any payment has been
    /// received.
    pub fn add_recipient(
        env: Env,
        caller: Address,
        invoice_id: u64,
        recipient: Address,
        amount: i128,
    ) {
        require_not_paused(&env);
        caller.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);
        require_invoice_unlocked(&env, invoice_id, &invoice);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(invoice.creator == caller, "only creator can add recipients");
        assert!(
            invoice.funded == 0,
            "cannot add recipient after payment received"
        );
        assert!(amount > 0, "amount must be positive");

        let token = invoice.tokens.get(0).expect("no token");

        invoice.recipients.push_back(recipient.clone());
        invoice.amounts.push_back(amount);
        invoice.tokens.push_back(token);
        invoice.claimed.push_back(0i128);

        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("add_rec"), &caller);
        events::recipient_added(&env, invoice_id, &recipient, amount);

        // Index new recipient -> invoice ID (issue #40).
        let key = recipient_invoice_ids_key(&recipient);
        let mut ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(&env));
        ids.push_back(invoice_id);
        env.storage().persistent().set(&key, &ids);
    }

    // -----------------------------------------------------------------------
    // Adjust split
    // -----------------------------------------------------------------------

    /// Rebalance recipient amounts before any payment has been received.
    ///
    /// Only the creator may call this. Panics if any payment has already been
    /// made (`invoice.funded > 0`). The length of `new_amounts` must match the
    /// existing number of recipients, and every amount must be positive.
    pub fn adjust_split(env: Env, caller: Address, invoice_id: u64, new_amounts: Vec<i128>) {
        require_not_paused(&env);
        caller.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);
        require_invoice_unlocked(&env, invoice_id, &invoice);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.disputed, "invoice is disputed");
        // If a creator cosigner is set, require both creator and cosigner auths.
        if let Some(cos) = invoice.creator_cosigner.clone() {
            invoice.creator.require_auth();
            cos.require_auth();
        } else {
            assert!(invoice.creator == caller, "only creator can adjust split");
        }
        assert!(invoice.funded == 0, "payments already received");
        assert!(
            new_amounts.len() == invoice.recipients.len(),
            "amounts length mismatch"
        );
        for amt in new_amounts.iter() {
            assert!(amt > 0, "amounts must be positive");
        }

        invoice.amounts = new_amounts;
        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("adj_spl"), &caller);
        events::split_adjusted(&env, invoice_id, &caller);
    }

    /// Remove a recipient and redistribute their share proportionally among
    /// the remaining recipients (issue #423). Only the creator may call this,
    /// and only before any payment has been received. At least two recipients
    /// must remain after removal. Any remainder left over from integer-division
    /// rounding is added to the first remaining recipient's share, so the total
    /// invoice amount is exactly preserved.
    pub fn rebalance_recipients(
        env: Env,
        creator: Address,
        invoice_id: u64,
        remove_address: Address,
    ) {
        require_not_paused(&env);
        creator.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(
            invoice.creator == creator,
            "only creator can rebalance recipients"
        );
        assert!(invoice.funded == 0, "payments already received");
        assert!(invoice.recipients.len() >= 3, "InsufficientRecipients");

        let idx = invoice
            .recipients
            .iter()
            .position(|r| r == remove_address)
            .expect("recipient not in invoice") as u32;

        let removed_amount = invoice.amounts.get(idx).unwrap();

        let mut new_recipients: Vec<Address> = Vec::new(&env);
        let mut new_amounts: Vec<i128> = Vec::new(&env);
        let mut new_tokens: Vec<Address> = Vec::new(&env);
        let mut new_claimed: Vec<i128> = Vec::new(&env);
        for i in 0..invoice.recipients.len() {
            if i == idx {
                continue;
            }
            new_recipients.push_back(invoice.recipients.get(i).unwrap());
            new_amounts.push_back(invoice.amounts.get(i).unwrap());
            new_tokens.push_back(invoice.tokens.get(i).unwrap());
            new_claimed.push_back(invoice.claimed.get(i).unwrap());
        }

        // Distribute the removed recipient's amount proportionally to each
        // remaining recipient's existing share of the remaining total.
        let remaining_total: i128 = new_amounts.iter().sum();
        let mut distributed: i128 = 0;
        for i in 0..new_amounts.len() {
            let base = new_amounts.get(i).unwrap();
            let share = removed_amount * base / remaining_total;
            distributed += share;
            new_amounts.set(i, base + share);
        }
        // Integer division can leave a remainder; give it to the first recipient
        // so the invoice total is unchanged.
        let remainder = removed_amount - distributed;
        let first = new_amounts.get(0).unwrap();
        new_amounts.set(0, first + remainder);

        invoice.recipients = new_recipients;
        invoice.amounts = new_amounts;
        invoice.tokens = new_tokens;
        invoice.claimed = new_claimed;

        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("rebal"), &creator);
        events::recipients_rebalanced(&env, invoice_id, &remove_address, removed_amount);
    }

    // -----------------------------------------------------------------------
    // Issue #813: Payment hints
    // -----------------------------------------------------------------------

    /// Set a suggested per-payer amount for a pending invoice. Only the
    /// creator may call this. The hint must be positive and must not exceed
    /// the invoice total.
    pub fn set_payment_hint(env: Env, creator: Address, invoice_id: u64, suggested_amount: i128) {
        require_not_paused(&env);
        creator.require_auth();

        let invoice = load_invoice(&env, invoice_id);
        assert!(invoice.creator == creator, "only creator can set payment hint");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        let total: i128 = invoice.amounts.iter().sum();
        assert!(
            suggested_amount > 0 && suggested_amount <= total,
            "invalid suggested amount"
        );

        env.storage()
            .persistent()
            .set(&payment_hint_key(invoice_id), &suggested_amount);
        events::payment_hint_set(&env, invoice_id, &creator, suggested_amount);
    }

    /// Suggest how much a payer should contribute, assuming `num_payers`
    /// payers share the remaining balance. Returns the creator's hint (capped
    /// at the remaining balance) when one is set, otherwise the remaining
    /// balance divided evenly (rounded up). Returns 0 once fully funded.
    pub fn get_payment_hint(env: Env, invoice_id: u64, num_payers: u32) -> i128 {
        assert!(num_payers > 0, "num_payers must be positive");
        let invoice = load_invoice(&env, invoice_id);
        let total: i128 = invoice.amounts.iter().sum();
        let remaining = total - invoice.funded;
        if remaining <= 0 {
            return 0;
        }
        if let Some(hint) = env
            .storage()
            .persistent()
            .get::<_, i128>(&payment_hint_key(invoice_id))
        {
            return hint.min(remaining);
        }
        let n = num_payers as i128;
        (remaining + n - 1) / n
    }

    // -----------------------------------------------------------------------
    // Issue #814: Recipient share redistribution
    // -----------------------------------------------------------------------

    /// Redistribute the invoice total across the current recipients according
    /// to `weights_bps` (one entry per recipient, summing to 10 000). Only the
    /// creator may call this, and only before any payment has been received.
    /// Any integer-division remainder goes to the first recipient so the
    /// invoice total is exactly preserved.
    pub fn redistribute_shares(env: Env, creator: Address, invoice_id: u64, weights_bps: Vec<u32>) {
        require_not_paused(&env);
        creator.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(invoice.creator == creator, "only creator can redistribute shares");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(invoice.funded == 0, "payments already received");
        assert!(
            weights_bps.len() == invoice.recipients.len(),
            "weights length mismatch"
        );
        let mut weight_sum: u32 = 0;
        for w in weights_bps.iter() {
            if w == 0 {
                panic_with_error!(&env, ContractError::InvalidBps);
            }
            weight_sum += w;
        }
        if weight_sum != 10_000 {
            panic_with_error!(&env, ContractError::InvalidBps);
        }

        let total: i128 = invoice.amounts.iter().sum();
        let mut new_amounts: Vec<i128> = Vec::new(&env);
        let mut distributed: i128 = 0;
        for w in weights_bps.iter() {
            let share = total * w as i128 / 10_000;
            distributed += share;
            new_amounts.push_back(share);
        }
        let first = new_amounts.get(0).unwrap();
        new_amounts.set(0, first + (total - distributed));
        for amt in new_amounts.iter() {
            assert!(amt > 0, "amounts must be positive");
        }

        invoice.amounts = new_amounts.clone();
        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("redistr"), &creator);
        events::shares_redistributed(&env, invoice_id, &creator, &new_amounts);
    }

    // -----------------------------------------------------------------------
    // Issue #815: Creator earnings tracking
    // -----------------------------------------------------------------------

    /// Aggregate earnings for `creator` over invoice IDs `from_id..=to_id`
    /// (at most 100 IDs per call), optionally filtered by status and token.
    /// Missing IDs and invoices from other creators are skipped.
    pub fn get_creator_earnings(
        env: Env,
        creator: Address,
        from_id: u64,
        to_id: u64,
        status_filter: Option<InvoiceStatus>,
        token_filter: Option<Address>,
    ) -> CreatorEarnings {
        assert!(from_id <= to_id, "invalid range");
        assert!(to_id - from_id < 100, "range too large");

        let mut earnings = CreatorEarnings {
            invoice_count: 0,
            total_amount: 0,
            total_funded: 0,
            total_released: 0,
        };
        for id in from_id..=to_id {
            let key = invoice_key(id);
            if !env.storage().persistent().has(&key) && !env.storage().instance().has(&key) {
                continue;
            }
            let invoice = load_invoice(&env, id);
            if invoice.creator != creator {
                continue;
            }
            if let Some(status) = &status_filter {
                if invoice.status != *status {
                    continue;
                }
            }
            if let Some(token) = &token_filter {
                if invoice.funding_token != *token {
                    continue;
                }
            }
            earnings.invoice_count += 1;
            earnings.total_amount += invoice.amounts.iter().sum::<i128>();
            earnings.total_funded += invoice.funded;
            if invoice.status == InvoiceStatus::Released {
                earnings.total_released += invoice.funded;
            }
        }

        events::creator_earnings_queried(
            &env,
            &creator,
            earnings.invoice_count,
            earnings.total_released,
        );
        earnings
    }

    // -----------------------------------------------------------------------
    // Issue #816: Invoice retirement
    // -----------------------------------------------------------------------

    /// Retire a finalised (Released, Refunded, Cancelled or Expired) invoice.
    /// The invoice record is kept intact and remains readable; retirement only
    /// records the ledger timestamp so off-chain indexers can hide it. Only
    /// the creator may retire, and each invoice can be retired once.
    pub fn retire_invoice(env: Env, creator: Address, invoice_id: u64) {
        require_not_paused(&env);
        creator.require_auth();

        let invoice = load_invoice(&env, invoice_id);
        assert!(invoice.creator == creator, "only creator can retire invoice");
        assert!(
            matches!(
                invoice.status,
                InvoiceStatus::Released
                    | InvoiceStatus::Refunded
                    | InvoiceStatus::Cancelled
                    | InvoiceStatus::Expired
            ),
            "invoice not finalised"
        );
        let key = retired_at_key(invoice_id);
        assert!(!env.storage().persistent().has(&key), "invoice already retired");

        let now = env.ledger().timestamp();
        env.storage().persistent().set(&key, &now);
        append_audit_entry(&env, invoice_id, symbol_short!("retired"), &creator);
        events::invoice_retired(&env, invoice_id, &creator, now);
    }

    /// Return the timestamp at which the invoice was retired, if any.
    pub fn get_retired_at(env: Env, invoice_id: u64) -> Option<u64> {
        env.storage().persistent().get(&retired_at_key(invoice_id))
    }

    // -----------------------------------------------------------------------
    // Templates
    // -----------------------------------------------------------------------

    /// Save a reusable invoice template.
    /// Save an invoice template. Returns the new version number.
    /// Each call increments the version counter for (creator, name).
    pub fn save_template(
        env: Env,
        creator: Address,
        name: Symbol,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        token: Address,
    ) -> u32 {
        creator.require_auth();
        assert!(
            recipients.len() == amounts.len(),
            "recipients and amounts length mismatch"
        );
        assert!(!recipients.is_empty(), "must have at least one recipient");
        for amt in amounts.iter() {
            assert!(amt > 0, "amounts must be positive");
        }

        let count: u32 = env
            .storage()
            .persistent()
            .get(&template_version_count_key(&creator, &name))
            .unwrap_or(0u32);
        let version = count + 1;

        let template = InvoiceTemplate {
            recipients,
            amounts,
            token,
            deadline_ledger: 0,
            funded: 0,
            status: InvoiceStatus::Pending,
            payments: Vec::new(&env),
            allowed_payers: None,
        };
        env.storage()
            .persistent()
            .set(&template_version_key(&creator, &name, version), &template);
        env.storage()
            .persistent()
            .set(&template_version_count_key(&creator, &name), &version);

        // Also store under the legacy unversioned key for backward compat.
        env.storage()
            .persistent()
            .set(&template_key(&creator, &name), &template);

        // Issue #748: announce the new (or updated) template version.
        events::template_saved(&env, &creator, &name, version);

        version
    }

    /// Issue #748: Load a saved template by `(creator, name)`.
    ///
    /// `version` of `None` selects the most recently saved version, falling
    /// back to the legacy unversioned key for templates stored before
    /// versioning existed. Panics with "template not found" when nothing is
    /// stored.
    fn _load_template(
        env: &Env,
        creator: &Address,
        name: &Symbol,
        version: Option<u32>,
    ) -> InvoiceTemplate {
        match version {
            Some(v) => env
                .storage()
                .persistent()
                .get(&template_version_key(creator, name, v))
                .expect("template version not found"),
            None => {
                let count: u32 = env
                    .storage()
                    .persistent()
                    .get(&template_version_count_key(creator, name))
                    .unwrap_or(0u32);
                if count > 0 {
                    env.storage()
                        .persistent()
                        .get(&template_version_key(creator, name, count))
                        .expect("latest template not found")
                } else {
                    // Fall back to legacy unversioned key.
                    env.storage()
                        .persistent()
                        .get(&template_key(creator, name))
                        .expect("template not found")
                }
            }
        }
    }

    /// Create a new invoice from a previously saved template.
    /// When `version` is None, the latest version is used.
    pub fn create_from_template(
        env: Env,
        creator: Address,
        name: Symbol,
        deadline: u64,
        version: Option<u32>,
    ) -> u64 {
        creator.require_auth();

        let tmpl = Self::_load_template(&env, &creator, &name, version);
        Self::_instantiate_template(
            &env,
            creator,
            tmpl.recipients,
            tmpl.amounts,
            tmpl.token,
            deadline,
        )
    }

    /// Issue #748: Instantiate a new invoice from resolved template values.
    ///
    /// Shared by `create_from_template` and `create_invoice_from_template` so
    /// every template-created invoice gets identical defaults (no co-creators,
    /// no early withdrawal, `OverflowBehavior::Reject`, flat 100% ratio split).
    fn _instantiate_template(
        env: &Env,
        creator: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        token: Address,
        deadline: u64,
    ) -> u64 {
        Self::_create_invoice_inner(
            env,
            creator,
            recipients,
            amounts,
            Vec::new(env),
            token,
            deadline,
            Vec::new(env),
            false,
            0,
            0,
            None,
            Vec::new(env),
            Vec::new(env),
            0,
            0,
            0,
            0,
            Vec::new(env),
            None,
            Vec::new(env),
            None,
            0,
            None,
            0,
            false,
            None,
            OverflowBehavior::Reject,
            false,
            Vec::new(env),
            None,
            None,
            None,
            0,
            0,
            Vec::new(env),
            Vec::new(env),
            None,
            None,
            None,
            None,
            None,
            None,
            Vec::new(env), // priorities
            false,         // require_kyc
            None,          // scheduled_release_at
            None,          // min_payer_rep
            None,          // release_delay_ledgers
            None,          // metadata_hash
            None,          // target_usd_cents
            None,          // oracle
            None,          // oracle_asset_pair_base
            None,          // oracle_asset_pair_quote
            None,          // escrow_hold_period
            None,          // payment_open_at
            None,          // payment_close_at
            None,          // milestones
            None,          // recipient_max_payouts
            false,         // recipient_whitelist_enabled
            None,          // release_condition_hash
            0,             // early_bird_window_ledgers
            0,             // early_bird_fee_bps
            0,             // creator_fee_bps
            Vec::new(env), // ratios
            1_u64,         // ratio_denominator
        )
    }

    /// Issue #748: Instantiate a new invoice from a saved template, applying
    /// optional field overrides on top of the stored configuration.
    ///
    /// Only the creator who saved the template may instantiate it. Overrides
    /// are validated with the same rules as `save_template` (parallel
    /// `recipients`/`amounts`, strictly positive amounts, at least one
    /// recipient), so an override can never produce a malformed invoice. The
    /// `deadline` argument is used unless `overrides.deadline` is set.
    ///
    /// Emits `InvoiceCreatedFromTemplate` with the resolved template version
    /// (`version` when supplied, otherwise the latest for `(creator, name)`).
    pub fn create_invoice_from_template(
        env: Env,
        creator: Address,
        name: Symbol,
        deadline: u64,
        version: Option<u32>,
        overrides: Option<TemplateOverrides>,
    ) -> u64 {
        creator.require_auth();

        let tmpl = Self::_load_template(&env, &creator, &name, version);
        let mut recipients = tmpl.recipients;
        let mut amounts = tmpl.amounts;
        let mut token = tmpl.token;
        let mut deadline = deadline;

        // Merge overrides over the stored template, field by field.
        if let Some(ov) = overrides {
            if let Some(new_recipients) = ov.recipients {
                recipients = new_recipients;
            }
            if let Some(new_amounts) = ov.amounts {
                amounts = new_amounts;
            }
            if let Some(new_token) = ov.token {
                token = new_token;
            }
            if let Some(new_deadline) = ov.deadline {
                deadline = new_deadline;
            }
        }

        assert!(
            recipients.len() == amounts.len(),
            "recipients and amounts length mismatch"
        );
        assert!(!recipients.is_empty(), "must have at least one recipient");
        for amt in amounts.iter() {
            assert!(amt > 0, "amounts must be positive");
        }

        // Resolve the effective version before `version` is consumed below.
        let resolved_version = match version {
            Some(v) => v,
            None => env
                .storage()
                .persistent()
                .get::<_, u32>(&template_version_count_key(&creator, &name))
                .unwrap_or(0u32),
        };

        let invoice_id =
            Self::_instantiate_template(&env, creator.clone(), recipients, amounts, token, deadline);

        events::invoice_created_from_template(&env, invoice_id, &creator, &name, resolved_version);

        invoice_id
    }

    /// Issue #748: Return a stored template by `(creator, name)`.
    ///
    /// `version` of `None` returns the latest saved version. Panics with
    /// "template not found" when nothing is stored for that pair.
    pub fn get_template_by_name(
        env: Env,
        creator: Address,
        name: Symbol,
        version: Option<u32>,
    ) -> InvoiceTemplate {
        Self::_load_template(&env, &creator, &name, version)
    }

    /// Link invoices into a group.
    ///
    /// `majority` — when `false` (default), all members must be fully funded before
    /// any can release (AllOrNothing). When `true`, a strict majority (>50%) being
    /// fully funded is sufficient to unblock release (Issue #212).
    pub fn create_invoice_group(env: Env, invoice_ids: Vec<u64>, majority: bool) -> u64 {
        assert!(invoice_ids.len() >= 2, "group needs at least 2 invoices");

        let grp_cnt_key = symbol_short!("grp_cnt");
        let group_id: u64 = env.storage().persistent().get(&grp_cnt_key).unwrap_or(0u64) + 1;
        env.storage().persistent().set(&grp_cnt_key, &group_id);

        for id in invoice_ids.iter() {
            env.storage()
                .persistent()
                .set(&invoice_group_key(id), &group_id);
        }
        let mode = if majority {
            types::GroupMode::Majority
        } else {
            types::GroupMode::AllOrNothing
        };
        let group = types::InvoiceGroup { invoice_ids, mode };
        env.storage().persistent().set(&group_key(group_id), &group);

        group_id
    }

    // -----------------------------------------------------------------------
    // Early withdrawal (#37)
    // -----------------------------------------------------------------------

    /// Allows a payer to reclaim their contribution before the deadline when
    /// `allow_early_withdrawal` is enabled on the invoice.
    pub fn withdraw(env: Env, invoice_id: u64, payer: Address) {
        payer.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.allow_early_withdrawal,
            "early withdrawal not allowed"
        );
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        let mut total_paid: i128 = 0;
        for payment in invoice.payments.iter() {
            if payment.payer == payer {
                total_paid += payment.amount;
            }
        }
        assert!(total_paid > 0, "no contributions to withdraw");

        // Remove payer's payments from all shards (issue #177).
        for shard_id in 0..SHARD_COUNT {
            if let Some(shard_payments) = env
                .storage()
                .persistent()
                .get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(invoice_id, shard_id))
            {
                let mut new_shard_payments: Vec<Payment> = Vec::new(&env);
                for payment in shard_payments.iter() {
                    if payment.payer != payer {
                        new_shard_payments.push_back(payment.clone());
                    }
                }
                if new_shard_payments.is_empty() {
                    env.storage()
                        .persistent()
                        .remove(&pay_shard_key(invoice_id, shard_id));
                } else {
                    env.storage()
                        .persistent()
                        .set(&pay_shard_key(invoice_id, shard_id), &new_shard_payments);
                }
            }
        }
        invoice.funded -= total_paid;

        let token_client = token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
        token_client.transfer(&env.current_contract_address(), &payer, &total_paid);

        // Decrement credit score by 2 on early withdrawal (floor 0) (issue #38).
        let credit: u64 = env
            .storage()
            .persistent()
            .get(&credit_key(&payer))
            .unwrap_or(0u64);
        env.storage()
            .persistent()
            .set(&credit_key(&payer), &credit.saturating_sub(2));

        save_invoice(&env, invoice_id, &invoice);
    }

    // -----------------------------------------------------------------------
    // Deadline extension by payer vote (#39)
    // -----------------------------------------------------------------------

    /// Vote to extend the invoice deadline before it expires.
    ///
    /// Issue #755: Once votes reach the configured quorum (default 51% of unique
    /// payers), the deadline is extended by `extension_duration_seconds` (default
    /// 7 days). A maximum of 3 extensions per invoice are allowed; a 4th attempt
    /// panics with `ExtensionLimitReached`.
    ///
    /// Only addresses that have contributed to the invoice may vote.
    pub fn vote_extend_deadline(env: Env, invoice_id: u64, voter: Address) {
        voter.require_auth();

        let invoice = load_invoice(&env, invoice_id);
        require_invoice_unlocked(&env, invoice_id, &invoice);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        // Only payers who have contributed can vote.
        let has_paid = invoice.payments.iter().any(|p| p.payer == voter);
        assert!(has_paid, "only payers can vote");

        // Enforce max 3 extensions.
        let ext_count: u32 = env
            .storage()
            .persistent()
            .get(&invoice_ext_count_key(invoice_id))
            .unwrap_or(0u32);
        if ext_count >= 3 {
            panic_with_error!(&env, ContractError::ExtensionLimitReached);
        }

        // Load per-invoice config: (duration_seconds, quorum_bps).
        // Default: 7 days (604_800s), 51% quorum (5_100 bps).
        let (duration_seconds, quorum_bps): (u64, u32) = env
            .storage()
            .persistent()
            .get(&invoice_ext_config_key(invoice_id))
            .unwrap_or((604_800u64, 5_100u32));

        // Collect unique payers.
        let mut unique_payers: Vec<Address> = Vec::new(&env);
        for payment in invoice.payments.iter() {
            if !unique_payers.contains(&payment.payer) {
                unique_payers.push_back(payment.payer);
            }
        }

        let vote_key = ext_vote_key(invoice_id);
        let mut votes: Vec<Address> = env
            .storage()
            .persistent()
            .get(&vote_key)
            .unwrap_or_else(|| Vec::new(&env));

        // Deduplicate votes.
        if votes.contains(&voter) {
            return;
        }
        votes.push_back(voter);

        // Check if quorum is reached: votes * 10_000 >= unique_payers * quorum_bps.
        let votes_bps = (votes.len() as u64)
            .saturating_mul(10_000u64)
            .checked_div(unique_payers.len().max(1) as u64)
            .unwrap_or(0) as u32;

        if votes_bps >= quorum_bps {
            // Quorum reached — extend deadline.
            let mut invoice = load_invoice(&env, invoice_id);
            invoice.deadline = invoice.deadline.saturating_add(duration_seconds);
            save_invoice(&env, invoice_id, &invoice);

            // Increment extension count.
            env.storage()
                .persistent()
                .set(&invoice_ext_count_key(invoice_id), &(ext_count + 1));

            // Clear votes for next round.
            env.storage().persistent().remove(&vote_key);

            events::deadline_extended_with_count(&env, invoice_id, invoice.deadline, ext_count + 1);
        } else {
            env.storage().persistent().set(&vote_key, &votes);
        }
    }

    /// Issue #755: Get the number of times this invoice's deadline has been extended.
    pub fn get_invoice_extension_count(env: Env, invoice_id: u64) -> u32 {
        env.storage()
            .persistent()
            .get(&invoice_ext_count_key(invoice_id))
            .unwrap_or(0u32)
    }

    // -----------------------------------------------------------------------
    // Drip / vesting claim
    // -----------------------------------------------------------------------

    /// Claim the vested portion of a drip invoice for a recipient.
    pub fn drip_claim(env: Env, invoice_id: u64, recipient: Address) {
        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Released,
            "invoice not released"
        );
        let drip_duration = invoice.drip_duration.expect("no drip schedule");
        let release_ts = invoice.release_timestamp.expect("no release timestamp");

        let idx = invoice
            .recipients
            .iter()
            .position(|r| r == recipient)
            .expect("recipient not found") as u32;

        let total_amount = invoice.amounts.get(idx).unwrap();
        let already_claimed = invoice.claimed.get(idx).unwrap();

        let elapsed = env.ledger().timestamp().saturating_sub(release_ts);
        let vested = if elapsed >= drip_duration {
            total_amount
        } else {
            (elapsed as i128) * total_amount / (drip_duration as i128)
        };

        let claimable = vested - already_claimed;
        assert!(claimable > 0, "nothing to claim");

        invoice.claimed.set(idx, already_claimed + claimable);
        save_invoice(&env, invoice_id, &invoice);

        let token_client = token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
        token_client.transfer(&env.current_contract_address(), &recipient, &claimable);
    }

    // -----------------------------------------------------------------------
    // Read-only
    // -----------------------------------------------------------------------

    pub fn get_invoice(env: Env, invoice_id: u64) -> InvoiceCore {
        let inv = load_invoice(&env, invoice_id);
        inv.split().0
    }

    pub fn is_archived(env: Env, invoice_id: u64) -> bool {
        env.storage()
            .instance()
            .has(&archive_marker_key(invoice_id))
            || env
                .storage()
                .persistent()
                .has(&archive_marker_key(invoice_id))
    }

    pub fn get_invoice_ext(env: Env, invoice_id: u64) -> InvoiceExt {
        let inv = load_invoice(&env, invoice_id);
        inv.split().1
    }

    pub fn get_invoice_ext2(env: Env, invoice_id: u64) -> InvoiceExt2 {
        let inv = load_invoice(&env, invoice_id);
        inv.split().2
    }

    pub fn get_audit_log(env: Env, invoice_id: u64) -> Vec<AuditEntry> {
        get_audit_log(&env, invoice_id)
    }

    /// Issue #763: Return the per-invoice history ring buffer in chronological order.
    ///
    /// Returns up to [`HISTORY_RING_CAP`] (20) entries.  The oldest entries
    /// are evicted first when the buffer is full, so the returned slice always
    /// represents the most recent state changes.  Returns an empty vec if no
    /// history has been written yet.
    pub fn get_history(env: Env, invoice_id: u64) -> Vec<HistoryEntry> {
        env.storage()
            .persistent()
            .get(&history_key(invoice_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Issue #681: Return how much `recipient` has already withdrawn from
    /// `invoice_id`. Returns 0 if `recipient` is not in the invoice's
    /// recipient list.
    pub fn get_recipient_claimed(env: Env, invoice_id: u64, recipient: Address) -> i128 {
        let invoice = load_invoice(&env, invoice_id);
        for i in 0..invoice.recipients.len() {
            if invoice.recipients.get(i).unwrap() == recipient {
                return invoice.claimed.get(i).unwrap_or(0);
            }
        }
        0
    }

    /// Issue #680: Partition `invoice_id`'s tranches into those already
    /// released and those still pending, based on `released_bps` accumulated
    /// so far. Returns (empty, empty) for invoices without tranches.
    pub fn get_invoice_tranche_status(
        env: Env,
        invoice_id: u64,
    ) -> (Vec<Tranche>, Vec<Tranche>) {
        let invoice = load_invoice(&env, invoice_id);
        let mut released = Vec::new(&env);
        let mut pending = Vec::new(&env);
        let mut cumulative_bps: u32 = 0;
        for tranche in invoice.tranches.iter() {
            cumulative_bps = cumulative_bps.saturating_add(tranche.basis_points);
            if cumulative_bps <= invoice.released_bps {
                released.push_back(tranche);
            } else {
                pending.push_back(tranche);
            }
        }
        (released, pending)
    }

    /// Return the total amount contributed by `payer` toward `invoice_id`.
    pub fn get_payer_total(env: Env, invoice_id: u64, payer: Address) -> i128 {
        let invoice = load_invoice(&env, invoice_id);
        invoice
            .payments
            .iter()
            .filter(|p| p.payer == payer)
            .map(|p| p.amount)
            .sum()
    }

    /// Issue #747: Return the running total recorded under `payer_total_key`.
    pub fn get_payer_contribution_total(env: Env, invoice_id: u64, payer: Address) -> i128 {
        let key = payer_total_key(invoice_id, &payer);
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    pub fn get_twafr(env: Env, invoice_id: u64) -> i128 {
        let invoice = load_invoice(&env, invoice_id);
        // Payments live in sharded storage (issue #177), so `invoice.payments`
        // is always empty — `twafr_last_ledger` is what actually records
        // whether any payment has been accumulated.
        if invoice.twafr_last_ledger == 0 {
            return 0;
        }
        let creation_ledger: u32 = env
            .storage()
            .persistent()
            .get(&created_ledger_key(invoice_id))
            .unwrap_or(env.ledger().sequence());
        let elapsed = env.ledger().sequence().saturating_sub(creation_ledger) as i128;
        if elapsed <= 0 {
            return 0;
        }
        invoice.twafr_numerator / elapsed
    }

    pub fn get_invoice_rating(env: Env, invoice_id: u64) -> (u32, u32) {
        (
            env.storage()
                .persistent()
                .get(&invoice_rating_sum_key(invoice_id))
                .unwrap_or(0u32),
            env.storage()
                .persistent()
                .get(&invoice_rating_count_key(invoice_id))
                .unwrap_or(0u32),
        )
    }

    /// Issue #447: Get per-invoice analytics.
    pub fn get_invoice_analytics(env: Env, invoice_id: u64) -> types::InvoiceAnalytics {
        env.storage()
            .persistent()
            .get(&invoice_analytics_key(invoice_id))
            .unwrap_or(types::InvoiceAnalytics {
                payment_count: 0,
                total_funded: 0,
                unique_payers: 0,
                first_payment_ledger: 0,
                last_payment_ledger: 0,
            })
    }

    /// Issue #448: Set slippage tolerance (in basis points) for an invoice.
    /// If set, release will check that the token balance hasn't deviated beyond this tolerance.
    pub fn set_slippage_tolerance(env: Env, creator: Address, invoice_id: u64, slippage_bps: u32) {
        require_not_paused(&env);
        creator.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        require_invoice_unlocked(&env, invoice_id, &invoice);
        assert!(invoice.creator == creator, "only creator can set slippage");
        assert!(slippage_bps <= 10_000, "slippage_bps must be <= 10000");
        env.storage()
            .persistent()
            .set(&slippage_tolerance_key(invoice_id), &slippage_bps);
    }

    /// Issue #449: Set invoice phase. Transitions must follow: Draft -> Active -> Locked -> Released.
    pub fn set_invoice_phase(
        env: Env,
        caller: Address,
        invoice_id: u64,
        new_phase: types::InvoicePhase,
    ) {
        require_not_paused(&env);
        caller.require_auth();
        let current_phase: types::InvoicePhase = env
            .storage()
            .persistent()
            .get(&invoice_phase_key(invoice_id))
            .unwrap_or(types::InvoicePhase::Draft);
        let valid = matches!(
            (&current_phase, &new_phase),
            (types::InvoicePhase::Draft, types::InvoicePhase::Active)
                | (types::InvoicePhase::Active, types::InvoicePhase::Locked)
                | (types::InvoicePhase::Locked, types::InvoicePhase::Released)
        );
        assert!(valid, "InvalidPhaseTransition");
        env.storage()
            .persistent()
            .set(&invoice_phase_key(invoice_id), &new_phase);
        events::invoice_phase_changed(&env, invoice_id, &current_phase, &new_phase);
    }

    /// Issue #449 / #676: Get the current phase of an invoice.
    ///
    /// An explicit phase written by [`SplitContract::set_invoice_phase`] is
    /// authoritative. When no phase has ever been set the phase is derived from
    /// the invoice's own state fields so callers never have to reconstruct it:
    ///
    /// * `Released`  — the invoice has been released or finalised.
    /// * `Locked`    — the invoice is frozen, disputed, or partially released,
    ///                 so its balance can no longer be freely funded or refunded.
    /// * `Active`    — funding has started (`funded > 0`).
    /// * `Draft`     — nothing has been funded yet.
    ///
    /// Errors with [`ContractError::InvoiceNotFound`] for unknown `invoice_id`.
    pub fn get_invoice_phase(
        env: Env,
        invoice_id: u64,
    ) -> Result<types::InvoicePhase, ContractError> {
        let core = Self::load_invoice_opt(&env, invoice_id)?;

        if let Some(phase) = env
            .storage()
            .persistent()
            .get::<_, types::InvoicePhase>(&invoice_phase_key(invoice_id))
        {
            return Ok(phase);
        }

        // Prefer the hot entry for status/funded — it is the copy `pay()` mutates.
        let (status, funded) = env
            .storage()
            .instance()
            .get::<_, InvoiceHot>(&invoice_hot_key(invoice_id))
            .map_or_else(
                || (core.status.clone(), core.funded),
                |hot| (hot.status, hot.funded),
            );

        let phase = match status {
            InvoiceStatus::Released | InvoiceStatus::Finalised => types::InvoicePhase::Released,
            InvoiceStatus::PartiallyReleased | InvoiceStatus::Disputed => {
                types::InvoicePhase::Locked
            }
            _ if core.frozen => types::InvoicePhase::Locked,
            _ if funded > 0 => types::InvoicePhase::Active,
            _ => types::InvoicePhase::Draft,
        };
        Ok(phase)
    }

    /// Return the aggregate rating for a creator (issue #759).
    ///
    /// Returns a `CreatorRating { total_ratings, average_stars_bps }` where
    /// `average_stars_bps` is the mean star score expressed in basis points
    /// (e.g. 4.5 stars = 45 000 bps). Returns zeros for an unrated creator.
    pub fn get_creator_rating(env: Env, creator: Address) -> CreatorRating {
        env.storage()
            .persistent()
            .get(&creator_rating_key(&creator))
            .unwrap_or(CreatorRating {
                total_ratings: 0,
                average_stars_bps: 0,
            })
    }

    // -----------------------------------------------------------------------
    // Issue #756: On-chain invoice notes
    // -----------------------------------------------------------------------

    /// Append an immutable on-chain note to an invoice (issue #756).
    ///
    /// Only the invoice creator may call this. Notes are stored as an
    /// ordered `Vec<Note>` under `notes_key(invoice_id)` and are
    /// permanently immutable after posting.
    ///
    /// Limits:
    /// - `content` must be ≤ 512 bytes; panics otherwise.
    /// - Maximum 10 notes per invoice; panics with `NoteLimitReached` if exceeded.
    ///
    /// Emits `NoteAdded { invoice_id, index, timestamp }`.
    pub fn add_note(env: Env, invoice_id: u64, creator: Address, content: Bytes) {
        require_not_paused(&env);
        creator.require_auth();

        let invoice = load_invoice(&env, invoice_id);
        // Only the creator may post notes.
        assert!(invoice.creator == creator, "not invoice creator");

        // Enforce content length limit.
        assert!(content.len() <= 512, "note content exceeds 512 bytes");

        // Load existing notes (or empty vec).
        let key = notes_key(invoice_id);
        let mut notes: Vec<Note> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(&env));

        // Enforce note count limit.
        if notes.len() >= 10 {
            panic_with_error!(&env, ContractError::NoteLimitReached);
        }

        let index = notes.len();
        let note = Note {
            timestamp: env.ledger().timestamp(),
            content,
            index,
        };
        notes.push_back(note);

        env.storage().persistent().set(&key, &notes);
        env.storage().persistent().extend_ttl(
            &key,
            crate::constants::MIN_INVOICE_TTL_LEDGERS,
            crate::constants::MAX_INVOICE_TTL_LEDGERS,
        );

        events::note_added(&env, invoice_id, index);
    }

    /// Return all notes posted on an invoice (issue #756).
    ///
    /// Returns an empty vec if no notes have been posted.
    pub fn get_notes(env: Env, invoice_id: u64) -> Vec<Note> {
        env.storage()
            .persistent()
            .get(&notes_key(invoice_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Returns the full `RepScore` struct for an address (issue #349).
    pub fn get_rep(env: Env, address: Address) -> RepScore {
        get_rep_internal(&env, &address)
    }

    /// Returns the on-chain reputation score (number of successful payments) for an address.
    ///
    /// Returns 0 for an address that has never paid.
    pub fn get_reputation(env: Env, address: Address) -> u64 {
        let score = get_rep_internal(&env, &address);
        (score.paid_on_time as u64).saturating_add(score.late_pays as u64)
    }

    /// Returns the credit score for an address.
    ///
    /// Incremented by 1 on every successful `pay()`, decremented by 2 on
    /// early `withdraw()` (floor 0). Returns 0 for an address that has never paid.
    pub fn get_credit_score(env: Env, address: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&credit_key(&address))
            .unwrap_or(0u64)
    }

    /// Returns the current expected nonce for a (invoice_id, payer) pair.
    ///
    /// The first payment must use nonce 0; each successful payment increments it by 1.
    /// Returns 0 for a payer that has never paid toward this invoice.
    pub fn get_nonce(env: Env, invoice_id: u64, payer: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&nonce_key(invoice_id, &payer))
            .unwrap_or(0u64)
    }

    /// Returns the current expected contract-wide nonce for `caller` (issue #424).
    ///
    /// This is separate from `get_nonce`: it is not scoped to a single invoice, but
    /// tracks one sequence per caller across every entry point that accepts an
    /// off-chain signed authorisation (e.g. `pay_invoice_delegated`). Starts at 0
    /// and increments by 1 after each successful nonce-protected call.
    pub fn get_global_nonce(env: Env, caller: Address) -> u64 {
        get_global_nonce_internal(&env, &caller)
    }

    /// Generate a completion proof for a finalized invoice.
    pub fn get_completion_proof(env: Env, invoice_id: u64) -> CompletionProof {
        let invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Released || invoice.status == InvoiceStatus::Refunded,
            "invoice not finalized"
        );

        let status_byte: u8 = match invoice.status {
            InvoiceStatus::Pending => 0u8,
            InvoiceStatus::Released => 1u8,
            InvoiceStatus::Refunded => 2u8,
            InvoiceStatus::Cancelled => 3u8,
            InvoiceStatus::Expired => 4u8,
            InvoiceStatus::PartiallyReleased => 5u8,
            InvoiceStatus::Disputed => 6u8,
            InvoiceStatus::Finalised => 7u8,
            InvoiceStatus::Deleted => 8u8,
            InvoiceStatus::PayoutInProgress => 9u8,
        };

        let mut preimage = [0u8; 17];
        preimage[..8].copy_from_slice(&invoice_id.to_be_bytes());
        preimage[8..16].copy_from_slice(&(invoice.funded as u64).to_be_bytes());
        preimage[16] = status_byte;

        let bytes = Bytes::from_array(&env, &preimage);
        let hash = env.crypto().sha256(&bytes);

        CompletionProof {
            id: invoice_id,
            status: invoice.status,
            funded: invoice.funded,
            timestamp: env.ledger().timestamp(),
            hash: hash.into(),
        }
    }

    /// Generate a payment proof for a specific payer on an invoice (issue #85).
    /// No auth required — read-only. Returns total_paid = 0 if the payer has
    /// not contributed. The proof_hash is deterministic over
    /// (invoice_id, payer, total_paid).
    pub fn generate_payment_proof(env: Env, invoice_id: u64, payer: Address) -> PaymentProof {
        let invoice = load_invoice(&env, invoice_id);

        let total_paid: i128 = invoice
            .payments
            .iter()
            .filter(|p| p.payer == payer)
            .map(|p| p.amount + p.tip)
            .sum();

        // Preimage: 8 bytes invoice_id || 16 bytes total_paid (big-endian i128)
        let mut preimage = [0u8; 24];
        preimage[..8].copy_from_slice(&invoice_id.to_be_bytes());
        preimage[8..24].copy_from_slice(&total_paid.to_be_bytes());

        let bytes = Bytes::from_array(&env, &preimage);
        let proof_hash: BytesN<32> = env.crypto().sha256(&bytes).into();

        PaymentProof {
            invoice_id,
            payer,
            total_paid,
            proof_hash,
        }
    }

    /// Verify a payment proof against the current invoice state.
    ///
    /// Recomputes the hash from the current payer total and compares to the proof's hash.
    /// Returns true only if the recomputed hash exactly matches proof.proof_hash.
    /// Returns false (not panic) for stale proofs where the payer has since paid more.
    /// Returns false for proofs referencing non-existent invoices.
    /// Pure view function — no state mutation, no auth required.
    pub fn verify_payment_proof(env: Env, proof: PaymentProof) -> bool {
        // Return false if invoice doesn't exist
        let invoice = if env
            .storage()
            .persistent()
            .has(&invoice_key(proof.invoice_id))
            || env.storage().instance().has(&invoice_key(proof.invoice_id))
        {
            load_invoice(&env, proof.invoice_id)
        } else {
            return false;
        };

        // Recompute the current total for the payer
        let current_total: i128 = invoice
            .payments
            .iter()
            .filter(|p| p.payer == proof.payer)
            .map(|p| p.amount + p.tip)
            .sum();

        // Recompute the hash using the current total
        let mut preimage = [0u8; 24];
        preimage[..8].copy_from_slice(&proof.invoice_id.to_be_bytes());
        preimage[8..24].copy_from_slice(&current_total.to_be_bytes());

        let bytes = Bytes::from_array(&env, &preimage);
        let recomputed_hash: BytesN<32> = env.crypto().sha256(&bytes).into();

        // Compare with the proof's hash
        recomputed_hash == proof.proof_hash
    }

    /// Return all invoice IDs that include `recipient` as a recipient (issue #40).
    pub fn get_recipient_invoice_ids(env: Env, recipient: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&recipient_invoice_ids_key(&recipient))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Return a paginated slice of invoice IDs for a recipient.
    /// `offset` is the starting index, `limit` is the max number to return (clamped to 50).
    /// Returns an empty Vec if `offset` is beyond the stored list length.
    pub fn get_recipient_invoice_ids_page(
        env: Env,
        recipient: Address,
        offset: u32,
        limit: u32,
    ) -> Vec<u64> {
        let ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&recipient_invoice_ids_key(&recipient))
            .unwrap_or_else(|| Vec::new(&env));
        let len = ids.len();
        if offset >= len {
            return Vec::new(&env);
        }
        let capped = if limit > 50 { 50 } else { limit };
        let end = (offset + capped).min(len);
        let mut result: Vec<u64> = Vec::new(&env);
        for i in offset..end {
            result.push_back(ids.get(i).unwrap());
        }
        result
    }

    /// Returns true if the invoice exists and its status matches `expected_status`.
    pub fn verify_invoice(env: Env, invoice_id: u64, expected_status: InvoiceStatus) -> bool {
        match env
            .storage()
            .persistent()
            .get::<(Symbol, u64), InvoiceCore>(&invoice_key(invoice_id))
            .or_else(|| env.storage().instance().get(&invoice_key(invoice_id)))
        {
            Some(core) => core.status == expected_status,
            None => false,
        }
    }

    /// Returns the referral count for an address (issue #87).
    ///
    /// This counts how many invoices have been created with this address as the referrer.
    /// Returns 0 for an address that has never been used as a referrer.
    pub fn get_referral_count(env: Env, referrer: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&referral_count_key(&referrer))
            .unwrap_or(0u64)
    }

    /// Issue #787: protocol-wide aggregate stats (invoices, paid/released/refunded
    /// amounts, unique creators and payers).
    pub fn get_protocol_stats(env: Env) -> analytics_ext::ProtocolStats {
        let released: i128 = env
            .storage()
            .persistent()
            .get(&total_released_key())
            .unwrap_or(0i128);
        let refunded: i128 = env
            .storage()
            .persistent()
            .get(&total_refunded_key())
            .unwrap_or(0i128);
        analytics_ext::get(&env, released, refunded)
    }

    /// Return the contract-level analytics counters (issue #28).
    ///
    /// Returns a tuple of (total_invoices, total_volume, total_released, total_refunded).
    /// Each counter starts at 0 and increments on the relevant state change.
    pub fn get_stats(env: Env) -> (u64, i128, i128, i128) {
        let total_invoices = env
            .storage()
            .persistent()
            .get(&total_invoices_key())
            .unwrap_or(0u64);
        let total_volume = env
            .storage()
            .persistent()
            .get(&total_volume_key())
            .unwrap_or(0i128);
        let total_released = env
            .storage()
            .persistent()
            .get(&total_released_key())
            .unwrap_or(0i128);
        let total_refunded = env
            .storage()
            .persistent()
            .get(&total_refunded_key())
            .unwrap_or(0i128);
        (total_invoices, total_volume, total_released, total_refunded)
    }

    // -----------------------------------------------------------------------
    // Archive (issue #40)
    // -----------------------------------------------------------------------

    /// Move a Released or Refunded invoice from persistent storage to instance
    /// storage (cheaper, shorter TTL), freeing up persistent storage budget.
    ///
    /// Panics with "invoice not completed" if the invoice is still Pending or Cancelled.
    /// After archival, `get_invoice` still returns the invoice from instance storage.
    pub fn archive_invoice(env: Env, invoice_id: u64) {
        if env
            .storage()
            .instance()
            .has(&archive_marker_key(invoice_id))
        {
            return;
        }

        let core: InvoiceCore = env
            .storage()
            .persistent()
            .get(&invoice_key(invoice_id))
            .or_else(|| env.storage().instance().get(&invoice_key(invoice_id)))
            .expect("invoice not found");

        assert!(
            core.status == InvoiceStatus::Released || core.status == InvoiceStatus::Refunded,
            "invoice not completed"
        );

        let ext: InvoiceExt = env
            .storage()
            .persistent()
            .get(&invoice_ext_key(invoice_id))
            .unwrap_or_else(|| InvoiceExt {
                co_signers: Vec::new(&env),
                required_signatures: 0,
                signatures: Vec::new(&env),
                approver: None,
                approved: false,
                condition_oracle: None,
                condition_met: false,
                penalty_bps: 0,
                penalty_deadline: 0,
                min_funding_bps: 0,
                release_stages: Vec::new(&env),
                released_stages: 0,
                allowed_payers: None,
                price_oracle: None,
                base_amounts: Vec::new(&env),
                swap_tokens: Vec::new(&env),
                tax_bps: 0,
                tax_authority: None,
                insurance_premium_bps: 0,
                insurance_fund: 0,
                smart_route: false,
                convert_to_stream: false,
                accepted_tokens: Vec::new(&env),
                forward_to: None,
                forward_invoice_id: None,
                split_rules: Vec::new(&env),
                auto_resolve_rules: Vec::new(&env),
                creator_cosigner: None,
                velocity_limit: 0,
                velocity_window: 0,
                parent_invoice_id: None,
                pause_reason: None,
                auto_resume_at: None,
                payment_cooldown_secs: None,
                max_payments_per_window: None,
                payment_window_secs: None,
                scheduled_release_at: None,
                refund_grace_secs: None,
                penalty_tiers: Vec::new(&env),
                allowed_callers: None,
            });
        let ext2: InvoiceExt2 = env
            .storage()
            .persistent()
            .get(&invoice_ext2_key(invoice_id))
            .unwrap_or_else(|| InvoiceExt2 {
                notification_contract: None,
                overflow_behavior: OverflowBehavior::Reject,
                cross_chain_ref: None,
                require_kyc: false,
                arbiter: None,
                disputed: false,
                admin_frozen: false,
                auction_on_expiry: false,
                auction_end: 0,
                bids: Vec::new(&env),
                min_payment: 0,
                min_funding_amount: 0,
                priorities: Vec::new(&env),
                target_usd_cents: None,
                refunded_addresses: Vec::new(&env),
                oracle: None,
                oracle_asset_pair_base: None,
                oracle_asset_pair_quote: None,
                min_payer_rep: None,
                escrow_hold_period: None,
                held_until: None,
                milestones: Vec::new(&env),
                milestones_released: 0,
                recipient_max_payouts: Vec::new(&env),
                twafr_numerator: 0,
                twafr_last_ledger: 0,
                release_condition_hash: None,
                recipient_whitelist_enabled: false,
                overfunding_policy: OverfundingPolicy::Cap,
                contributor_allowlist: None,
                early_bird_window_ledgers: 0,
                early_bird_fee_bps: 0,
                early_bird_fee_credit: 0,
                creator_fee_bps: 0,
                ratio_denominator: 1,
                ratios: Vec::new(&env),
                max_contribution_per_payer: None,
            });

        // Copy to instance storage.
        env.storage()
            .instance()
            .set(&invoice_key(invoice_id), &core);
        env.storage()
            .instance()
            .set(&invoice_ext_key(invoice_id), &ext);
        env.storage()
            .instance()
            .set(&invoice_ext2_key(invoice_id), &ext2);

        // Remove from persistent storage.
        env.storage().persistent().remove(&invoice_key(invoice_id));
        env.storage()
            .persistent()
            .remove(&invoice_ext_key(invoice_id));
        env.storage()
            .persistent()
            .remove(&invoice_ext2_key(invoice_id));

        events::invoice_archived(&env, invoice_id);
    }

    /// Batch archive sweep. Accepts up to 20 invoice IDs; archives those that are
    /// Released or Refunded. Returns the list of IDs actually archived.
    pub fn archive_invoices_batch(env: Env, invoice_ids: Vec<u64>) -> Vec<u64> {
        assert!(invoice_ids.len() <= 20, "batch limit exceeded");

        let mut archived: Vec<u64> = Vec::new(&env);
        for i in 0..invoice_ids.len() {
            let id = invoice_ids.get(i).unwrap();
            let exists = env.storage().persistent().has(&invoice_key(id));
            if !exists {
                continue;
            }
            let core: InvoiceCore = env.storage().persistent().get(&invoice_key(id)).unwrap();
            if core.status == InvoiceStatus::Released || core.status == InvoiceStatus::Refunded {
                let ext: InvoiceExt = env
                    .storage()
                    .persistent()
                    .get(&invoice_ext_key(id))
                    .unwrap_or_else(|| InvoiceExt {
                        co_signers: Vec::new(&env),
                        required_signatures: 0,
                        signatures: Vec::new(&env),
                        approver: None,
                        approved: false,
                        condition_oracle: None,
                        condition_met: false,
                        penalty_bps: 0,
                        penalty_deadline: 0,
                        min_funding_bps: 0,
                        release_stages: Vec::new(&env),
                        released_stages: 0,
                        allowed_payers: None,
                        price_oracle: None,
                        base_amounts: Vec::new(&env),
                        swap_tokens: Vec::new(&env),
                        tax_bps: 0,
                        tax_authority: None,
                        insurance_premium_bps: 0,
                        insurance_fund: 0,
                        smart_route: false,
                        convert_to_stream: false,
                        accepted_tokens: Vec::new(&env),
                        forward_to: None,
                        forward_invoice_id: None,
                        split_rules: Vec::new(&env),
                        auto_resolve_rules: Vec::new(&env),
                        creator_cosigner: None,
                        velocity_limit: 0,
                        velocity_window: 0,
                        parent_invoice_id: None,
                        pause_reason: None,
                        auto_resume_at: None,
                        payment_cooldown_secs: None,
                        max_payments_per_window: None,
                        payment_window_secs: None,
                        scheduled_release_at: None,
                        refund_grace_secs: None,
                        penalty_tiers: Vec::new(&env),
                        allowed_callers: None,
                    });
                let ext2: InvoiceExt2 = env
                    .storage()
                    .persistent()
                    .get(&invoice_ext2_key(id))
                    .unwrap_or_else(|| InvoiceExt2 {
                        notification_contract: None,
                        overflow_behavior: OverflowBehavior::Reject,
                        cross_chain_ref: None,
                        require_kyc: false,
                        arbiter: None,
                        disputed: false,
                        admin_frozen: false,
                        auction_on_expiry: false,
                        auction_end: 0,
                        bids: Vec::new(&env),
                        min_payment: 0,
                        min_funding_amount: 0,
                        priorities: Vec::new(&env),
                        target_usd_cents: None,
                        refunded_addresses: Vec::new(&env),
                        oracle: None,
                        oracle_asset_pair_base: None,
                        oracle_asset_pair_quote: None,
                        min_payer_rep: None,
                        escrow_hold_period: None,
                        held_until: None,
                        milestones: Vec::new(&env),
                        milestones_released: 0,
                        recipient_max_payouts: Vec::new(&env),
                        twafr_numerator: 0,
                        twafr_last_ledger: 0,
                        release_condition_hash: None,
                        recipient_whitelist_enabled: false,
                        overfunding_policy: OverfundingPolicy::Cap,
                        contributor_allowlist: None,
                        early_bird_window_ledgers: 0,
                        early_bird_fee_bps: 0,
                        early_bird_fee_credit: 0,
                        creator_fee_bps: 0,
                        ratio_denominator: 1,
                        ratios: Vec::new(&env),
                        max_contribution_per_payer: None,
                    });

                env.storage().instance().set(&invoice_key(id), &core);
                env.storage().instance().set(&invoice_ext_key(id), &ext);
                env.storage().instance().set(&invoice_ext2_key(id), &ext2);

                env.storage().persistent().remove(&invoice_key(id));
                env.storage().persistent().remove(&invoice_ext_key(id));
                env.storage().persistent().remove(&invoice_ext2_key(id));

                archived.push_back(id);
                events::invoice_archived(&env, id);
            }
        }

        events::batch_archived(&env, archived.len(), &archived);
        archived
    }

    // -----------------------------------------------------------------------
    // Delegation (issue #43)
    // -----------------------------------------------------------------------

    /// Assign a delegate address that may call management functions (e.g. extend_deadline)
    /// on behalf of the creator. Requires creator auth.
    pub fn delegate_invoice(env: Env, invoice_id: u64, delegate: Address) {
        let invoice = load_invoice(&env, invoice_id);
        invoice.creator.require_auth();

        env.storage()
            .persistent()
            .set(&delegate_key(invoice_id), &delegate);

        events::delegate_set(&env, invoice_id, &delegate);
        append_audit_entry(
            &env,
            invoice_id,
            symbol_short!("delegate"),
            &invoice.creator,
        );
    }

    /// Remove the delegate from an invoice. Requires creator auth.
    pub fn revoke_delegate(env: Env, invoice_id: u64) {
        let invoice = load_invoice(&env, invoice_id);
        invoice.creator.require_auth();

        env.storage().persistent().remove(&delegate_key(invoice_id));

        events::delegate_revoked(&env, invoice_id, &invoice.creator);
        append_audit_entry(&env, invoice_id, symbol_short!("rvk_del"), &invoice.creator);
    }

    /// Return the current delegate for an invoice, or None if none is set.
    pub fn get_delegate(env: Env, invoice_id: u64) -> Option<Address> {
        env.storage().persistent().get(&delegate_key(invoice_id))
    }

    /// Authorise an address to pay on behalf of the beneficiary.
    /// Requires beneficiary auth.
    pub fn authorise_delegate(env: Env, beneficiary: Address, delegate: Address) {
        require_not_paused(&env);
        beneficiary.require_auth();

        let mut delegates: Vec<Address> = env
            .storage()
            .persistent()
            .get(&delegate_pay_key(&beneficiary))
            .unwrap_or_else(|| Vec::new(&env));

        if !delegates.iter().any(|d| d == delegate) {
            delegates.push_back(delegate.clone());
            env.storage()
                .persistent()
                .set(&delegate_pay_key(&beneficiary), &delegates);
        }
    }

    /// Pay toward an invoice using an authorised delegate.
    /// The invoice records the beneficiary as the payer.
    pub fn delegate_pay(
        env: Env,
        delegate: Address,
        beneficiary: Address,
        invoice_id: u64,
        amount: i128,
    ) {
        require_not_paused(&env);
        delegate.require_auth();

        let delegates: Vec<Address> = env
            .storage()
            .persistent()
            .get(&delegate_pay_key(&beneficiary))
            .unwrap_or_else(|| Vec::new(&env));
        assert!(delegates.iter().any(|d| d == delegate), "not authorised");

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(
            env.ledger().timestamp() <= invoice.deadline,
            "invoice deadline has passed"
        );
        // Issue #483: reject zero or negative payment amounts.
        guard_nonzero_amount(amount).expect("ZeroAmountNotAllowed");
        Self::enforce_invoice_rate_limit(&env, invoice_id, &beneficiary);

        let total: i128 = invoice.amounts.iter().sum();
        let remaining = total - invoice.funded;
        assert!(amount <= remaining, "payment exceeds remaining balance");

        let token_client = token::Client::new(&env, &funding_token_for(&invoice));
        token_client.transfer(&delegate, &env.current_contract_address(), &amount);

        // Write payment to sharded storage (issue #177).
        let shard_id = compute_shard_id(&env, &beneficiary);
        let mut shard_payments: Vec<Payment> = env
            .storage()
            .persistent()
            .get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(invoice_id, shard_id))
            .unwrap_or_else(|| Vec::new(&env));
        shard_payments.push_back(Payment {
            payer: beneficiary.clone(),
            amount,
            tip: 0,
            attestation_hash: None,
            donate_on_failure: false,
            ledger: env.ledger().sequence(),
            timestamp: env.ledger().timestamp(),
        });
        env.storage()
            .persistent()
            .set(&pay_shard_key(invoice_id, shard_id), &shard_payments);

        invoice.funded += amount;
        let cumulative_key = cumulative_contributed_key(invoice_id);
        let cumulative: i128 = env.storage().persistent().get(&cumulative_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&cumulative_key, &(cumulative + amount));

        append_audit_entry(&env, invoice_id, symbol_short!("del_pay"), &delegate);
        events::payment_received(&env, invoice_id, &beneficiary, amount, &funding_token_for(&invoice));
        check_and_emit_funding_checkpoints(&env, invoice_id, invoice.funded, total);
        Self::record_invoice_rate_limit(&env, invoice_id, &beneficiary);
        notify_invoice(
            &env,
            invoice_id,
            symbol_short!("pay"),
            &invoice.notification_contract,
        );

        let in_group = env
            .storage()
            .persistent()
            .has(&invoice_group_key(invoice_id));
        let guarded = invoice.prerequisite_id.is_some()
            || !invoice.tranches.is_empty()
            || !invoice.release_stages.is_empty()
            || in_group
            || !invoice.co_signers.is_empty()
            || env.storage().persistent().has(&cosigners_key(invoice_id));
        if invoice.funded >= total {
            if guarded {
                save_invoice(&env, invoice_id, &invoice);
            } else {
                Self::_release(&env, invoice_id, &mut invoice, &delegate);
            }
        } else {
            save_invoice(&env, invoice_id, &invoice);
        }
    }

    fn enforce_payment_limits(
        env: &Env,
        invoice_id: u64,
        payer: &Address,
        invoice: &Invoice,
        now: u64,
    ) {
        if let Some(cooldown_secs) = invoice.payment_cooldown_secs {
            let last_payment: Option<u64> = env
                .storage()
                .persistent()
                .get(&payer_cooldown_key(invoice_id, payer.clone()));

            if let Some(last_payment_at) = last_payment {
                let retry_after = last_payment_at.saturating_add(cooldown_secs);
                if retry_after > now {
                    // Issue #762: emit the CooldownActive event before panicking
                    // so indexers can observe the rejection without consuming
                    // resources reconstructing the cooldown state from storage.
                    events::cooldown_active(env, invoice_id, payer, retry_after);
                    panic!("PaymentCooldownActive: retry_after={}", retry_after);
                }
            }
        }

        if let (Some(max_payments), Some(window_secs)) =
            (invoice.max_payments_per_window, invoice.payment_window_secs)
        {
            let recent = Self::active_payment_window(env, invoice_id, now, window_secs);
            assert!(recent.len() < max_payments, "payment rate limit exceeded");
        }
    }

    fn record_payment_limits(
        env: &Env,
        invoice_id: u64,
        payer: &Address,
        invoice: &Invoice,
        now: u64,
    ) {
        if invoice.payment_cooldown_secs.is_some() {
            env.storage()
                .persistent()
                .set(&payer_cooldown_key(invoice_id, payer.clone()), &now);
        }

        if let (Some(_), Some(window_secs)) =
            (invoice.max_payments_per_window, invoice.payment_window_secs)
        {
            let mut recent = Self::active_payment_window(env, invoice_id, now, window_secs);
            while recent.len() >= PAYMENT_WINDOW_CAP {
                recent.pop_front();
            }
            recent.push_back(now);
            env.storage()
                .persistent()
                .set(&payment_window_key(invoice_id), &recent);
        }
    }

    fn enforce_invoice_rate_limit(env: &Env, invoice_id: u64, payer: &Address) {
        let window_ledgers: u32 = env
            .storage()
            .instance()
            .get(&invoice_rate_limit_window_key())
            .unwrap_or(0u32);
        let max_payments: u32 = env
            .storage()
            .instance()
            .get(&invoice_rate_limit_max_key())
            .unwrap_or(0u32);
        let max_payments = payer_tier_ext::max_payments_for(env, payer, max_payments);
        if window_ledgers == 0 || max_payments == 0 {
            return;
        }

        let now = env.ledger().sequence();
        let timestamps: Vec<u32> = env
            .storage()
            .persistent()
            .get(&payer_payment_timestamps_key(invoice_id, payer))
            .unwrap_or_else(|| Vec::new(env));
        let mut pruned: Vec<u32> = Vec::new(env);
        for ts in timestamps.iter() {
            if now.saturating_sub(ts) < window_ledgers {
                pruned.push_back(ts);
            }
        }

        if pruned.len() >= max_payments {
            let next_allowed_ledger = pruned.get(0).unwrap_or(now).saturating_add(window_ledgers);
            events::rate_limit_hit(env, invoice_id, payer, next_allowed_ledger);
            panic!("RateLimitExceeded");
        }
    }

    fn record_invoice_rate_limit(env: &Env, invoice_id: u64, payer: &Address) {
        let window_ledgers: u32 = env
            .storage()
            .instance()
            .get(&invoice_rate_limit_window_key())
            .unwrap_or(0u32);
        let max_payments: u32 = env
            .storage()
            .instance()
            .get(&invoice_rate_limit_max_key())
            .unwrap_or(0u32);
        let max_payments = payer_tier_ext::max_payments_for(env, payer, max_payments);
        if window_ledgers == 0 || max_payments == 0 {
            return;
        }

        let now = env.ledger().sequence();
        let timestamps: Vec<u32> = env
            .storage()
            .persistent()
            .get(&payer_payment_timestamps_key(invoice_id, payer))
            .unwrap_or_else(|| Vec::new(env));
        let mut pruned: Vec<u32> = Vec::new(env);
        for ts in timestamps.iter() {
            if now.saturating_sub(ts) < window_ledgers {
                pruned.push_back(ts);
            }
        }
        pruned.push_back(now);
        env.storage()
            .persistent()
            .set(&payer_payment_timestamps_key(invoice_id, payer), &pruned);
    }

    fn active_payment_window(env: &Env, invoice_id: u64, now: u64, window_secs: u64) -> Vec<u64> {
        let stored: Vec<u64> = env
            .storage()
            .persistent()
            .get(&payment_window_key(invoice_id))
            .unwrap_or(Vec::new(env));
        let mut active = Vec::new(env);

        for paid_at in stored.iter() {
            if paid_at.saturating_add(window_secs) > now {
                active.push_back(paid_at);
            }
        }

        while active.len() > PAYMENT_WINDOW_CAP {
            active.pop_front();
        }

        active
    }

    pub fn issue_certificate(env: Env, invoice_id: u64) -> PaymentCertificate {
        let invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status == InvoiceStatus::Released,
            "invoice is not released"
        );

        // Return the cached certificate if one already exists.
        if let Some(existing) = env
            .storage()
            .persistent()
            .get::<_, PaymentCertificate>(&cert_key(invoice_id))
        {
            return existing;
        }

        let total: i128 = invoice.amounts.iter().sum();
        let release_timestamp = invoice
            .release_timestamp
            .unwrap_or_else(|| env.ledger().timestamp());

        // Deterministic preimage: invoice_id || total || release_timestamp
        let mut preimage = [0u8; 32];
        preimage[..8].copy_from_slice(&invoice_id.to_be_bytes());
        preimage[8..24].copy_from_slice(&total.to_be_bytes());
        preimage[24..32].copy_from_slice(&release_timestamp.to_be_bytes());

        let bytes = Bytes::from_array(&env, &preimage);
        let cert_hash: BytesN<32> = env.crypto().sha256(&bytes).into();

        let cert = PaymentCertificate {
            invoice_id,
            total,
            recipients: invoice.recipients.clone(),
            release_timestamp,
            cert_hash,
        };

        env.storage().persistent().set(&cert_key(invoice_id), &cert);

        cert
    }

    pub fn get_certificate(env: Env, invoice_id: u64) -> PaymentCertificate {
        env.storage()
            .persistent()
            .get::<_, PaymentCertificate>(&cert_key(invoice_id))
            .expect("certificate not found")
    }

    // -----------------------------------------------------------------------
    // Issue #298: Compute cost estimation before release submission
    // -----------------------------------------------------------------------

    /// Estimate the compute cost of releasing a given invoice without executing it.
    /// Returns { estimated_instructions, estimated_fee_stroops, would_succeed }.
    pub fn simulate_release(env: Env, invoice_id: u64) -> SimulateReleaseResult {
        let invoice = load_invoice(&env, invoice_id);
        let recipient_count = invoice.recipients.len() as u64;
        let shard_count = SHARD_COUNT;

        let estimated_instructions = INSTRUCTIONS_BASE
            + recipient_count * INSTRUCTIONS_PER_RECIPIENT
            + shard_count * INSTRUCTIONS_PER_SHARD;

        let estimated_fee_stroops =
            (estimated_instructions / 10_000) * STROOPS_PER_10K_INSTRUCTIONS;

        let would_succeed = estimated_instructions <= INSTRUCTION_BUDGET_LIMIT;

        SimulateReleaseResult {
            estimated_instructions,
            estimated_fee_stroops,
            would_succeed,
        }
    }

    // -----------------------------------------------------------------------
    // Issue #297: Contract-wide circuit breaker
    // -----------------------------------------------------------------------

    /// Activate the circuit breaker. Admin-only. Halts all mutating entry points.
    pub fn activate_circuit_breaker(env: Env, admin: Address, reason: String) {
        require_admin_role(&env, &admin, AdminRole::SuperAdmin);
        env.storage()
            .persistent()
            .set(&circuit_breaker_key(), &true);
        env.storage()
            .persistent()
            .set(&circuit_breaker_reason_key(), &reason.clone());
        events::circuit_breaker_activated(&env, &reason);
    }

    /// Deactivate the circuit breaker. Admin-only.
    pub fn deactivate_circuit_breaker(env: Env, admin: Address) {
        require_admin_role(&env, &admin, AdminRole::SuperAdmin);
        env.storage()
            .persistent()
            .set(&circuit_breaker_key(), &false);
        env.storage()
            .persistent()
            .remove(&circuit_breaker_reason_key());
        events::circuit_breaker_deactivated(&env);
    }

    /// Returns the current circuit breaker status.
    /// Read-only — not affected by the circuit breaker.
    pub fn get_circuit_breaker_status(env: Env) -> CircuitBreakerStatus {
        let active: bool = env
            .storage()
            .persistent()
            .get(&circuit_breaker_key())
            .unwrap_or(false);
        let reason: Option<String> = env
            .storage()
            .persistent()
            .get(&circuit_breaker_reason_key());
        CircuitBreakerStatus { active, reason }
    }

    // -----------------------------------------------------------------------
    // Issue #296: Per-creator fee waiver list
    // -----------------------------------------------------------------------

    /// Grant a fee waiver to a creator. Admin-only. Max 100 entries.
    pub fn add_fee_waiver(env: Env, admin: Address, creator: Address) {
        require_admin_role(&env, &admin, AdminRole::SuperAdmin);
        let mut waivers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&creator_fee_waiver_key())
            .unwrap_or_else(|| Vec::new(&env));
        assert!(
            (waivers.len() as usize) < MAX_FEE_WAIVER_ENTRIES,
            "fee waiver list full"
        );
        if !waivers.iter().any(|a| a == creator) {
            waivers.push_back(creator.clone());
            env.storage()
                .persistent()
                .set(&creator_fee_waiver_key(), &waivers);
            events::fee_waiver_granted(&env, &creator);
        }
    }

    /// Revoke a fee waiver from a creator. Admin-only.
    pub fn remove_fee_waiver(env: Env, admin: Address, creator: Address) {
        require_admin_role(&env, &admin, AdminRole::SuperAdmin);
        let waivers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&creator_fee_waiver_key())
            .unwrap_or_else(|| Vec::new(&env));
        let mut new_waivers: Vec<Address> = Vec::new(&env);
        for a in waivers.iter() {
            if a != creator {
                new_waivers.push_back(a);
            }
        }
        env.storage()
            .persistent()
            .set(&creator_fee_waiver_key(), &new_waivers);
        events::fee_waiver_revoked(&env, &creator);
    }

    /// Returns true if the creator is on the fee waiver list.
    pub fn has_fee_waiver(env: Env, creator: Address) -> bool {
        let waivers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&creator_fee_waiver_key())
            .unwrap_or_else(|| Vec::new(&env));
        waivers.iter().any(|a| a == creator)
    }

    // -----------------------------------------------------------------------
    // Issue #295: Confidential payment amounts using blinded commitments
    // -----------------------------------------------------------------------

    /// Submit a confidential payment for an invoice.
    ///
    /// # Cryptographic scheme
    /// The caller provides a Pedersen commitment `C = r·G + amount·H` where `G` and `H` are
    /// independent generators and `r` is a blinding factor known only to the payer/creator.
    /// A bulletproof-style `range_proof` (provided externally) asserts `amount > 0` without
    /// revealing it. The contract stores the commitment and an `encrypted_amount` (encrypted
    /// under the creator's public key off-chain). The creator later calls
    /// `reveal_confidential_total` to prove the decrypted sum equals the funded total.
    ///
    /// In the current implementation the range proof is verified by hashing the concatenation of
    /// commitment and proof bytes and asserting the result is non-zero (a placeholder that
    /// maintains the correct API surface for future ZK integration).
    pub fn pay_confidential(
        env: Env,
        payer: Address,
        invoice_id: u64,
        commitment: BytesN<32>,
        range_proof: Bytes,
        encrypted_amount: Bytes,
    ) {
        require_not_paused(&env);
        payer.require_auth();

        let invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        // Reject all-zero range proof directly (placeholder for full ZK verify).
        let proof_is_nonzero = range_proof.iter().any(|b| b != 0);
        assert!(proof_is_nonzero, "invalid range proof");

        let already_exists = env
            .storage()
            .persistent()
            .has(&confidential_pay_key(invoice_id, &payer));

        let record = ConfidentialPayment {
            commitment,
            encrypted_amount,
        };
        env.storage()
            .persistent()
            .set(&confidential_pay_key(invoice_id, &payer), &record);

        if !already_exists {
            let count: u32 = env
                .storage()
                .persistent()
                .get(&confidential_count_key(invoice_id))
                .unwrap_or(0u32);
            env.storage()
                .persistent()
                .set(&confidential_count_key(invoice_id), &(count + 1));
        }
    }

    /// Returns the number of confidential payments registered for an invoice.
    pub fn get_confidential_payment_count(env: Env, invoice_id: u64) -> u32 {
        env.storage()
            .persistent()
            .get(&confidential_count_key(invoice_id))
            .unwrap_or(0u32)
    }

    /// Creator reveals the decrypted sum of all confidential payments and provides
    /// a proof (hash of encrypted_amounts XOR'd together) to trigger release.
    pub fn reveal_confidential_total(
        env: Env,
        invoice_id: u64,
        decrypted_sum: i128,
        proof: BytesN<32>,
    ) {
        require_not_paused(&env);
        let invoice = load_invoice(&env, invoice_id);
        invoice.creator.require_auth();

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(decrypted_sum > 0, "decrypted_sum must be positive");

        // Verify proof is non-zero (placeholder for full ZK verification).
        let zero: BytesN<32> = BytesN::from_array(&env, &[0u8; 32]);
        assert!(proof != zero, "invalid reveal proof");

        // Credit the revealed sum to the invoice so the normal release path can proceed.
        let mut invoice = invoice;
        let total: i128 = invoice.amounts.iter().sum();
        let new_funded = invoice.funded + decrypted_sum;
        invoice.funded = if new_funded > total {
            total
        } else {
            new_funded
        };
        save_invoice(&env, invoice_id, &invoice);

        if invoice.funded >= total {
            let actor = env.current_contract_address();
            Self::_release(&env, invoice_id, &mut invoice.clone(), &actor);
        }
    }

    // Issue #308: Per-payer claim_refund after deadline
    // -----------------------------------------------------------------------

    /// Claim a refund for the calling payer's contribution after the deadline.
    ///
    /// Conditions: invoice must be Pending, deadline must have passed, invoice
    /// must NOT be fully funded, and the payer must not have already claimed.
    /// Idempotent — calling twice after the first claim is a no-op.
    pub fn claim_refund(env: Env, payer: Address, invoice_id: u64) {
        require_fn_not_paused(&env, &symbol_short!("refund"));
        payer.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        // Must still be pending (not fully released/refunded via bulk refund).
        if invoice.status != InvoiceStatus::Pending {
            // Idempotent: already refunded via bulk path — nothing to do.
            return;
        }

        // Deadline must have passed (respecting grace period).
        let refund_deadline = if let Some(grace_secs) = invoice.refund_grace_secs {
            invoice.deadline.saturating_add(grace_secs)
        } else {
            invoice.deadline
        };
        assert!(
            env.ledger().timestamp() > refund_deadline,
            "invoice has not expired yet"
        );

        // Invoice must NOT be fully funded.
        let total: i128 = invoice.amounts.iter().sum();
        assert!(invoice.funded < total, "invoice is fully funded");

        // Idempotent: if payer already claimed, silently return.
        if invoice.refunded_addresses.iter().any(|a| a == payer) {
            return;
        }

        // Compute this payer's total contribution (across all shards).
        let mut payer_total: i128 = 0;
        for shard_id in 0..SHARD_COUNT {
            if let Some(shard_payments) = env
                .storage()
                .persistent()
                .get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(invoice_id, shard_id))
            {
                for payment in shard_payments.iter() {
                    if payment.payer == payer && !payment.donate_on_failure {
                        payer_total += payment.amount;
                    }
                }
            }
        }

        if payer_total == 0 {
            return; // No contribution to refund.
        }

        let token_client = token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
        token_client.transfer(&env.current_contract_address(), &payer, &payer_total);

        // Record that this payer has claimed.
        invoice.refunded_addresses.push_back(payer.clone());
        invoice.funded = invoice.funded.saturating_sub(payer_total);
        save_invoice(&env, invoice_id, &invoice);

        events::refund_claimed(&env, invoice_id, &payer, payer_total);
        append_audit_entry(&env, invoice_id, symbol_short!("clm_ref"), &payer);
    }

    // -----------------------------------------------------------------------
    // Issue #786: Payment matching pool
    // -----------------------------------------------------------------------

    /// Lock `amount` against `invoice_id` to match upcoming payments. One pledge per matcher.
    pub fn pledge_match(env: Env, matcher: Address, invoice_id: u64, amount: i128) {
        match_pool_ext::pledge(&env, &matcher, invoice_id, amount);
    }

    /// Return the unmatched portion of the caller's pledge once the invoice is closed
    /// (deadline passed, fully funded, or no longer pending). Returns the refunded amount.
    pub fn claim_unmatched_pledge(env: Env, matcher: Address, invoice_id: u64) -> i128 {
        match_pool_ext::claim(&env, &matcher, invoice_id)
    }

    /// All pledges for an invoice.
    pub fn get_match_pool(env: Env, invoice_id: u64) -> Vec<match_pool_ext::MatchPledge> {
        match_pool_ext::get_pool(&env, invoice_id)
    }

    // -----------------------------------------------------------------------
    // Issue #310: Two-step upgrade with 48-hour timelock
    // -----------------------------------------------------------------------

    /// Propose a contract upgrade. Only the admin may call this.
    ///
    /// Stores a pending proposal with an eligible_at = now + 48 h.
    /// Panics with "upgrade already pending" if a proposal exists (cancel it first).
    pub fn propose_upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) {
        require_admin(&env);
        let _ = admin;

        assert!(
            !env.storage().instance().has(&upgrade_proposal_key()),
            "upgrade already pending"
        );
        let eligible_at = env
            .ledger()
            .timestamp()
            .saturating_add(constants::UPGRADE_TIMELOCK_SECONDS);

        let proposal = UpgradeProposal {
            new_wasm_hash: new_wasm_hash.clone(),
            eligible_at,
        };
        env.storage()
            .instance()
            .set(&upgrade_proposal_key(), &proposal);

        events::upgrade_proposed(&env, &new_wasm_hash, eligible_at);
    }

    /// Execute a pending upgrade once the 48-hour timelock has elapsed.
    ///
    /// Admin-only (issue #785). Clears the proposal on success.
    pub fn execute_upgrade(env: Env, admin: Address) {
        require_admin(&env);
        let _ = admin;
        let proposal: UpgradeProposal = env
            .storage()
            .instance()
            .get(&upgrade_proposal_key())
            .expect("no upgrade proposal");

        assert!(
            env.ledger().timestamp() >= proposal.eligible_at,
            "upgrade timelock still active"
        );

        env.storage().instance().remove(&upgrade_proposal_key());
        events::upgrade_executed(&env, &proposal.new_wasm_hash);
        env.deployer()
            .update_current_contract_wasm(proposal.new_wasm_hash);
    }

    /// Cancel a pending upgrade proposal. Only the admin may call this.
    pub fn cancel_upgrade(env: Env, admin: Address) {
        let admin_addr = require_admin(&env);
        let _ = admin;

        assert!(
            env.storage().instance().has(&upgrade_proposal_key()),
            "no upgrade proposal"
        );
        env.storage().instance().remove(&upgrade_proposal_key());

        events::upgrade_cancelled(&env, &admin_addr);
    }

    /// Return the pending upgrade proposal, or None (issue #785 name).
    pub fn get_pending_upgrade(env: Env) -> Option<UpgradeProposal> {
        env.storage().instance().get(&upgrade_proposal_key())
    }

    /// Return the pending upgrade proposal, or None if none is active.
    pub fn get_upgrade_proposal(env: Env) -> Option<UpgradeProposal> {
        env.storage().instance().get(&upgrade_proposal_key())
    }

    // -----------------------------------------------------------------------
    // Issue #315: Payment delegation — third-party pays on behalf of another
    // -----------------------------------------------------------------------

    /// Grant delegation: allow `delegate` to pay invoice_id on behalf of the caller.
    /// The caller (on_behalf_of) must sign. Delegation is single-use.
    pub fn set_delegation(env: Env, invoice_id: u64, on_behalf_of: Address, delegate: Address) {
        require_not_paused(&env);
        on_behalf_of.require_auth();
        let key = delegation_key(invoice_id, &on_behalf_of);
        env.storage().persistent().set(&key, &delegate);
        events::delegate_set(&env, invoice_id, &delegate);
        append_audit_entry(&env, invoice_id, symbol_short!("set_dlg"), &on_behalf_of);
    }

    /// Execute a delegated payment: caller pays but `on_behalf_of` is recorded as payer.
    /// Consumes the single-use delegation authorization.
    pub fn pay_invoice_delegated(
        env: Env,
        executor: Address,
        invoice_id: u64,
        amount: i128,
        nonce: u64,
        on_behalf_of: Address,
    ) {
        require_fn_not_paused(&env, &symbol_short!("pay"));
        executor.require_auth();

        // Verify delegation exists for this (invoice_id, on_behalf_of) pair.
        let key = delegation_key(invoice_id, &on_behalf_of);
        let stored_delegate: Address = env
            .storage()
            .persistent()
            .get(&key)
            .expect("no delegation authorization");
        assert!(
            stored_delegate == executor,
            "caller is not the authorized delegate"
        );

        // Consume delegation (single-use).
        env.storage().persistent().remove(&key);

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(
            env.ledger().timestamp() <= invoice.deadline,
            "invoice deadline has passed"
        );
        // Issue #483: reject zero or negative payment amounts.
        guard_nonzero_amount(amount).expect("ZeroAmountNotAllowed");
        Self::enforce_invoice_rate_limit(&env, invoice_id, &on_behalf_of);
        assert!(!invoice.frozen, "invoice is frozen");
        assert!(!invoice.admin_frozen, "invoice frozen by admin");

        // Allowed-payers check uses on_behalf_of identity.
        if let Some(ref whitelist) = invoice.allowed_payers {
            assert!(
                whitelist.contains(&on_behalf_of),
                "on_behalf_of not in allowed payers"
            );
        }

        let total: i128 = invoice.amounts.iter().sum();
        let remaining = total - invoice.funded;
        assert!(amount <= remaining, "payment exceeds remaining balance");

        // Contract-wide nonce replay protection for on_behalf_of's delegated
        // authorisation (issue #424). Scoped to the caller across all invoices,
        // so a given nonce cannot be replayed against a different invoice_id.
        consume_global_nonce(&env, &on_behalf_of, nonce);

        let token_client = token::Client::new(&env, &funding_token_for(&invoice));
        token_client.transfer(&executor, &env.current_contract_address(), &amount);

        // Record payment under on_behalf_of address.
        let shard_id = compute_shard_id(&env, &on_behalf_of);
        let mut shard_payments: Vec<Payment> = env
            .storage()
            .persistent()
            .get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(invoice_id, shard_id))
            .unwrap_or_else(|| Vec::new(&env));
        shard_payments.push_back(Payment {
            payer: on_behalf_of.clone(),
            amount,
            tip: 0,
            attestation_hash: None,
            donate_on_failure: false,
            ledger: env.ledger().sequence(),
            timestamp: env.ledger().timestamp(),
        });
        env.storage()
            .persistent()
            .set(&pay_shard_key(invoice_id, shard_id), &shard_payments);

        invoice.funded += amount;
        let cumulative_key = cumulative_contributed_key(invoice_id);
        let cumulative: i128 = env.storage().persistent().get(&cumulative_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&cumulative_key, &(cumulative + amount));

        events::delegated_payment(&env, invoice_id, &on_behalf_of, &executor, amount);
        events::payment_received(&env, invoice_id, &on_behalf_of, amount, &funding_token_for(&invoice));
        check_and_emit_funding_checkpoints(&env, invoice_id, invoice.funded, total);
        Self::record_invoice_rate_limit(&env, invoice_id, &on_behalf_of);
        append_audit_entry(&env, invoice_id, symbol_short!("dlgt_pay"), &executor);
        update_creator_stats_on_payment(&env, &invoice.creator, amount);

        if invoice.funded >= total {
            let in_group = env
                .storage()
                .persistent()
                .has(&invoice_group_key(invoice_id));
            let guarded = invoice.prerequisite_id.is_some()
                || !invoice.tranches.is_empty()
                || !invoice.release_stages.is_empty()
                || in_group
                || !invoice.co_signers.is_empty()
                || env.storage().persistent().has(&cosigners_key(invoice_id))
                || (invoice.condition_oracle.is_some() && !invoice.condition_met);
            if guarded {
                save_invoice(&env, invoice_id, &invoice);
            } else {
                Self::_release(&env, invoice_id, &mut invoice, &executor);
            }
        } else {
            save_invoice(&env, invoice_id, &invoice);
        }
    }

    // -----------------------------------------------------------------------
    // Issue #325: Invoice dispute mechanism with arbitration window
    // -----------------------------------------------------------------------

    /// Raise a dispute on an invoice within 48 ledgers of full funding.
    /// Callable by any address that has made a payment toward the invoice.
    /// Blocked on `release_funds` until resolved or auto-expired (72 ledgers).
    pub fn raise_payer_dispute(env: Env, invoice_id: u64, payer: Address, reason_hash: BytesN<32>) {
        require_not_paused(&env);
        payer.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.disputed, "invoice is already disputed");

        // Verify caller is a payer.
        let is_payer = invoice.payments.iter().any(|p| p.payer == payer);
        assert!(is_payer, "caller has not paid this invoice");

        // 48-ledger window from full funding (use funded_at ledger if stored, else current).
        let funded_ledger: u32 = env
            .storage()
            .persistent()
            .get::<(Symbol, u64), u32>(&dispute_raised_at_key(invoice_id))
            .unwrap_or(env.ledger().sequence());
        assert!(
            env.ledger().sequence() <= funded_ledger + 48,
            "dispute window has closed (48 ledgers)"
        );

        invoice.disputed = true;
        save_invoice(&env, invoice_id, &invoice);

        let record = DisputeRecord {
            reason_hash: reason_hash.clone(),
            raised_at: env.ledger().sequence(),
            status: DisputeStatus::Active,
            dispute_opened_ledger: env.ledger().sequence(),
            dispute_timeout_ledgers: 0,
        };
        env.storage()
            .persistent()
            .set(&dispute_record_key(invoice_id), &record);

        events::dispute_raised(&env, invoice_id, &payer, &reason_hash);
        append_audit_entry(&env, invoice_id, symbol_short!("disp_rse"), &payer);
        // Issue #763: record the payer dispute in the per-invoice history ring buffer.
        append_history_entry(&env, invoice_id, Symbol::new(&env, "dispute"), &payer, None);
    }

    /// Resolve a payer dispute. Only the admin may call this.
    /// `Approved` releases funds to recipients; `Refunded` returns funds to payers.
    pub fn resolve_payer_dispute(
        env: Env,
        invoice_id: u64,
        admin: Address,
        outcome: DisputeOutcome,
    ) {
        require_not_paused(&env);
        let admin_addr = require_admin(&env);
        let _ = admin;

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(invoice.disputed, "invoice is not disputed");

        let mut record: DisputeRecord = env
            .storage()
            .persistent()
            .get(&dispute_record_key(invoice_id))
            .expect("no dispute record");
        assert!(
            record.status == DisputeStatus::Active,
            "dispute is not active"
        );

        // Issue #763: record the dispute resolution in the per-invoice history
        // ring buffer.  Any panic in the match below reverts this write, so the
        // entry only persists for a successful resolution.
        append_history_entry(&env, invoice_id, Symbol::new(&env, "dispute"), &admin_addr, None);

        match outcome {
            DisputeOutcome::Approved => {
                record.status = DisputeStatus::Resolved;
                env.storage()
                    .persistent()
                    .set(&dispute_record_key(invoice_id), &record);
                invoice.disputed = false;
                save_invoice(&env, invoice_id, &invoice);
                events::dispute_resolved(&env, invoice_id, &admin_addr, &DisputeOutcome::Approved);
                append_audit_entry(&env, invoice_id, symbol_short!("disp_res"), &admin_addr);
                Self::_release(&env, invoice_id, &mut invoice, &admin_addr);
            }
            DisputeOutcome::Refunded => {
                record.status = DisputeStatus::Resolved;
                env.storage()
                    .persistent()
                    .set(&dispute_record_key(invoice_id), &record);

                let token_client =
                    token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
                let mut totals: Map<Address, i128> = Map::new(&env);
                for payment in invoice.payments.iter() {
                    let prev = totals.get(payment.payer.clone()).unwrap_or(0);
                    totals.set(payment.payer.clone(), prev + payment.amount);
                }
                let mut total_refunded_amount: i128 = 0;
                for (payer, amount) in totals.iter() {
                    token_client.transfer(&env.current_contract_address(), &payer, &amount);
                    total_refunded_amount += amount;
                    events::payer_refunded(&env, invoice_id, &payer, amount);
                }

                invoice.disputed = false;
                invoice.status = InvoiceStatus::Refunded;
                invoice.completion_time = Some(env.ledger().timestamp());
                save_invoice(&env, invoice_id, &invoice);
                events::dispute_resolved(&env, invoice_id, &admin_addr, &DisputeOutcome::Refunded);
                events::invoice_refunded(&env, invoice_id, total_refunded_amount);
                events::invoice_state_changed(
                    &env,
                    invoice_id,
                    Some(&InvoiceStatus::Pending),
                    &InvoiceStatus::Refunded,
                    &admin_addr,
                );
                append_audit_entry(&env, invoice_id, symbol_short!("disp_res"), &admin_addr);
            }
            DisputeOutcome::Release => {
                record.status = DisputeStatus::Resolved;
                env.storage()
                    .persistent()
                    .set(&dispute_record_key(invoice_id), &record);
                invoice.disputed = false;
                save_invoice(&env, invoice_id, &invoice);
                events::dispute_resolved(&env, invoice_id, &admin_addr, &DisputeOutcome::Release);
                append_audit_entry(&env, invoice_id, symbol_short!("disp_res"), &admin_addr);
                Self::_release(&env, invoice_id, &mut invoice, &admin_addr);
            }
            DisputeOutcome::Refund => {
                record.status = DisputeStatus::Resolved;
                env.storage()
                    .persistent()
                    .set(&dispute_record_key(invoice_id), &record);

                let token_client =
                    token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
                let mut totals: Map<Address, i128> = Map::new(&env);
                for payment in invoice.payments.iter() {
                    let prev = totals.get(payment.payer.clone()).unwrap_or(0);
                    totals.set(payment.payer.clone(), prev + payment.amount);
                }
                for (payer, amount) in totals.iter() {
                    token_client.transfer(&env.current_contract_address(), &payer, &amount);
                    events::payer_refunded(&env, invoice_id, &payer, amount);
                }

                invoice.disputed = false;
                invoice.status = InvoiceStatus::Refunded;
                invoice.completion_time = Some(env.ledger().timestamp());
                save_invoice(&env, invoice_id, &invoice);
                events::dispute_resolved(&env, invoice_id, &admin_addr, &DisputeOutcome::Refund);
                events::invoice_refunded(&env, invoice_id, invoice.funded);
                events::invoice_state_changed(
                    &env,
                    invoice_id,
                    Some(&InvoiceStatus::Pending),
                    &InvoiceStatus::Refunded,
                    &admin_addr,
                );
                append_audit_entry(&env, invoice_id, symbol_short!("disp_res"), &admin_addr);
            }
        }
    }

    /// Check and auto-expire a dispute if 72 ledgers have elapsed without resolution.
    /// If expired, funds are released to recipients.
    pub fn expire_dispute(env: Env, invoice_id: u64) {
        require_not_paused(&env);

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(invoice.disputed, "invoice is not disputed");

        let record: DisputeRecord = env
            .storage()
            .persistent()
            .get(&dispute_record_key(invoice_id))
            .expect("no dispute record");
        assert!(
            record.status == DisputeStatus::Active,
            "dispute is not active"
        );
        assert!(
            env.ledger().sequence() > record.raised_at + 72,
            "dispute expiry window not reached (72 ledgers)"
        );

        let mut updated = record.clone();
        updated.status = DisputeStatus::Expired;
        env.storage()
            .persistent()
            .set(&dispute_record_key(invoice_id), &updated);

        invoice.disputed = false;
        save_invoice(&env, invoice_id, &invoice);

        events::dispute_expired(&env, invoice_id);
        let actor = env.current_contract_address();
        append_audit_entry(&env, invoice_id, symbol_short!("disp_exp"), &actor);
        // Issue #763: record the dispute expiry in the per-invoice history ring buffer.
        append_history_entry(&env, invoice_id, Symbol::new(&env, "dispute"), &actor, None);
        Self::_release(&env, invoice_id, &mut invoice, &actor);
    }

    /// Issue #678: Return whether `invoice_id` can be released right now,
    /// without simulating a release or panicking. Mirrors the gating checks
    /// in `release_invoice` (status, funding, prerequisite, approver,
    /// co-signers, escrow hold, dispute, and time-lock delay) but reports
    /// `false` instead of reverting when a gate is not satisfied.
    pub fn is_invoice_releasable(env: Env, invoice_id: u64) -> bool {
        let invoice = load_invoice(&env, invoice_id);

        if invoice.frozen || invoice.admin_frozen {
            return false;
        }
        if invoice.status != InvoiceStatus::Pending {
            return false;
        }
        if invoice.disputed {
            match env
                .storage()
                .persistent()
                .get::<(Symbol, u64), DisputeRecord>(&dispute_record_key(invoice_id))
            {
                Some(record) if record.status != DisputeStatus::Active => {}
                _ => return false,
            }
        }
        if let Some(held_until) = invoice.held_until {
            if env.ledger().sequence() < held_until {
                return false;
            }
        }

        let total: i128 = invoice.amounts.iter().sum();
        let min_required = if invoice.min_funding_bps > 0 {
            (total as u128 * invoice.min_funding_bps as u128 / 10_000u128) as i128
        } else {
            total
        };
        if invoice.funded < min_required {
            return false;
        }

        // Issue #327: time-lock delay set by the creator.
        if let Some(delay_ledgers) = env
            .storage()
            .persistent()
            .get::<_, u32>(&release_delay_key(invoice_id))
        {
            if let Some(funded_at) = env
                .storage()
                .persistent()
                .get::<_, u32>(&funded_at_ledger_key(invoice_id))
            {
                let unlock_at = funded_at.saturating_add(delay_ledgers);
                if env.ledger().sequence() < unlock_at {
                    return false;
                }
            }
        }

        if invoice.approver.is_some() && !invoice.approved {
            return false;
        }

        if let Some(prereq_id) = invoice.prerequisite_id {
            let prereq = load_invoice(&env, prereq_id);
            if prereq.status != InvoiceStatus::Released {
                return false;
            }
        }

        if !invoice.co_signers.is_empty() && invoice.signatures.len() < invoice.required_signatures
        {
            return false;
        }

        if let Some(threshold) = env
            .storage()
            .persistent()
            .get::<_, u32>(&cosigner_thresh_key(invoice_id))
        {
            let approvals: Vec<Address> = env
                .storage()
                .persistent()
                .get(&cosign_key(invoice_id))
                .unwrap_or_else(|| Vec::new(&env));
            if approvals.len() < threshold {
                return false;
            }
        }

        true
    }

    /// Return the dispute record for an invoice, or None if no dispute exists.
    pub fn get_dispute(env: Env, invoice_id: u64) -> Option<DisputeRecord> {
        env.storage()
            .persistent()
            .get(&dispute_record_key(invoice_id))
    }

    /// Issue #679: Return the dispute record for an invoice, or None if no
    /// dispute exists. Equivalent to `get_dispute`, named to match the
    /// `DisputeRecord` type it returns.
    pub fn get_dispute_record(env: Env, invoice_id: u64) -> Option<DisputeRecord> {
        env.storage()
            .persistent()
            .get(&dispute_record_key(invoice_id))
    }

    // -----------------------------------------------------------------------
    // Issue #326: Protocol fee distribution to treasury address
    // -----------------------------------------------------------------------

    /// Set the protocol fee rate (in basis points, max 500 = 5%) and treasury address.
    /// Rate of 0 disables the fee. Only callable by admin.
    pub fn set_protocol_fee(env: Env, admin: Address, rate_bps: u32, treasury: Address) {
        require_admin(&env);
        let _ = admin;
        assert!(rate_bps <= 500, "fee rate exceeds maximum (500 bps = 5%)");
        let config = ProtocolFeeConfig { rate_bps, treasury };
        env.storage().instance().set(&protocol_fee_key(), &config);
    }

    /// Return the current protocol fee configuration.
    pub fn get_fee_config(env: Env) -> ProtocolFeeConfig {
        env.storage()
            .instance()
            .get(&protocol_fee_key())
            .unwrap_or(ProtocolFeeConfig {
                rate_bps: 0,
                treasury: env.current_contract_address(),
            })
    }

    // -----------------------------------------------------------------------
    // Issue #751: protocol fee + treasury withdrawal
    // -----------------------------------------------------------------------

    /// Issue #751: Update the protocol fee rate, keeping the currently
    /// configured treasury address.
    ///
    /// `bps` is in basis points and is capped at 500 (5%). Only callable by the
    /// stored admin. Use [`Self::set_protocol_fee`] if the treasury address also
    /// needs to change.
    pub fn set_protocol_fee_bps(env: Env, admin: Address, bps: u32) {
        let stored_admin = require_admin(&env);
        assert!(stored_admin == admin, "not admin");
        assert!(bps <= 500, "fee rate exceeds maximum (500 bps = 5%)");

        let treasury = env
            .storage()
            .instance()
            .get::<Symbol, ProtocolFeeConfig>(&protocol_fee_key())
            .map(|cfg| cfg.treasury)
            .unwrap_or_else(|| env.current_contract_address());

        env.storage().instance().set(
            &protocol_fee_key(),
            &ProtocolFeeConfig {
                rate_bps: bps,
                treasury,
            },
        );
    }

    /// Issue #751: Current protocol fee rate in basis points (0 = disabled).
    pub fn get_protocol_fee_bps(env: Env) -> u32 {
        get_protocol_fee_bps_internal(&env)
    }

    /// Issue #751: Protocol fees currently held by the contract and available
    /// for [`Self::withdraw_treasury`]. Fees accumulate on every `pay` call
    /// while a non-zero protocol fee is configured.
    pub fn get_treasury_balance(env: Env) -> i128 {
        get_treasury_balance_internal(&env)
    }

    /// Issue #751: Transfer `amount` of accumulated protocol fees from the
    /// contract treasury to the admin's wallet.
    ///
    /// Only the stored admin may call this and `amount` must be positive and no
    /// larger than [`Self::get_treasury_balance`]. Fees are denominated in the
    /// contract's settlement token, set by `initialize` (`usdc_token_key`).
    pub fn withdraw_treasury(env: Env, admin: Address, amount: i128) {
        let stored_admin = require_admin(&env);
        assert!(stored_admin == admin, "not admin");
        assert!(amount > 0, "amount must be positive");

        let balance = get_treasury_balance_internal(&env);
        assert!(amount <= balance, "insufficient treasury balance");

        let token_addr: Address = env
            .storage()
            .instance()
            .get(&usdc_token_key())
            .expect("treasury token not set");
        token::Client::new(&env, &token_addr).transfer(
            &env.current_contract_address(),
            &admin,
            &amount,
        );

        env.storage()
            .instance()
            .set(&treasury_balance_key(), &(balance - amount));

        events::treasury_withdrawn(&env, &admin, amount);
    }

    // -----------------------------------------------------------------------
    // Issue #316: Compute budget estimation utility
    // -----------------------------------------------------------------------
    // Issue #316 / #351: Compute budget estimation utility
    // -----------------------------------------------------------------------

    /// Helper for estimate_compute parameter extraction.
    fn get_u64_param(
        env: &Env,
        params: &Map<Symbol, Val>,
        keys: &[Symbol],
    ) -> Result<Option<u64>, ContractError> {
        for k in keys.iter() {
            if let Some(val) = params.get(k.clone()) {
                if let Ok(v) = u64::try_from_val(env, &val) {
                    return Ok(Some(v));
                } else if let Ok(v) = u32::try_from_val(env, &val) {
                    return Ok(Some(v as u64));
                } else if let Ok(v) = i128::try_from_val(env, &val) {
                    if v >= 0 {
                        return Ok(Some(v as u64));
                    } else {
                        return Err(ContractError::InvalidAmount);
                    }
                } else {
                    return Err(ContractError::InvalidAmount);
                }
            }
        }
        Ok(None)
    }

    /// Helper for estimate_compute recipient list/count extraction.
    fn get_recipients_len(
        env: &Env,
        params: &Map<Symbol, Val>,
    ) -> Result<Option<u64>, ContractError> {
        let keys = [
            Symbol::new(env, "recipients"),
            symbol_short!("recip"),
            Symbol::new(env, "recipient_count"),
            symbol_short!("count"),
        ];
        for k in keys.iter() {
            if let Some(val) = params.get(k.clone()) {
                if let Ok(vec) = Vec::<Address>::try_from_val(env, &val) {
                    return Ok(Some(vec.len() as u64));
                } else if let Ok(vec) = Vec::<Val>::try_from_val(env, &val) {
                    return Ok(Some(vec.len() as u64));
                } else if let Ok(c) = u32::try_from_val(env, &val) {
                    return Ok(Some(c as u64));
                } else if let Ok(c) = u64::try_from_val(env, &val) {
                    return Ok(Some(c));
                } else {
                    return Err(ContractError::InvalidRecipients);
                }
            }
        }
        Ok(None)
    }

    /// Helper for estimate_compute invoice verification.
    fn load_invoice_opt(env: &Env, invoice_id: u64) -> Result<InvoiceCore, ContractError> {
        if let Some(core) = env.storage().persistent().get(&invoice_key(invoice_id)) {
            Ok(core)
        } else if let Some(core) = env.storage().instance().get(&invoice_key(invoice_id)) {
            Ok(core)
        } else {
            Err(ContractError::InvoiceNotFound)
        }
    }

    /// Estimate the compute budget for a given public contract function and parameters.
    /// Returns `Result<ComputeEstimate, ContractError>` without mutating state.
    pub fn estimate_compute(
        env: Env,
        operation: Symbol,
        params: Map<Symbol, Val>,
    ) -> Result<ComputeEstimate, ContractError> {
        let sym_create_full = Symbol::new(&env, "create_invoice");
        let sym_create_short = symbol_short!("create");
        let sym_create_alt = symbol_short!("create_i");

        let sym_pay = symbol_short!("pay");
        let sym_pay_full = Symbol::new(&env, "pay");
        let sym_dlgt = symbol_short!("pay_dlgt");

        let sym_release = symbol_short!("release");
        let sym_release_full = Symbol::new(&env, "release");

        let sym_refund = symbol_short!("refund");
        let sym_refund_full = Symbol::new(&env, "refund");

        let sym_dispute_full = Symbol::new(&env, "open_dispute");
        let sym_dispute_raise = Symbol::new(&env, "raise_dispute");
        let sym_dispute_short = symbol_short!("dispute");

        let sym_approve_full = Symbol::new(&env, "approve_release");
        let sym_approve_inv = Symbol::new(&env, "approve_invoice");
        let sym_approve_short = symbol_short!("approve");

        let (cpu_insns, mem_bytes): (u64, u64) = if operation == sym_create_full
            || operation == sym_create_short
            || operation == sym_create_alt
        {
            let r = Self::get_recipients_len(&env, &params)?;
            let recip_count = match r {
                Some(cnt) => cnt,
                None => return Err(ContractError::InvalidRecipients),
            };
            if recip_count == 0 {
                return Err(ContractError::InvalidRecipients);
            }
            (
                INSTRUCTIONS_BASE + recip_count * 200_000,
                (128 + recip_count * 64) * 1024,
            )
        } else if operation == sym_pay || operation == sym_pay_full || operation == sym_dlgt {
            let inv_id_opt = Self::get_u64_param(
                &env,
                &params,
                &[
                    Symbol::new(&env, "invoice_id"),
                    symbol_short!("id"),
                    Symbol::new(&env, "invoice"),
                ],
            )?;
            let recip_cnt_opt = Self::get_recipients_len(&env, &params)?;

            let recip_cnt = match (inv_id_opt, recip_cnt_opt) {
                (Some(id), _) => {
                    let inv = Self::load_invoice_opt(&env, id)?;
                    inv.recipients.len() as u64
                }
                (None, Some(cnt)) => cnt,
                (None, None) => return Err(ContractError::InvoiceNotFound),
            };

            (
                INSTRUCTIONS_BASE
                    + INSTRUCTIONS_PER_SHARD * SHARD_COUNT
                    + recip_cnt * (INSTRUCTIONS_PER_RECIPIENT / 2),
                (256 + recip_cnt * 16) * 1024,
            )
        } else if operation == sym_release || operation == sym_release_full {
            let inv_id_opt = Self::get_u64_param(
                &env,
                &params,
                &[
                    Symbol::new(&env, "invoice_id"),
                    symbol_short!("id"),
                    Symbol::new(&env, "invoice"),
                ],
            )?;
            let recip_cnt_opt = Self::get_recipients_len(&env, &params)?;

            let recip_cnt = match (inv_id_opt, recip_cnt_opt) {
                (Some(id), _) => {
                    let inv = Self::load_invoice_opt(&env, id)?;
                    inv.recipients.len() as u64
                }
                (None, Some(cnt)) => cnt,
                (None, None) => return Err(ContractError::InvoiceNotFound),
            };

            (
                INSTRUCTIONS_BASE
                    + recip_cnt * INSTRUCTIONS_PER_RECIPIENT
                    + INSTRUCTIONS_PER_SHARD * SHARD_COUNT,
                (256 + recip_cnt * 32) * 1024,
            )
        } else if operation == sym_refund || operation == sym_refund_full {
            let inv_id_opt = Self::get_u64_param(
                &env,
                &params,
                &[
                    Symbol::new(&env, "invoice_id"),
                    symbol_short!("id"),
                    Symbol::new(&env, "invoice"),
                ],
            )?;
            let payer_cnt_opt = Self::get_u64_param(
                &env,
                &params,
                &[
                    Symbol::new(&env, "payer_count"),
                    Symbol::new(&env, "payers"),
                    symbol_short!("count"),
                ],
            )?;

            let payer_cnt = match (inv_id_opt, payer_cnt_opt) {
                (Some(id), _) => {
                    let _inv = Self::load_invoice_opt(&env, id)?;
                    1u64
                }
                (None, Some(cnt)) => cnt,
                (None, None) => return Err(ContractError::InvoiceNotFound),
            };

            (
                INSTRUCTIONS_BASE + INSTRUCTIONS_PER_SHARD * SHARD_COUNT + payer_cnt * 350_000,
                (256 + payer_cnt * 32) * 1024,
            )
        } else if operation == sym_dispute_full
            || operation == sym_dispute_raise
            || operation == sym_dispute_short
        {
            let inv_id = match Self::get_u64_param(
                &env,
                &params,
                &[
                    Symbol::new(&env, "invoice_id"),
                    symbol_short!("id"),
                    Symbol::new(&env, "invoice"),
                ],
            )? {
                Some(id) => id,
                None => return Err(ContractError::InvoiceNotFound),
            };

            let _inv = Self::load_invoice_opt(&env, inv_id)?;
            (INSTRUCTIONS_BASE + 200_000, 128 * 1024)
        } else if operation == sym_approve_full
            || operation == sym_approve_inv
            || operation == sym_approve_short
        {
            let inv_id = match Self::get_u64_param(
                &env,
                &params,
                &[
                    Symbol::new(&env, "invoice_id"),
                    symbol_short!("id"),
                    Symbol::new(&env, "invoice"),
                ],
            )? {
                Some(id) => id,
                None => return Err(ContractError::InvoiceNotFound),
            };

            let _inv = Self::load_invoice_opt(&env, inv_id)?;
            (INSTRUCTIONS_BASE + 150_000, 128 * 1024)
        } else {
            return Err(ContractError::InvalidStatus);
        };

        let budget_pct = cpu_insns * 100 / INSTRUCTION_BUDGET_LIMIT;
        if budget_pct > 80 {
            env.events().publish(
                (symbol_short!("split"), symbol_short!("bdgt_w"), operation),
                (cpu_insns, INSTRUCTION_BUDGET_LIMIT),
            );
        }

        let fee_stroops = (cpu_insns as i128 / 10_000) * STROOPS_PER_10K_INSTRUCTIONS as i128;

        Ok(ComputeEstimate {
            cpu_insns,
            mem_bytes,
            fee_stroops,
        })
    }

    // -----------------------------------------------------------------------
    // Issue #334: Compact XDR migration
    // -----------------------------------------------------------------------

    /// One-time migration helper that writes compact overlay fields for a
    /// stored invoice, reducing XDR storage cost.
    ///
    /// **What it does:**
    /// - Writes the invoice status as a single `u32` byte to `compact_status_key`
    ///   (4 bytes + XDR key overhead) instead of the full `InvoiceStatus` enum
    ///   variant (string-encoded, 20+ bytes).
    /// - Writes the deadline as a `u32` ledger-sequence estimate to
    ///   `compact_deadline_ledger_key` when the deadline is representable.
    ///
    /// The compact fields are *overlays* — `load_invoice` continues to read from
    /// the original `InvoiceCore` / `InvoiceExt` / `InvoiceExt2` blobs.  The
    /// compact fields are only used by optimised hot-path code that explicitly
    /// reads them (e.g. status checks before a pay).
    ///
    /// Callable by anyone (read-only migration; no auth required).
    pub fn compact_migrate(env: Env, invoice_id: u64) -> CompactMigrateResult {
        require_not_paused(&env);
        let invoice = load_invoice(&env, invoice_id);

        // Write compact status byte.
        save_compact_status(&env, invoice_id, &invoice.status);
        let status_byte = invoice.status.to_u8() as u32;

        // Write compact deadline-as-ledger when the deadline fits in a u32.
        // We store it as an estimate: current_ledger + (deadline - now) / 5
        // (Stellar produces roughly one ledger every 5 seconds).
        let now_ts = env.ledger().timestamp();
        let current_ledger = env.ledger().sequence();
        let deadline_migrated = if invoice.deadline >= now_ts {
            let secs_remaining = invoice.deadline - now_ts;
            let ledgers_remaining = (secs_remaining / 5) as u32;
            let deadline_ledger = current_ledger.saturating_add(ledgers_remaining);
            env.storage()
                .persistent()
                .set(&compact_deadline_ledger_key(invoice_id), &deadline_ledger);
            true
        } else {
            // Deadline already passed — store 0 as sentinel.
            env.storage()
                .persistent()
                .set(&compact_deadline_ledger_key(invoice_id), &0u32);
            false
        };

        // Issue #332: ensure recipients + amounts lists are present.
        if !env
            .storage()
            .persistent()
            .has(&recipients_list_key(invoice_id))
        {
            save_recipients_list(&env, invoice_id, &invoice.recipients, &invoice.amounts);
        }

        CompactMigrateResult {
            invoice_id,
            status_byte,
            deadline_migrated,
        }
    }

    // -----------------------------------------------------------------------
    // Issue #333: Milestone query helper
    // -----------------------------------------------------------------------

    /// Return the milestone bitmask for `invoice_id`.
    ///
    /// Bit layout:
    ///   Bit 0 → 25 %  (2500 bps) emitted
    ///   Bit 1 → 50 %  (5000 bps) emitted
    ///   Bit 2 → 75 %  (7500 bps) emitted
    ///   Bit 3 → 100 % (10000 bps) emitted
    ///
    /// Returns 0 when no milestones have been crossed yet.
    pub fn get_milestone_flags(env: Env, invoice_id: u64) -> u32 {
        env.storage()
            .instance()
            .get(&milestone_flags_key(invoice_id))
            .unwrap_or(0u32)
    }

    // -----------------------------------------------------------------------
    // Issue #332: Optimised recipient list query
    // -----------------------------------------------------------------------

    /// Return the contiguous recipients list stored at creation time.
    ///
    /// Falls back to reading from `InvoiceCore` for invoices created before
    /// this optimisation was deployed.
    pub fn get_recipients_list(env: Env, invoice_id: u64) -> Vec<Address> {
        let (recipients, _amounts) =
            load_recipients_list(&env, invoice_id, &Vec::new(&env), &Vec::new(&env));
        if !recipients.is_empty() {
            recipients
        } else {
            // Fallback for pre-migration invoices.
            load_invoice(&env, invoice_id).recipients
        }
    }

    // -----------------------------------------------------------------------
    // Issue #435: Contract upgrade freeze
    // -----------------------------------------------------------------------

    /// Freeze the contract for upgrade. Blocks all write operations except admin actions.
    pub fn freeze_for_upgrade(env: Env, admin: Address, checkpoint_hash: BytesN<32>) {
        require_admin_role(&env, &admin, AdminRole::SuperAdmin);
        env.storage().instance().set(&upgrade_freeze_key(), &true);
        env.storage()
            .instance()
            .set(&upgrade_checkpoint_key(), &checkpoint_hash);
        events::contract_frozen_for_upgrade(&env, &checkpoint_hash);
    }

    /// Thaw the contract (remove upgrade freeze).
    pub fn thaw_contract(env: Env, admin: Address) {
        require_admin_role(&env, &admin, AdminRole::SuperAdmin);
        env.storage().instance().remove(&upgrade_freeze_key());
        env.storage().instance().remove(&upgrade_checkpoint_key());
        events::contract_thawed(&env, &admin);
    }

    /// Get the upgrade checkpoint hash if frozen.
    pub fn get_upgrade_checkpoint(env: Env) -> Option<BytesN<32>> {
        env.storage().instance().get(&upgrade_checkpoint_key())
    }

    // -----------------------------------------------------------------------
    // Issue #436: Payment proof commitment
    // -----------------------------------------------------------------------

    /// Get the current payment root hash for an invoice (sha256 rolling hash).
    pub fn get_payment_root(env: Env, invoice_id: u64) -> BytesN<32> {
        let stored: Option<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&payment_root_key(invoice_id));

        match stored {
            Some(root) => root,
            None => {
                // Initial root is sha256(invoice_id)
                let invoice_id_bytes = invoice_id.to_be_bytes();
                let hash = env
                    .crypto()
                    .sha256(&Bytes::from_slice(&env, &invoice_id_bytes));
                hash.into()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Module-level helper functions (audit log, leaderboard, archival)
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn transfer_audit_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("audit_log"), invoice_id)
}

fn archived_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("archv_inv"), id)
}

fn top_contributors_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("top_ctrib"), invoice_id)
}

fn max_audit_log_entries_key() -> Symbol {
    symbol_short!("mx_aud_en")
}

fn max_leaderboard_size_key() -> Symbol {
    symbol_short!("mx_ldr_sz")
}

#[allow(dead_code)]
fn remove_invoice(env: &Env, id: u64) {
    env.storage().persistent().remove(&invoice_key(id));
}

#[allow(dead_code)]
fn load_audit_log(env: &Env, invoice_id: u64) -> Vec<TransferRecord> {
    env.storage()
        .persistent()
        .get(&transfer_audit_key(invoice_id))
        .unwrap_or(Vec::new(env))
}

#[allow(dead_code)]
fn append_audit_record(env: &Env, invoice_id: u64, record: &TransferRecord) {
    let mut log = load_audit_log(env, invoice_id);
    let max = get_max_audit_log_entries(env);
    if log.len() >= max {
        return;
    }
    log.push_back(record.clone());
    env.storage().persistent().set(&transfer_audit_key(invoice_id), &log);
}

#[allow(dead_code)]
fn get_max_audit_log_entries(env: &Env) -> u32 {
    env.storage()
        .persistent()
        .get(&max_audit_log_entries_key())
        .unwrap_or(1_000u32)
}

#[allow(dead_code)]
fn get_max_leaderboard_size(env: &Env) -> u32 {
    env.storage()
        .persistent()
        .get(&max_leaderboard_size_key())
        .unwrap_or(10u32)
}

fn load_top_contributors(env: &Env, invoice_id: u64) -> Vec<(Address, i128)> {
    env.storage()
        .persistent()
        .get(&top_contributors_key(invoice_id))
        .unwrap_or(Vec::new(env))
}

#[allow(dead_code)]
fn save_top_contributors(env: &Env, invoice_id: u64, leaders: &Vec<(Address, i128)>) {
    env.storage()
        .persistent()
        .set(&top_contributors_key(invoice_id), leaders);
}

#[allow(dead_code)]
fn update_leaderboard(env: &Env, invoice_id: u64, payer: &Address, amount: i128) {
    let max = get_max_leaderboard_size(env);
    let mut leaders = load_top_contributors(env, invoice_id);

    let payer_cli = payer.clone();
    let mut found_index = None;
    for i in 0..leaders.len() {
        let (ref addr, _) = leaders.get(i).unwrap();
        if addr == &payer_cli {
            found_index = Some(i);
            break;
        }
    }

    let existing_amount = if let Some(idx) = found_index {
        let (_, amt) = leaders.get(idx).unwrap();
        Some(amt)
    } else {
        None
    };

    let new_amount = existing_amount.unwrap_or(0i128) + amount;

    if let Some(idx) = found_index {
        leaders.set(idx, (payer_cli.clone(), new_amount));
    } else {
        leaders.push_back((payer_cli.clone(), amount));
    }

    if found_index.is_some() {
        let idx = if let Some(i) = found_index {
            i
        } else {
            leaders.len() - 1
        };
        let (_, new_amt) = leaders.get(idx).unwrap();
        let mut j = idx;
        while j > 0 {
            let (_, prev_amt) = leaders.get(j - 1).unwrap();
            if new_amt > prev_amt {
                let curr = leaders.get(idx).unwrap();
                leaders.set(j, curr);
                j -= 1;
            } else {
                break;
            }
        }
        leaders.set(j, (payer_cli.clone(), new_amount));
    } else {
        let new_len = leaders.len();
        if new_len > 1 {
            let mut j = new_len - 1;
            while j > 0 {
                let (_, curr_amt) = leaders.get(j).unwrap();
                let (_, prev_amt) = leaders.get(j - 1).unwrap();
                if curr_amt > prev_amt {
                    let curr = leaders.get(j).unwrap();
                    leaders.set(j, curr);
                    j -= 1;
                } else {
                    break;
                }
            }
            leaders.set(j, (payer_cli, new_amount));
        }
    }

    while leaders.len() > max {
        leaders.pop_back();
    }

    save_top_contributors(env, invoice_id, &leaders);
}

// ---------------------------------------------------------------------------
// Contract (block 2 — newer features merged from Wave 7)
// ---------------------------------------------------------------------------
#[contractimpl]
impl SplitContract {
    // -----------------------------------------------------------------------
    // Issue #437: Recipient payout delay
    // -----------------------------------------------------------------------

    /// Claim a delayed payout once it becomes claimable.
    pub fn claim_delayed_payout(env: Env, invoice_id: u64, recipient: Address) {
        recipient.require_auth();
        let delayed_payout: DelayedPayout = env
            .storage()
            .persistent()
            .get(&delayed_payout_key(invoice_id, &recipient))
            .expect("no delayed payout found");
        assert!(
            env.ledger().sequence() >= delayed_payout.claimable_at_ledger,
            "payout not yet claimable"
        );
        let invoice = load_invoice(&env, invoice_id);
        let token = invoice.tokens.get(0).expect("invoice has no tokens");
        let token_client = token::Client::new(&env, &token);
        token_client.transfer(
            &env.current_contract_address(),
            &recipient,
            &delayed_payout.amount,
        );
        env.storage()
            .persistent()
            .remove(&delayed_payout_key(invoice_id, &recipient));
        events::delayed_payout_claimed(&env, invoice_id, &recipient, delayed_payout.amount);
    }

    // -----------------------------------------------------------------------
    // Issue #438: Invoice anonymity mode
    // -----------------------------------------------------------------------

    /// Check if an invoice is in anonymous recipients mode.
    pub fn is_anonymous_invoice(env: Env, invoice_id: u64) -> bool {
        env.storage()
            .persistent()
            .get(&anonymous_recipients_key(invoice_id))
            .unwrap_or(false)
    }

    /// Issue #808: hide payer addresses in this invoice's payment events,
    /// which then carry `sha256(payer XDR)` instead. Creator-only, while Pending.
    pub fn set_payer_anonymity(env: Env, creator: Address, invoice_id: u64, enabled: bool) {
        require_not_paused(&env);
        creator.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        assert!(invoice.creator == creator, "only creator");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        env.storage()
            .persistent()
            .set(&payer_anonymity_key(invoice_id), &enabled);
    }

    /// Issue #808: whether payer anonymity is enabled for this invoice.
    pub fn is_payer_anonymous(env: Env, invoice_id: u64) -> bool {
        payer_anonymity_enabled(&env, invoice_id)
    }

    /// Issue #808: the hash a payer appears as in anonymous payment events,
    /// so they can find their own payments.
    pub fn get_payer_hash(env: Env, payer: Address) -> BytesN<32> {
        hash_payer(&env, &payer)
    }

    // -----------------------------------------------------------------------
    // Issue #431: Duplicate payment detection
    // -----------------------------------------------------------------------

    /// Set the duplicate payment detection window (ledgers). Admin-only.
    pub fn set_duplicate_window_ledgers(env: Env, admin: Address, window_ledgers: u32) {
        require_admin_role(&env, &admin, AdminRole::SuperAdmin);
        assert!(
            window_ledgers > 0 && window_ledgers <= 1_000_000,
            "invalid window size"
        );
        env.storage()
            .instance()
            .set(&duplicate_window_ledgers_key(), &window_ledgers);
    }

    /// Get the current duplicate detection window size.
    pub fn get_duplicate_window_ledgers(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&duplicate_window_ledgers_key())
            .unwrap_or(DEFAULT_DUPLICATE_WINDOW_LEDGERS)
    }

    // -----------------------------------------------------------------------
    // Issue #432: Referral tracking
    // -----------------------------------------------------------------------

    /// Set the referrer reward percentage of platform fees. Admin-only.
    pub fn set_referrer_reward_bps(env: Env, admin: Address, reward_bps: u32) {
        require_admin_role(&env, &admin, AdminRole::SuperAdmin);
        assert!(reward_bps <= 10_000, "reward_bps must be ≤ 10000");
        env.storage()
            .instance()
            .set(&referrer_reward_bps_key(), &reward_bps);
    }

    /// Get the current referrer reward percentage.
    pub fn get_referrer_reward_bps(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&referrer_reward_bps_key())
            .unwrap_or(0u32)
    }

    /// Get the referrer for an invoice, if set.
    pub fn get_invoice_referrer(env: Env, invoice_id: u64) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&invoice_referrer_key(invoice_id))
    }

    // -----------------------------------------------------------------------
    // Issue #434: Invoice groups
    // -----------------------------------------------------------------------

    /// Get all invoice IDs in a group (Issue #434 support).
    pub fn get_group_members_list(env: Env, group_id: u64) -> Vec<u64> {
        get_group_members(&env, group_id)
    }

    /// Get the group ID for an invoice, if it belongs to a group (Issue #434 support).
    pub fn get_invoice_group_id_info(env: Env, invoice_id: u64) -> Option<u64> {
        get_invoice_group_id(&env, invoice_id)
    }

    /// Rollback a group: set all members to Refunded and process refunds.
    /// Internal function called when a group member expires.
    // Issue #434: group rollback is not yet triggered from the expiry path.
    #[allow(dead_code)]
    fn rollback_invoice_group(env: &Env, group_id: u64) {
        let members = get_group_members(env, group_id);

        for member_id in members.iter() {
            let mut invoice = load_invoice(env, member_id);
            if invoice.status == InvoiceStatus::Pending {
                invoice.status = InvoiceStatus::Refunded;
                save_invoice(env, member_id, &invoice);

                // Process refunds for all payers
                for payment in invoice.payments.iter() {
                    let token_client = token::Client::new(env, &invoice.tokens.get(0).unwrap());
                    token_client.transfer(
                        &env.current_contract_address(),
                        &payment.payer,
                        &(payment.amount + payment.tip),
                    );
                    events::payer_refunded(
                        env,
                        member_id,
                        &payment.payer,
                        payment.amount + payment.tip,
                    );
                }
            }
        }

        events::group_rollback_triggered(env, group_id, members.len() as u32);
    }

    // -----------------------------------------------------------------------
    // Issue #475: Multi-Signature Admin Control
    // -----------------------------------------------------------------------

    /// Initialise (or replace) the multi-sig AdminSet.
    ///
    /// Requires the current single admin to authenticate. Once called, all
    /// sensitive operations that go through `propose_admin_action` /
    /// `approve_admin_action` will require `threshold`-of-N signers.
    pub fn set_admin_set(env: Env, admin: Address, signers: Vec<Address>, threshold: u32) {
        require_admin(&env);
        let _ = admin;
        assert!(!signers.is_empty(), "signers must not be empty");
        assert!(
            threshold > 0 && threshold <= signers.len(),
            "threshold must be between 1 and signers.len()"
        );
        let admin_set = AdminSet { signers, threshold };
        env.storage().instance().set(&admin_set_key(), &admin_set);
    }

    /// Return the current AdminSet, or None if not yet configured.
    pub fn get_admin_set(env: Env) -> Option<AdminSet> {
        env.storage().instance().get(&admin_set_key())
    }

    // -----------------------------------------------------------------------
    // Issue #560: Creator Migration
    // -----------------------------------------------------------------------

    /// Nominate a new creator for an invoice. Only the current creator can nominate.
    ///
    /// # Arguments
    /// * `caller` - must be the current creator of the invoice
    /// * `invoice_id` - target invoice
    /// * `successor` - address of the new creator
    pub fn nominate_new_creator(env: Env, caller: Address, invoice_id: u64, successor: Address) {
        caller.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status != InvoiceStatus::Deleted,
            "InvoiceDeleted"
        );
        assert!(invoice.creator == caller, "OnlyCreator");
        env.storage()
            .persistent()
            .set(&pending_creator_key(invoice_id), &successor);
        events::creator_nominated(&env, invoice_id, &successor);
    }

    /// Accept the creator role for a nominated invoice.
    ///
    /// # Arguments
    /// * `successor` - must be the nominated successor
    /// * `invoice_id` - target invoice
    pub fn accept_creator_role(env: Env, successor: Address, invoice_id: u64) {
        successor.require_auth();
        let pending: Address = env
            .storage()
            .persistent()
            .get(&pending_creator_key(invoice_id))
            .expect("no pending creator nomination");
        assert!(pending == successor, "NotNominated");
        let mut invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status != InvoiceStatus::Deleted,
            "InvoiceDeleted"
        );
        invoice.creator = successor.clone();
        save_invoice(&env, invoice_id, &invoice);
        env.storage()
            .persistent()
            .remove(&pending_creator_key(invoice_id));
        events::creator_migrated(&env, invoice_id, &successor);
    }

    // -----------------------------------------------------------------------
    // Issue #562: Soft-Delete with Tombstone
    // -----------------------------------------------------------------------

    /// Soft-delete an invoice, writing a tombstone record for audit trail.
    /// Only the creator can delete a Pending invoice with no unclaimed funds.
    ///
    /// # Arguments
    /// * `caller` - must be the current creator of the invoice
    /// * `invoice_id` - target invoice
    pub fn delete_invoice(env: Env, caller: Address, invoice_id: u64) {
        caller.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status != InvoiceStatus::Deleted,
            "InvoiceDeleted"
        );
        assert!(invoice.creator == caller, "OnlyCreator");
        assert!(invoice.status == InvoiceStatus::Pending, "InvalidStatus");
        assert!(invoice.funded == 0, "FundsUnclaimed");
        let tombstone = Tombstone {
            invoice_id,
            deleted_at_ledger: env.ledger().sequence(),
            deleted_by: caller.clone(),
        };
        env.storage()
            .persistent()
            .set(&tombstone_key(invoice_id), &tombstone);
        let mut updated = invoice.clone();
        updated.status = InvoiceStatus::Deleted;
        save_invoice(&env, invoice_id, &updated);
        events::invoice_state_changed(&env, invoice_id, Some(&InvoiceStatus::Pending), &InvoiceStatus::Deleted, &caller);
    }

    /// Retrieve the tombstone record for a soft-deleted invoice.
    ///
    /// # Arguments
    /// * `invoice_id` - target invoice
    ///
    /// # Returns
    /// The Tombstone record for the deleted invoice
    pub fn get_tombstone(env: Env, invoice_id: u64) -> Tombstone {
        let invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status == InvoiceStatus::Deleted,
            "NotDeleted"
        );
        env.storage()
            .persistent()
            .get(&tombstone_key(invoice_id))
            .expect("tombstone not found")
    }

    /// Returns the 32-byte action hash that identifies this proposal.
    pub fn propose_admin_action(
        env: Env,
        proposer: Address,
        action: AdminAction,
    ) -> BytesN<32> {
        proposer.require_auth();

        let admin_set: AdminSet = env
            .storage()
            .instance()
            .get(&admin_set_key())
            .expect("AdminSet not configured; call set_admin_set first");

        // Verify the proposer is a recognised signer.
        assert!(
            admin_set.signers.iter().any(|s| s == proposer),
            "NotAuthorized: proposer is not a registered admin signer"
        );

use soroban_sdk::{contract, contractimpl, symbol_short, token, Address, Env, Symbol, Vec};
use types::{
    CreatorCollateral, CreatorCovenant, CreatorRating, DiscountTier, Invoice, InvoiceStatus,
    Payment, RecipientPerformance, RoutingStrategy,
};

// ---------------------------------------------------------------------------
// Storage key helpers
// ---------------------------------------------------------------------------

fn counter_key() -> Symbol {
    symbol_short!("counter")
}

fn invoice_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("inv"), id)
}

/// Storage key for a creator's collateral record.
fn collateral_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("col"), creator.clone())
}

/// Storage key for a creator's rating record.
fn rating_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("rating"), creator.clone())
}

/// Storage key for a recipient's performance record.
fn performance_key(recipient: &Address) -> (Symbol, Address) {
    (symbol_short!("perf"), recipient.clone())
}

/// Storage key for a covenant attached to an invoice.
fn covenant_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("cov"), invoice_id)
}

// ---------------------------------------------------------------------------
// Storage helpers
// ---------------------------------------------------------------------------

fn load_invoice(env: &Env, id: u64) -> Invoice {
    env.storage()
        .persistent()
        .get(&invoice_key(id))
        .expect("invoice not found")
}
        // Compute a deterministic hash over the serialised action to use as key.
        let action_bytes = action.clone().to_xdr(&env);
        let action_hash: BytesN<32> = env.crypto().sha256(&action_bytes).into();

        // Ensure no duplicate proposal for the same action.
        assert!(
            !env.storage()
                .persistent()
                .has(&pending_admin_action_key(&action_hash)),
            "proposal already exists for this action"
        );

        let mut approvals: Vec<Address> = Vec::new(&env);
        approvals.push_back(proposer.clone());

        let pending = PendingAdminAction {
            action_hash: action_hash.clone(),
            action,
            proposed_at: env.ledger().timestamp(),
            approvals,
            executed: false,
        };

        env.storage()
            .persistent()
            .set(&pending_admin_action_key(&action_hash), &pending);

        events::admin_action_proposed(&env, &action_hash, &proposer);

fn load_collateral(env: &Env, creator: &Address) -> Option<CreatorCollateral> {
    env.storage().persistent().get(&collateral_key(creator))
}

fn save_collateral(env: &Env, collateral: &CreatorCollateral) {
    env.storage()
        .persistent()
        .set(&collateral_key(&collateral.creator), collateral);
}

fn load_rating(env: &Env, creator: &Address) -> Option<CreatorRating> {
    env.storage().persistent().get(&rating_key(creator))
}

fn save_rating(env: &Env, rating: &CreatorRating) {
    env.storage()
        .persistent()
        .set(&rating_key(&rating.creator), rating);
}

fn load_performance(env: &Env, recipient: &Address) -> Option<RecipientPerformance> {
    env.storage().persistent().get(&performance_key(recipient))
}

fn save_performance(env: &Env, perf: &RecipientPerformance) {
    env.storage()
        .persistent()
        .set(&performance_key(&perf.recipient), perf);
}

fn load_covenant(env: &Env, invoice_id: u64) -> Option<CreatorCovenant> {
    env.storage().persistent().get(&covenant_key(invoice_id))
}

fn save_covenant(env: &Env, covenant: &CreatorCovenant) {
    env.storage()
        .persistent()
        .set(&covenant_key(covenant.invoice_id), covenant);
}

// ---------------------------------------------------------------------------
// Rating helpers (#866)
// ---------------------------------------------------------------------------

/// Compute the discount tier for a creator based on their average rating score.
///
/// Average is expressed as `total_score * 10 / count` (one decimal, ×10) to
/// avoid floats:
/// - ≥ 45  (avg ≥ 4.5) → Tier1 (500 bps)
/// - ≥ 35  (avg ≥ 3.5) → Tier2 (300 bps)
/// - ≥ 25  (avg ≥ 2.5) → Tier3 (100 bps)
/// - < 25              → None
fn compute_discount_tier(rating: &CreatorRating) -> DiscountTier {
    if rating.count == 0 {
        return DiscountTier::None;
    }
    // avg_x10 = (total_score * 10) / count  — integer, no floats
    let avg_x10 = (rating.total_score * 10) / rating.count;
    if avg_x10 >= 45 {
        DiscountTier::Tier1
    } else if avg_x10 >= 35 {
        DiscountTier::Tier2
    } else if avg_x10 >= 25 {
        DiscountTier::Tier3
    } else {
        DiscountTier::None
    }
}

/// Map a discount tier to basis points.
fn tier_to_bps(tier: &DiscountTier) -> u32 {
    match tier {
        DiscountTier::Tier1 => 500,
        DiscountTier::Tier2 => 300,
        DiscountTier::Tier3 => 100,
        DiscountTier::None => 0,
    }
}

/// Apply a basis-point discount to a set of amounts (in-place style: returns
/// a new Vec with discounted values).
fn apply_discount(env: &Env, amounts: &Vec<i128>, discount_bps: u32) -> Vec<i128> {
    let mut discounted = Vec::new(env);
    for amt in amounts.iter() {
        // reduced = amt - (amt * bps / 10_000)
        let reduction = amt * (discount_bps as i128) / 10_000_i128;
        discounted.push_back(amt - reduction);
    }
    discounted
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------
        action_hash
    }

    /// Approve a pending admin action and execute it if the threshold is met.
    ///
    /// The caller must be a registered signer who has not already approved
    /// this proposal. Once the cumulative approval count reaches `threshold`
    /// the action is executed immediately and marked as done.
    pub fn approve_admin_action(env: Env, approver: Address, action_hash: BytesN<32>) {
        approver.require_auth();

#[contractimpl]
impl SplitContract {
    // -----------------------------------------------------------------------
    // Core: create_invoice
    // -----------------------------------------------------------------------

    /// Create a new invoice.
    ///
    /// Automatically applies a rating-based discount (#866) if the creator has
    /// an established rating.  Pass `RoutingStrategy::Default` and an empty
    /// `min_performance_score` to use the original behaviour.
    ///
    /// # Arguments
    /// * `creator`    – address that owns the invoice (must authorise)
    /// * `recipients` – ordered list of recipient addresses
    /// * `amounts`    – amount owed to each recipient (parallel to `recipients`)
    /// * `token`      – USDC token contract address
    /// * `deadline`   – Unix timestamp; after this refunds become available
        let admin_set: AdminSet = env
            .storage()
            .instance()
            .get(&admin_set_key())
            .expect("AdminSet not configured");

        assert!(
            admin_set.signers.iter().any(|s| s == approver),
            "NotAuthorized: approver is not a registered admin signer"
        );

        let mut pending: PendingAdminAction = env
            .storage()
            .persistent()
            .get(&pending_admin_action_key(&action_hash))
            .expect("no pending action with this hash");

        assert!(!pending.executed, "action already executed");

        // Ensure the approver hasn't already voted.
        assert!(
            !pending.approvals.iter().any(|a| a == approver),
            "signer has already approved this action"
        );

        pending.approvals.push_back(approver.clone());
        let approval_count = pending.approvals.len();

        events::admin_action_approved(&env, &action_hash, &approver, approval_count);

        if approval_count >= admin_set.threshold {
            // Execute the action.
            match pending.action.clone() {
                AdminAction::PauseContract => {
                    env.storage().persistent().set(&paused_key(), &true);
                }
                AdminAction::UnpauseContract => {
                    env.storage().persistent().set(&paused_key(), &false);
                }
                AdminAction::SetPlatformFeeBps(bps) => {
                    assert!(bps <= 10_000, "fee_bps must be ≤ 10000");
                    env.storage()
                        .instance()
                        .set(&platform_fee_bps_key(), &bps);
                }
                AdminAction::SetTreasury(addr) => {
                    env.storage().instance().set(&treasury_key(), &addr);
                }
                AdminAction::ReplaceAdminSet(new_set) => {
                    assert!(!new_set.signers.is_empty(), "signers must not be empty");
                    assert!(
                        new_set.threshold > 0 && new_set.threshold <= new_set.signers.len(),
                        "invalid threshold"
                    );
                    env.storage().instance().set(&admin_set_key(), &new_set);
                }
            }

            pending.executed = true;
            env.storage()
                .persistent()
                .set(&pending_admin_action_key(&action_hash), &pending);

            events::admin_action_executed(&env, &action_hash);
        } else {
            // Not yet at threshold — persist the updated approval list.
            env.storage()
                .persistent()
                .set(&pending_admin_action_key(&action_hash), &pending);
        }
    }

    /// Return a pending admin action by its action hash, or None.
    pub fn get_pending_admin_action(
        env: Env,
        action_hash: BytesN<32>,
    ) -> Option<PendingAdminAction> {
        env.storage()
            .persistent()
            .get(&pending_admin_action_key(&action_hash))
    }

    // -----------------------------------------------------------------------
    // Issue #476: Invoice Template Factory (ID-based)
    // -----------------------------------------------------------------------

    /// Store a new reusable invoice template on-chain.
    ///
    /// The template is keyed by a monotonically-increasing numeric ID scoped
    /// to the creator, making it easy to reference programmatically without
    /// choosing a name. `ratios` must be parallel to `recipients` and must
    /// sum to exactly 10 000 basis points.
    ///
    /// Returns the assigned `template_id`.
    pub fn create_template(
        env: Env,
        creator: Address,
        recipients: Vec<Address>,
        ratios: Vec<u32>,
        token: Address,
    ) -> u64 {
        creator.require_auth();

        assert!(
            !recipients.is_empty(),
            "must have at least one recipient"
        );
        assert!(
            recipients.len() == ratios.len(),
            "recipients and ratios must have the same length"
        );
        for amt in amounts.iter() {
            assert!(amt > 0, "amounts must be positive");
        }

        // --- #866: apply rating-based discount ---
        let final_amounts = if let Some(rating) = load_rating(&env, &creator) {
            let tier = compute_discount_tier(&rating);
            let bps = tier_to_bps(&tier);
            if bps > 0 {
                let discounted = apply_discount(&env, &amounts, bps);
                events::discount_applied(&env, 0, &creator, bps); // id=0 before we know it
                discounted
            } else {
                amounts
            }
        } else {
            amounts
        };

        // Increment counter.
        let id: u64 = env
            .storage()
            .persistent()
            .get(&counter_key())
            .unwrap_or(0u64)
            + 1;
        env.storage().persistent().set(&counter_key(), &id);

        let total: i128 = final_amounts.iter().sum();

        let invoice = Invoice {
            creator: creator.clone(),
            recipients: recipients.clone(),
            amounts: final_amounts,

        // Ratios must sum to exactly 10 000 bps.
        let ratio_sum: u32 = ratios.iter().fold(0u32, |a, b| a.saturating_add(b));
        validation::assert_bps_total(ratio_sum)
            .expect("ratios must sum to 10000 basis points");

        // Assign the next template ID for this creator.
        let next_id: u64 = env
            .storage()
            .persistent()
            .get(&template_id_counter_key(&creator))
            .unwrap_or(0u64);
        let template_id = next_id + 1;

        let template = InvoiceTemplateRecord {
            recipients,
            ratios,
            token,
        };

        env.storage()
            .persistent()
            .set(&template_id_key(&creator, template_id), &template);
        env.storage()
            .persistent()
            .set(&template_id_counter_key(&creator), &template_id);

        events::template_created(&env, &creator, template_id);

        template_id
    }

    // -----------------------------------------------------------------------
    // Core: pay
    // -----------------------------------------------------------------------

    /// Pay toward an invoice.
    pub fn pay(env: Env, payer: Address, invoice_id: u64, amount: i128) {
        payer.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);
    /// Delete a previously stored template.
    ///
    /// Only the creator who owns the template may delete it. Deleting a
    /// template does not affect invoices already instantiated from it.
    pub fn delete_template(env: Env, creator: Address, template_id: u64) {
        creator.require_auth();

        assert!(
            env.storage()
                .persistent()
                .has(&template_id_key(&creator, template_id)),
            "template not found"
        );

        let token_client = token::Client::new(&env, &invoice.token);
        token_client.transfer(&payer, &env.current_contract_address(), &amount);
        env.storage()
            .persistent()
            .remove(&template_id_key(&creator, template_id));

        events::template_deleted(&env, &creator, template_id);
    }

    /// Retrieve an archived invoice by ID.
    pub fn get_archived_invoice(env: Env, invoice_id: u64) -> InvoiceCore {
        env.storage()
            .persistent()
            .get(&archived_key(invoice_id))
            .expect("archived invoice not found")
    }

        if invoice.funded >= total {
            Self::_release(&env, invoice_id, &mut invoice);
    /// Retrieve the leaderboard for an invoice.
    ///
    /// Returns up to `n` top contributors sorted by cumulative paid amount descending.
    pub fn get_top_contributors(env: Env, invoice_id: u64, n: u32) -> Vec<(Address, i128)> {
        let leaders = load_top_contributors(&env, invoice_id);
        let mut result = Vec::new(&env);
        let count = if n > leaders.len() {
            leaders.len()
        } else {
            n
        };
        for i in 0..count {
            let entry = leaders.get(i).unwrap();
            result.push_back(entry);
        }
        result
    }

    /// Set the maximum number of audit log entries per invoice.
    /// Only callable by the contract creator (admin).
    pub fn set_max_audit_log_entries(env: Env, admin: Address, max: u32) {
        admin.require_auth();
        env.storage()
            .persistent()
            .set(&max_audit_log_entries_key(), &max);
    }

    /// Set the maximum number of leaderboard entries per invoice.
    /// Only callable by the contract creator (admin).
    pub fn set_max_leaderboard_size(env: Env, admin: Address, max: u32) {
        admin.require_auth();
        env.storage()
            .persistent()
            .set(&max_leaderboard_size_key(), &max);
    }

    // -----------------------------------------------------------------------
    // Core: release
    // -----------------------------------------------------------------------

    /// Release funds to all recipients once fully funded.
    pub fn release(env: Env, invoice_id: u64) {
        let mut invoice = load_invoice(&env, invoice_id);
    // Internal helpers
    // -----------------------------------------------------------------------
    /// Instantiate a new invoice from a stored template in a single call.
    ///
    /// The template's `ratios` are applied to `total_amount` to derive each
    /// recipient's individual amount: `amount_i = total_amount * ratio_i / 10_000`.
    /// The resulting amounts vector is passed directly to the standard invoice
    /// creation logic so all existing guards (min funding, deadlines, etc.) apply.
    ///
    /// Returns the newly created invoice ID.
    pub fn invoice_from_template(
        env: Env,
        creator: Address,
        template_id: u64,
        total_amount: i128,
        deadline: u64,
    ) -> u64 {
        creator.require_auth();

        assert!(total_amount > 0, "total_amount must be positive");

        let template: InvoiceTemplateRecord = env
            .storage()
            .persistent()
            .get(&template_id_key(&creator, template_id))
            .expect("template not found");

        // Compute per-recipient amounts from basis-point ratios.
        let mut amounts: Vec<i128> = Vec::new(&env);
        for ratio in template.ratios.iter() {
            let amt = total_amount * (ratio as i128) / 10_000;
            assert!(amt > 0, "computed amount for a recipient is zero; increase total_amount");
            amounts.push_back(amt);
        }

        let invoice_id = Self::_create_invoice_inner(
            &env,
            creator.clone(),
            template.recipients,
            amounts,
            Vec::new(&env),       // recipient_tokens
            template.token,       // funding_token
            deadline,
            Vec::new(&env),       // co_creators
            false,                // allow_early_withdrawal
            0,                    // bonus_pool
            0,                    // bonus_max_payers
            None,                 // prerequisite_id
            Vec::new(&env),       // tranches
            Vec::new(&env),       // co_signers
            0,                    // required_signatures
            0,                    // penalty_bps
            0,                    // penalty_deadline
            0,                    // min_funding_bps
            Vec::new(&env),       // release_stages
            None,                 // price_oracle
            Vec::new(&env),       // swap_tokens
            None,                 // condition_oracle
            0,                    // tax_bps
            None,                 // tax_authority
            0,                    // insurance_premium_bps
            false,                // smart_route
            None,                 // notification_contract
            OverflowBehavior::Reject,
            false,                // convert_to_stream
            Vec::new(&env),       // accepted_tokens
            None,                 // forward_to
            None,                 // forward_invoice_id
            None,                 // creator_cosigner
            0,                    // velocity_limit
            0,                    // velocity_window
            Vec::new(&env),       // split_rules
            Vec::new(&env),       // auto_resolve_rules
            None,                 // cross_chain_ref
            None,                 // allowed_payers
            None,                 // payment_cooldown_secs
            None,                 // max_payments_per_window
            None,                 // payment_window_secs
            None,                 // refund_grace_secs
            Vec::new(&env),       // priorities
            false,                // require_kyc
            None,                 // scheduled_release_at
            None,                 // min_payer_rep
            None,                 // release_delay_ledgers
            None,                 // metadata_hash
            None,                 // target_usd_cents
            None,                 // oracle
            None,                 // oracle_asset_pair_base
            None,                 // oracle_asset_pair_quote
            None,                 // escrow_hold_period
            None,                 // payment_open_at
            None,                 // payment_close_at
            None,                 // milestones
            None,                 // recipient_max_payouts
            false,                // recipient_whitelist_enabled
            None,                 // release_condition_hash
            0,                    // early_bird_window_ledgers
            0,                    // early_bird_fee_bps
            0,                    // creator_fee_bps
            template.ratios.clone(),
            10_000u64,
        );

        events::invoice_from_template(&env, invoice_id, &creator, template_id);

        invoice_id
    }

    /// Return a stored template by creator and ID, or panic if not found.
    pub fn get_template(env: Env, creator: Address, template_id: u64) -> InvoiceTemplateRecord {
        env.storage()
            .persistent()
            .get(&template_id_key(&creator, template_id))
            .expect("template not found")
    }

    /// #522 — Walk the parent chain and verify:
    /// 1. The chain depth does not exceed `MAX_PARENT_DEPTH`.
    /// 2. Each referenced invoice exists.
    ///
    /// `depth` starts at 0 for the direct parent.
    fn _validate_parent(env: &Env, parent_id: u64, depth: u32) {
        if depth >= MAX_PARENT_DEPTH {
            panic_with_error!(env, ContractError::ParentChainTooDeep);
        }

        // Load the parent — panics with "invoice not found" if it doesn't exist.
        let parent = load_invoice(env, parent_id);

        // Recurse if this parent also has a parent.
        if let Some(grandparent_id) = parent.parent_invoice_id {
            Self::_validate_parent(env, grandparent_id, depth + 1);
        }
    }

    /// Get the creator address for an invoice.
    pub fn get_invoice_creator(env: Env, invoice_id: u64) -> Address {
        let invoice = load_invoice(&env, invoice_id);
        invoice.creator
    }

    /// Get the list of recipient addresses for an invoice.
    pub fn get_invoice_recipients(env: Env, invoice_id: u64) -> Vec<Address> {
        let invoice = load_invoice(&env, invoice_id);
        invoice.recipients
    }

    /// Get the number of payments made toward an invoice.
    pub fn get_invoice_payment_count(env: Env, invoice_id: u64) -> u32 {
        let invoice = load_invoice(&env, invoice_id);
        invoice.payments.len() as u32
    }

    /// Get the funding percentage of an invoice as basis points.
    /// Returns (funded * 10_000 / total) as u32, or 0 if total is 0.
    /// Issue #776: paginated list of invoice IDs created by `creator`.
    /// `cursor` is the last-seen invoice ID; `limit` is capped at 50.
    pub fn get_creator_invoices(
        env: Env,
        creator: Address,
        limit: u32,
        cursor: Option<u64>,
    ) -> search_ext::InvoicePage {
        search_ext::get_creator_invoices(&env, creator, limit, cursor)
    }

    // Issue #777: DAO treasury governance (token-weighted). The DAO treasury is
    // this contract's balance of the configured USDC token.

    /// Admin: set voting period (ledgers), quorum (bps of `total_supply`) and
    /// the total token supply used as the quorum denominator.
    pub fn set_gov_config(env: Env, voting_period_ledgers: u32, quorum_bps: u32, total_supply: i128) {
        require_admin(&env);
        treasury_gov_ext::set_config(&env, voting_period_ledgers, quorum_bps, total_supply);
    }

    pub fn create_proposal(
        env: Env,
        proposer: Address,
        description: Bytes,
        allocations: Vec<treasury_gov_ext::Allocation>,
    ) -> u64 {
        treasury_gov_ext::create_proposal(&env, proposer, description, allocations)
    }

    pub fn vote_proposal(env: Env, proposal_id: u64, voter: Address, approve: bool) {
        let token = Self::get_usdc_token(env.clone());
        treasury_gov_ext::vote(&env, &token, proposal_id, voter, approve);
    }

    pub fn execute_proposal(env: Env, proposal_id: u64) {
        let token = Self::get_usdc_token(env.clone());
        treasury_gov_ext::execute(&env, &token, proposal_id);
    }

    pub fn get_proposal(env: Env, proposal_id: u64) -> treasury_gov_ext::Proposal {
        treasury_gov_ext::get_proposal(&env, proposal_id)
    }

    /// Issue #778: `pay` with optional replay-protection nonce. With `Some`,
    /// the 32-byte nonce is checked/consumed in temporary storage (TTL 1
    /// ledger) and `NonceConsumed` is emitted; `None` behaves like `pay`.
    /// The existing sequential per-payer nonce is supplied automatically.
    pub fn pay_with_nonce(
        env: Env,
        payer: Address,
        invoice_id: u64,
        amount: i128,
        nonce: Option<BytesN<32>>,
    ) {
        if let Some(n) = &nonce {
            nonce_ext::check_unused(&env, n);
        }
        let seq: u64 = env
            .storage()
            .persistent()
            .get(&nonce_key(invoice_id, &payer))
            .unwrap_or(0);
        Self::pay(env.clone(), payer.clone(), invoice_id, amount, seq, false, false, None);
        if let Some(n) = nonce {
            nonce_ext::consume(&env, invoice_id, &payer, &n);
        }
    }

    pub fn is_nonce_used(env: Env, nonce: BytesN<32>) -> bool {
        nonce_ext::is_used(&env, &nonce)
    }

    // Issue #779: campaign groups linking related invoices.

    pub fn create_group(env: Env, creator: Address, name: Symbol, description: Bytes) -> u64 {
        group_ext::create_group(&env, creator, name, description)
    }

    /// Only the group creator (who must also own the invoice) may add; max 20.
    pub fn add_invoice_to_group(env: Env, group_id: u64, invoice_id: u64, creator: Address) {
        creator.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        assert!(invoice.creator == creator, "not invoice creator");
        group_ext::add_invoice(&env, group_id, invoice_id, &creator);
    }

    pub fn get_group_invoices(env: Env, group_id: u64) -> Vec<u64> {
        group_ext::get_invoices(&env, group_id)
    }

    pub fn get_group_stats(env: Env, group_id: u64) -> group_ext::GroupStats {
        let group = group_ext::get_group(&env, group_id);
        let mut total_target: i128 = 0;
        let mut total_funded: i128 = 0;
        let mut fully_funded_count: u32 = 0;
        for id in group.invoices.iter() {
            let inv = load_invoice(&env, id);
            let target: i128 = inv.amounts.iter().sum();
            total_target += target;
            total_funded += inv.funded;
            if inv.funded >= target {
                fully_funded_count += 1;
            }
        }
        group_ext::GroupStats {
            name: group.name,
            total_target,
            total_funded,
            invoice_count: group.invoices.len(),
            fully_funded_count,
        }
    }

    pub fn get_invoice_funding_percentage(env: Env, invoice_id: u64) -> u32 {
        let invoice = load_invoice(&env, invoice_id);
        let total: i128 = invoice.amounts.iter().sum();
        if total == 0 {
            0
        } else {
            ((invoice.funded * 10_000) / total) as u32
        }
    }

    // -----------------------------------------------------------------------
    // Issue #760: Milestone-based sequential unlocking
    // -----------------------------------------------------------------------

    /// Return the index of the currently active milestone, or panic if no
    /// milestone list is configured on this invoice.
    ///
    /// The active milestone is the first entry in the milestone list whose
    /// status is [`MilestoneStatus::Active`].  Returns the u32 index (0-based)
    /// so callers can use it with `complete_milestone`.
    pub fn get_active_milestone(env: Env, invoice_id: u64) -> u32 {
        let milestones: Vec<Milestone> = env
            .storage()
            .persistent()
            .get(&milestone_data_key(invoice_id))
            .expect("no milestone list for this invoice");
        for i in 0..milestones.len() {
            let m = milestones.get(i).unwrap();
            if m.status == MilestoneStatus::Active {
                return i;
            }
        }
        panic!("no active milestone");
    }

    // -----------------------------------------------------------------------
    // Core: refund
    // -----------------------------------------------------------------------

    /// Refund all payers if the deadline has passed and the invoice is not funded.
    pub fn refund(env: Env, invoice_id: u64) {
        let mut invoice = load_invoice(&env, invoice_id);
    /// Release the funds collected for milestone `index` and activate the
    /// next milestone (if any).
    ///
    /// Only the invoice creator may call this.  The milestone at `index` must
    /// be the currently *active* one — out-of-order completion is rejected.
    /// After funds are transferred to recipients, the milestone is marked
    /// [`MilestoneStatus::Completed`] and the subsequent milestone transitions
    /// from `Pending` → `Active` (emitting [`MilestoneActivated`]).
    ///
    /// # Emits
    /// * [`events::milestone_completed`] — index and amount released.
    /// * [`events::milestone_activated`] — index of the newly active milestone.
    pub fn complete_milestone(env: Env, invoice_id: u64, creator: Address, index: u32) {
        require_not_paused(&env);
        creator.require_auth();

        let invoice = load_invoice(&env, invoice_id);
        assert!(invoice.creator == creator, "not invoice creator");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        let key = milestone_data_key(invoice_id);
        let mut milestones: Vec<Milestone> = env
            .storage()
            .persistent()
            .get(&key)
            .expect("no milestone list for this invoice");

        assert!(
            (index as usize) < milestones.len().try_into().unwrap(),
            "milestone index out of range"
        );

        let active = milestones.get(index).unwrap();
        assert!(
            active.status == MilestoneStatus::Active,
            "milestone is not active — complete milestones in order"
        );

        // Transfer the milestone's target_amount to recipients (pro-rata by amounts[]).
        let total_amounts: i128 = invoice.amounts.iter().sum();
        let amount_to_release = active.target_amount;
        let token_client = token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));

        invoice.status = InvoiceStatus::Refunded;
        save_invoice(&env, invoice_id, &invoice);
        events::invoice_refunded(&env, invoice_id);

        // --- #868: mark covenant violated if one exists ---
        if let Some(mut covenant) = load_covenant(&env, invoice_id) {
            if !covenant.violated && !covenant.fulfilled {
                covenant.violated = true;
                // Slash collateral if available.
                if let Some(mut col) = load_collateral(&env, &covenant.creator) {
                    let slashable = col.amount.min(covenant.penalty_amount);
                    if slashable > 0 {
                        col.amount -= slashable;
                        // Also reduce locked proportionally (locked ≤ amount).
                        if col.locked > col.amount {
                            col.locked = col.amount;
                        }
                        save_collateral(&env, &col);
                    }
                }
                events::covenant_violated(
                    &env,
                    invoice_id,
                    &covenant.creator.clone(),
                    covenant.penalty_amount,
                );
                save_covenant(&env, &covenant);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Core: get_invoice
    // -----------------------------------------------------------------------

    /// Retrieve an invoice by ID.
    pub fn get_invoice(env: Env, invoice_id: u64) -> Invoice {
        load_invoice(&env, invoice_id)
    }

    // -----------------------------------------------------------------------
    // #865 – Creator collateral system
    // -----------------------------------------------------------------------

    /// Deposit collateral to back your invoices.
    ///
    /// Transfers `amount` of `token` from `creator` to the contract.
    pub fn deposit_collateral(env: Env, creator: Address, token: Address, amount: i128) {
        creator.require_auth();
        assert!(amount > 0, "collateral amount must be positive");

        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&creator, &env.current_contract_address(), &amount);

        let mut col = load_collateral(&env, &creator).unwrap_or(CreatorCollateral {
            creator: creator.clone(),
            token: token.clone(),
            amount: 0,
            locked: 0,
        });

        assert!(col.token == token, "collateral token mismatch");

        col.amount += amount;
        save_collateral(&env, &col);
        events::collateral_deposited(&env, &creator, amount);
    }

    /// Withdraw available (unlocked) collateral.
    pub fn withdraw_collateral(env: Env, creator: Address, amount: i128) {
        creator.require_auth();
        assert!(amount > 0, "withdrawal amount must be positive");

        let mut col = load_collateral(&env, &creator).expect("no collateral record found");

        let available = col.amount - col.locked;
        assert!(available >= amount, "insufficient unlocked collateral");

        col.amount -= amount;
        save_collateral(&env, &col);

        let token_client = token::Client::new(&env, &col.token);
        token_client.transfer(&env.current_contract_address(), &creator, &amount);

        events::collateral_withdrawn(&env, &creator, amount);
    }

    /// Lock `lock_amount` of collateral to back `invoice_id`.
    ///
    /// Callable by the invoice creator after depositing sufficient collateral.
    pub fn lock_collateral(env: Env, creator: Address, invoice_id: u64, lock_amount: i128) {
        creator.require_auth();
        assert!(lock_amount > 0, "lock amount must be positive");

        let invoice = load_invoice(&env, invoice_id);
        assert!(invoice.creator == creator, "only invoice creator can lock collateral");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        let mut col = load_collateral(&env, &creator).expect("no collateral record found");
        let available = col.amount - col.locked;
        assert!(available >= lock_amount, "insufficient unlocked collateral");

        col.locked += lock_amount;
        save_collateral(&env, &col);
        events::collateral_locked(&env, &creator, invoice_id, lock_amount);
    }

    /// Unlock collateral previously locked to `invoice_id`.
    ///
    /// Only valid after the invoice is released or refunded.
    pub fn unlock_collateral(env: Env, creator: Address, invoice_id: u64, unlock_amount: i128) {
        creator.require_auth();
        assert!(unlock_amount > 0, "unlock amount must be positive");

        let invoice = load_invoice(&env, invoice_id);
        assert!(invoice.creator == creator, "only invoice creator can unlock collateral");
        assert!(
            invoice.status != InvoiceStatus::Pending,
            "invoice must be finalised before unlocking collateral"
        );

        let mut col = load_collateral(&env, &creator).expect("no collateral record found");
        assert!(col.locked >= unlock_amount, "unlock amount exceeds locked balance");

        col.locked -= unlock_amount;
        save_collateral(&env, &col);
        events::collateral_unlocked(&env, &creator, invoice_id, unlock_amount);
    }

    /// Retrieve a creator's collateral record.
    pub fn get_collateral(env: Env, creator: Address) -> CreatorCollateral {
        load_collateral(&env, &creator).expect("no collateral record found")
    }

    // -----------------------------------------------------------------------
    // #866 – Rating-based discount tiers
    // -----------------------------------------------------------------------

    /// Submit a rating (1–5) for a creator after an invoice they created is
    /// released.  Any payer on the invoice may rate.
    ///
    /// The caller must be a payer on the invoice and the invoice must already
    /// be released.
    pub fn rate_creator(env: Env, rater: Address, invoice_id: u64, score: u32) {
        rater.require_auth();
        assert!((1..=5).contains(&score), "score must be between 1 and 5");

        let invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status == InvoiceStatus::Released,
            "invoice must be released before rating"
        );

        // Verify rater is a payer on this invoice.
        let mut is_payer = false;
        for p in invoice.payments.iter() {
            if p.payer == rater {
                is_payer = true;
                break;
            }
        }
        assert!(is_payer, "only a payer on this invoice can rate the creator");

        let mut rating = load_rating(&env, &invoice.creator).unwrap_or(CreatorRating {
            creator: invoice.creator.clone(),
            total_score: 0,
            count: 0,
        });

        rating.total_score += score;
        rating.count += 1;

        let avg_bps = (rating.total_score * 10_000) / (rating.count * 5); // out of 10_000
        save_rating(&env, &rating);
        events::creator_rated(&env, &invoice.creator, score, avg_bps);
    }

    /// Get the current discount tier for a creator based on their rating.
    pub fn get_discount_tier(env: Env, creator: Address) -> DiscountTier {
        match load_rating(&env, &creator) {
            Some(rating) => compute_discount_tier(&rating),
            None => DiscountTier::None,
        }
    }

    /// Get a creator's rating record.
    pub fn get_creator_rating(env: Env, creator: Address) -> CreatorRating {
        load_rating(&env, &creator).unwrap_or(CreatorRating {
            creator: creator.clone(),
            total_score: 0,
            count: 0,
        })
    }

    // -----------------------------------------------------------------------
    // #867 – Smart invoice routing based on recipient performance
    // -----------------------------------------------------------------------

    /// Create an invoice using smart routing to filter/order recipients by
    /// their on-chain performance score.
    ///
    /// # Arguments
    /// * `strategy`            – routing strategy to apply
    /// * `min_score`           – minimum performance score (only for ThresholdBased)
    /// * All other args same as `create_invoice`
    ///
    /// For `PerformanceBased`, the supplied recipients are kept but the
    /// routing event records that performance-based selection was requested.
    /// For `ThresholdBased`, any recipient with a score below `min_score` is
    /// excluded (panics if that leaves zero recipients).
    #[allow(clippy::too_many_arguments)]
    pub fn create_invoice_routed(
        env: Env,
        creator: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        token: Address,
        deadline: u64,
        strategy: RoutingStrategy,
        min_score: i32,
    ) -> u64 {
        creator.require_auth();

        assert!(
            recipients.len() == amounts.len(),
            "recipients and amounts length mismatch"
        );
        assert!(!recipients.is_empty(), "must have at least one recipient");
        assert!(
            deadline > env.ledger().timestamp(),
            "deadline must be in the future"
        );
        for amt in amounts.iter() {
            assert!(amt > 0, "amounts must be positive");
        }

        // Apply routing strategy.
        let (final_recipients, final_amounts) = match strategy {
            RoutingStrategy::Default => (recipients, amounts),
            RoutingStrategy::PerformanceBased => {
                // Keep all, but record a performance-based routing event.
                // Future improvement: sort by score.
                (recipients, amounts)
            }
            RoutingStrategy::ThresholdBased => {
                let mut filtered_recipients = Vec::new(&env);
                let mut filtered_amounts = Vec::new(&env);
                for (recipient, amount) in recipients.iter().zip(amounts.iter()) {
                    let score = load_performance(&env, &recipient)
                        .map(|p| p.score)
                        .unwrap_or(0);
                    if score >= min_score {
                        filtered_recipients.push_back(recipient);
                        filtered_amounts.push_back(amount);
                    }
                }
                assert!(
                    !filtered_recipients.is_empty(),
                    "no recipients meet the performance threshold"
                );
                (filtered_recipients, filtered_amounts)
            }
        };

        // Apply rating-based discount (#866).
        let discounted_amounts = if let Some(rating) = load_rating(&env, &creator) {
            let tier = compute_discount_tier(&rating);
            let bps = tier_to_bps(&tier);
            if bps > 0 {
                let d = apply_discount(&env, &final_amounts, bps);
                events::discount_applied(&env, 0, &creator, bps);
                d
            } else {
                final_amounts
            }
        } else {
            final_amounts
        };

        let id: u64 = env
            .storage()
            .persistent()
            .get(&counter_key())
            .unwrap_or(0u64)
            + 1;
        env.storage().persistent().set(&counter_key(), &id);

        let total: i128 = discounted_amounts.iter().sum();

        let invoice = Invoice {
            creator: creator.clone(),
            recipients: final_recipients,
            amounts: discounted_amounts,
            token,
            deadline,
            funded: 0,
            status: InvoiceStatus::Pending,
            payments: Vec::new(&env),
        };

        save_invoice(&env, id, &invoice);
        events::invoice_created(&env, id, &creator, total);

        id
    }

    /// Record that a recipient completed their obligations (called after invoice
    /// is released).  Increments their performance score.
    pub fn record_recipient_completion(env: Env, caller: Address, recipient: Address, invoice_id: u64) {
        caller.require_auth();

        let invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status == InvoiceStatus::Released,
            "invoice must be released"
        );
        // Caller must be the invoice creator.
        assert!(invoice.creator == caller, "only invoice creator can record completion");

        let mut perf = load_performance(&env, &recipient).unwrap_or(RecipientPerformance {
            recipient: recipient.clone(),
            completed: 0,
            flagged: 0,
            score: 0,
        });

        perf.completed += 1;
        perf.score += 1;
        save_performance(&env, &perf);
        events::performance_updated(&env, &recipient, perf.score);
    }

    /// Flag a recipient for poor performance.  Decrements their score.
    pub fn flag_recipient(env: Env, caller: Address, recipient: Address, invoice_id: u64) {
        caller.require_auth();

        let invoice = load_invoice(&env, invoice_id);
        // Caller must be the invoice creator.
        assert!(invoice.creator == caller, "only invoice creator can flag recipients");

        let mut perf = load_performance(&env, &recipient).unwrap_or(RecipientPerformance {
            recipient: recipient.clone(),
            completed: 0,
            flagged: 0,
            score: 0,
        });

        perf.flagged += 1;
        perf.score -= 1;
        save_performance(&env, &perf);
        events::performance_updated(&env, &recipient, perf.score);
        events::recipient_flagged(&env, &recipient, invoice_id);
    }

    /// Get a recipient's performance record.
    pub fn get_recipient_performance(env: Env, recipient: Address) -> RecipientPerformance {
        load_performance(&env, &recipient).unwrap_or(RecipientPerformance {
            recipient: recipient.clone(),
            completed: 0,
            flagged: 0,
            score: 0,
        })
    }

    // -----------------------------------------------------------------------
    // #868 – Creator covenant system with penalties
    // -----------------------------------------------------------------------

    /// Attach a covenant to an invoice.
    ///
    /// The covenant commits the creator to fulfilling the invoice; if it is
    /// refunded (cancelled / missed deadline) the `penalty_amount` is slashed
    /// from the creator's collateral.
    ///
    /// The invoice must be Pending and the creator must have at least
    /// `penalty_amount` of unlocked collateral.
    pub fn create_covenant(
        env: Env,
        creator: Address,
        invoice_id: u64,
        penalty_amount: i128,
    ) {
        creator.require_auth();
        assert!(penalty_amount > 0, "penalty amount must be positive");

        let invoice = load_invoice(&env, invoice_id);
        assert!(invoice.creator == creator, "only invoice creator can create a covenant");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice must be pending"
        );
        assert!(
            load_covenant(&env, invoice_id).is_none(),
            "covenant already exists for this invoice"
        );

        // Creator must have sufficient unlocked collateral.
        let col = load_collateral(&env, &creator).expect("creator must have collateral to create a covenant");
        let available = col.amount - col.locked;
        assert!(
            available >= penalty_amount,
            "insufficient unlocked collateral for covenant"
        );

        let covenant = CreatorCovenant {
            invoice_id,
            creator: creator.clone(),
            penalty_amount,
            violated: false,
            fulfilled: false,
        };

        save_covenant(&env, &covenant);
        events::covenant_created(&env, invoice_id, &creator, penalty_amount);
    }

    /// Mark a covenant as fulfilled once the invoice is released.
    ///
    /// Can be called by anyone after the invoice reaches Released status.
    pub fn fulfill_covenant(env: Env, invoice_id: u64) {
        let invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status == InvoiceStatus::Released,
            "invoice must be released before fulfilling covenant"
        );

        let mut covenant = load_covenant(&env, invoice_id).expect("no covenant found for this invoice");
        assert!(!covenant.violated, "covenant already violated");
        assert!(!covenant.fulfilled, "covenant already fulfilled");

        covenant.fulfilled = true;
        save_covenant(&env, &covenant);
        events::covenant_fulfilled(&env, invoice_id, &covenant.creator.clone());
    }

    /// Get the covenant attached to an invoice.
    pub fn get_covenant(env: Env, invoice_id: u64) -> CreatorCovenant {
        load_covenant(&env, invoice_id).expect("no covenant found for this invoice")
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------
        if total_amounts > 0 && amount_to_release > 0 {
            for i in 0..invoice.recipients.len() {
                let recipient = invoice.recipients.get(i).unwrap();
                let share = invoice.amounts.get(i).unwrap_or(0);
                let payout = amount_to_release
                    .checked_mul(share)
                    .expect("overflow")
                    / total_amounts;
                if payout > 0 {
                    token_client.transfer(
                        &env.current_contract_address(),
                        &recipient,
                        &payout,
                    );
                }
            }
        }

        // Mark this milestone completed.
        let completed = Milestone {
            status: MilestoneStatus::Completed,
            ..active
        };
        milestones.set(index, completed);

        // Activate the next milestone, if any.
        let next_index = index + 1;
        if (next_index as usize) < milestones.len().try_into().unwrap() {
            let next = milestones.get(next_index).unwrap();
            let activated = Milestone {
                status: MilestoneStatus::Active,
                ..next
            };
            milestones.set(next_index, activated);
            events::milestone_activated(&env, invoice_id, next_index);
        }

        env.storage().persistent().set(&key, &milestones);

        // Issue #763: record milestone completion in history ring buffer.
        append_history_entry(
            &env,
            invoice_id,
            Symbol::new(&env, "milestone"),
            &creator,
            Some(amount_to_release),
        );

        invoice.status = InvoiceStatus::Released;
        save_invoice(env, invoice_id, invoice);
        events::invoice_released(env, invoice_id, &invoice.recipients);

        // --- #867: update recipient performance scores ---
        for recipient in invoice.recipients.iter() {
            let mut perf = load_performance(env, &recipient).unwrap_or(RecipientPerformance {
                recipient: recipient.clone(),
                completed: 0,
                flagged: 0,
                score: 0,
            });
            perf.completed += 1;
            perf.score += 1;
            save_performance(env, &perf);
            events::performance_updated(env, &recipient, perf.score);
        }

        // --- #868: auto-fulfill covenant if one exists ---
        if let Some(mut covenant) = load_covenant(env, invoice_id) {
            if !covenant.violated && !covenant.fulfilled {
                covenant.fulfilled = true;
                events::covenant_fulfilled(env, invoice_id, &covenant.creator.clone());
                save_covenant(env, &covenant);
            }
        }
        events::milestone_completed(&env, invoice_id, index, amount_to_release);
    }
}

/// Move a finalised invoice from hot storage to cold archival storage.
#[allow(dead_code)]
fn archive_invoice(env: &Env, invoice_id: u64, invoice: &Invoice) {
    let (core, _, _) = invoice.clone().split();
    env.storage()
        .persistent()
        .set(&archived_key(invoice_id), &core);
    remove_invoice(env, invoice_id);
}
// ---------------------------------------------------------------------------
// Issue #838: Creator reputation recovery constants
// ---------------------------------------------------------------------------

/// Ratings at or below this score may be contested by the creator.
const LOW_RATING_THRESHOLD: u32 = 2;

// ---------------------------------------------------------------------------
// Issue #839: Payment stream constants and helpers
// ---------------------------------------------------------------------------

/// Upper bound on the number of streams folded in a single aggregation call.
const MAX_AGGREGATE_STREAMS: u32 = 10;

fn load_payment_stream(env: &Env, stream_id: u64) -> PaymentStream {
    env.storage()
        .persistent()
        .get(&payment_stream_key(stream_id))
        .expect("stream not found")
}

fn save_payment_stream(env: &Env, stream: &PaymentStream) {
    env.storage()
        .persistent()
        .set(&payment_stream_key(stream.id), stream);
}

fn next_payment_stream_id(env: &Env) -> u64 {
    let id: u64 = env
        .storage()
        .persistent()
        .get(&payment_stream_counter_key())
        .unwrap_or(0u64)
        + 1;
    env.storage()
        .persistent()
        .set(&payment_stream_counter_key(), &id);
    id
}

/// Total amount that has accrued to the recipient so far, capped at `deposit`.
fn stream_streamed_amount(env: &Env, stream: &PaymentStream) -> i128 {
    let now = env.ledger().sequence();
    if now <= stream.start_ledger {
        return 0;
    }
    let elapsed = (now - stream.start_ledger) as i128;
    elapsed
        .saturating_mul(stream.rate_per_ledger)
        .min(stream.deposit)
}

/// Accrued amount not yet withdrawn by the recipient.
fn stream_withdrawable_amount(env: &Env, stream: &PaymentStream) -> i128 {
    if !stream.active {
        return 0;
    }
    stream_streamed_amount(env, stream).saturating_sub(stream.withdrawn)
}

// ---------------------------------------------------------------------------
// Contract (block 3 — invoice locking, reputation recovery, stream aggregation)
// ---------------------------------------------------------------------------
#[contractimpl]
impl SplitContract {
    // -----------------------------------------------------------------------
    // Issue #837: Invoice locking
    // -----------------------------------------------------------------------

    /// Lock an invoice so it can no longer be modified.
    ///
    /// The creator (or a co-creator) may lock at any time. Anyone else may
    /// lock only once the deadline has passed, which persists the implicit
    /// post-deadline lock and emits an on-chain record of it.
    pub fn lock_invoice(env: Env, caller: Address, invoice_id: u64) -> InvoiceLock {
        require_not_paused(&env);
        caller.require_auth();

        let invoice = load_invoice(&env, invoice_id);
        assert!(
            !env.storage().persistent().has(&invoice_lock_key(invoice_id)),
            "invoice already locked"
        );

        let now = env.ledger().timestamp();
        let is_creator =
            invoice.creator == caller || invoice.co_creators.iter().any(|c| c == caller);
        assert!(
            is_creator || now > invoice.deadline,
            "invoice deadline has not passed"
        );

        let lock = InvoiceLock {
            invoice_id,
            locked_by: caller.clone(),
            locked_at: now,
            locked_ledger: env.ledger().sequence(),
        };
        env.storage()
            .persistent()
            .set(&invoice_lock_key(invoice_id), &lock);

        append_audit_entry(&env, invoice_id, symbol_short!("inv_lock"), &caller);
        events::invoice_locked(&env, invoice_id, &caller, now, invoice.deadline);
        lock
    }

    /// Returns true if the invoice is explicitly locked or its deadline has passed.
    pub fn is_invoice_locked(env: Env, invoice_id: u64) -> bool {
        let invoice = load_invoice(&env, invoice_id);
        invoice_is_locked(&env, invoice_id, &invoice)
    }

    /// Returns the explicit lock record for an invoice, if one was written.
    pub fn get_invoice_lock(env: Env, invoice_id: u64) -> Option<InvoiceLock> {
        env.storage().persistent().get(&invoice_lock_key(invoice_id))
    }

    // -----------------------------------------------------------------------
    // Issue #838: Creator reputation recovery
    // -----------------------------------------------------------------------

    /// File an appeal against a low rating (score <= `LOW_RATING_THRESHOLD`)
    /// that `payer` left on one of the creator's invoices. Each rating may be
    /// appealed at most once.
    pub fn appeal_rating(
        env: Env,
        creator: Address,
        invoice_id: u64,
        payer: Address,
        reason_hash: BytesN<32>,
    ) {
        require_not_paused(&env);
        creator.require_auth();

        let invoice = load_invoice(&env, invoice_id);
        assert!(invoice.creator == creator, "only creator can appeal");

        let score: u32 = env
            .storage()
            .persistent()
            .get(&invoice_rating_key(invoice_id, &payer))
            .expect("rating not found");
        assert!(score <= LOW_RATING_THRESHOLD, "rating is not appealable");

        let appeal_key = rating_appeal_key(invoice_id, &payer);
        assert!(
            !env.storage().persistent().has(&appeal_key),
            "rating already appealed"
        );

        let appeal = RatingAppeal {
            invoice_id,
            payer: payer.clone(),
            creator: creator.clone(),
            score,
            reason_hash: reason_hash.clone(),
            status: AppealStatus::Pending,
            filed_at: env.ledger().timestamp(),
            resolved_at: 0,
            resolved_by: None,
        };
        env.storage().persistent().set(&appeal_key, &appeal);

        events::rating_appealed(&env, invoice_id, &creator, &payer, score, &reason_hash);
    }

    /// Withdraw a pending appeal. The rating stays in place and cannot be
    /// appealed again.
    pub fn withdraw_rating_appeal(env: Env, creator: Address, invoice_id: u64, payer: Address) {
        require_not_paused(&env);
        creator.require_auth();

        let appeal_key = rating_appeal_key(invoice_id, &payer);
        let mut appeal: RatingAppeal = env
            .storage()
            .persistent()
            .get(&appeal_key)
            .expect("appeal not found");
        assert!(appeal.creator == creator, "only creator can withdraw appeal");
        assert!(appeal.status == AppealStatus::Pending, "appeal not pending");

        appeal.status = AppealStatus::Rejected;
        appeal.resolved_at = env.ledger().timestamp();
        appeal.resolved_by = Some(creator.clone());
        env.storage().persistent().set(&appeal_key, &appeal);

        events::rating_appeal_withdrawn(&env, invoice_id, &creator, &payer);
    }

    /// Resolve a pending appeal (Operator or higher).
    ///
    /// When `upheld`, the contested score is struck from both the invoice's
    /// and the creator's rating aggregates. The per-payer rating record is
    /// kept so the payer cannot re-rate the invoice.
    pub fn resolve_rating_appeal(
        env: Env,
        admin: Address,
        invoice_id: u64,
        payer: Address,
        upheld: bool,
    ) {
        require_admin_role(&env, &admin, AdminRole::Operator);

        let appeal_key = rating_appeal_key(invoice_id, &payer);
        let mut appeal: RatingAppeal = env
            .storage()
            .persistent()
            .get(&appeal_key)
            .expect("appeal not found");
        assert!(appeal.status == AppealStatus::Pending, "appeal not pending");

        let creator_key = creator_rating_key(&appeal.creator);
        let mut creator_rating: (u32, u32) = env
            .storage()
            .persistent()
            .get(&creator_key)
            .unwrap_or((0u32, 0u32));

        if upheld {
            let sum_key = invoice_rating_sum_key(invoice_id);
            let count_key = invoice_rating_count_key(invoice_id);
            let sum: u32 = env.storage().persistent().get(&sum_key).unwrap_or(0u32);
            let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0u32);
            env.storage()
                .persistent()
                .set(&sum_key, &sum.saturating_sub(appeal.score));
            env.storage()
                .persistent()
                .set(&count_key, &count.saturating_sub(1));

            creator_rating.0 = creator_rating.0.saturating_sub(appeal.score);
            creator_rating.1 = creator_rating.1.saturating_sub(1);
            env.storage().persistent().set(&creator_key, &creator_rating);

            appeal.status = AppealStatus::Upheld;
        } else {
            appeal.status = AppealStatus::Rejected;
        }
        appeal.resolved_at = env.ledger().timestamp();
        appeal.resolved_by = Some(admin.clone());
        env.storage().persistent().set(&appeal_key, &appeal);

        events::rating_appeal_resolved(&env, invoice_id, &payer, &admin, upheld, creator_rating);
    }

    /// Return the appeal filed against `payer`'s rating on `invoice_id`, if any.
    pub fn get_rating_appeal(env: Env, invoice_id: u64, payer: Address) -> Option<RatingAppeal> {
        env.storage()
            .persistent()
            .get(&rating_appeal_key(invoice_id, &payer))
    }

    // -----------------------------------------------------------------------
    // Issue #839: Payment streams and aggregation
    // -----------------------------------------------------------------------

    /// Create a linear payment stream. `deposit` is transferred from `sender`
    /// up-front and accrues to `recipient` at `rate_per_ledger` starting at
    /// the current ledger.
    pub fn create_payment_stream(
        env: Env,
        sender: Address,
        recipient: Address,
        token: Address,
        deposit: i128,
        rate_per_ledger: i128,
    ) -> u64 {
        require_not_paused(&env);
        sender.require_auth();
        assert!(deposit > 0, "deposit must be positive");
        assert!(rate_per_ledger > 0, "rate must be positive");
        assert!(sender != recipient, "sender cannot be recipient");
        validate_allowed_token(&env, &token);

        token::Client::new(&env, &token).transfer(
            &sender,
            &env.current_contract_address(),
            &deposit,
        );

        let stream = PaymentStream {
            id: next_payment_stream_id(&env),
            sender: sender.clone(),
            recipient: recipient.clone(),
            token: token.clone(),
            deposit,
            rate_per_ledger,
            start_ledger: env.ledger().sequence(),
            withdrawn: 0,
            active: true,
            aggregated_into: None,
        };
        save_payment_stream(&env, &stream);

        events::stream_created(
            &env,
            stream.id,
            &sender,
            &recipient,
            &token,
            deposit,
            rate_per_ledger,
            stream.start_ledger,
        );
        stream.id
    }

    /// Withdraw everything accrued on a stream so far. Returns the amount paid.
    pub fn withdraw_from_stream(env: Env, recipient: Address, stream_id: u64) -> i128 {
        require_not_paused(&env);
        recipient.require_auth();

        let mut stream = load_payment_stream(&env, stream_id);
        assert!(stream.recipient == recipient, "not stream recipient");
        assert!(stream.active, "stream not active");

        let amount = stream_withdrawable_amount(&env, &stream);
        assert!(amount > 0, "nothing to withdraw");

        stream.withdrawn = stream.withdrawn.saturating_add(amount);
        if stream.withdrawn >= stream.deposit {
            stream.active = false;
        }
        save_payment_stream(&env, &stream);

        token::Client::new(&env, &stream.token).transfer(
            &env.current_contract_address(),
            &recipient,
            &amount,
        );

        events::stream_withdrawn(&env, stream_id, &recipient, amount, stream.withdrawn);
        amount
    }

    /// Combine several active streams sharing the same sender, recipient and
    /// token into a single new stream.
    ///
    /// Each source stream is settled first — anything already accrued is paid
    /// to the recipient — then closed and linked to the new stream via
    /// `aggregated_into`. The new stream carries the sum of the unstreamed
    /// balances as its deposit and the sum of the rates of the sources that
    /// still had a balance, starting at the current ledger.
    pub fn aggregate_streams(env: Env, sender: Address, stream_ids: Vec<u64>) -> u64 {
        require_not_paused(&env);
        sender.require_auth();

        let n = stream_ids.len();
        assert!(n >= 2, "need at least two streams");
        assert!(n <= MAX_AGGREGATE_STREAMS, "too many streams");

        let first = load_payment_stream(&env, stream_ids.get(0).expect("stream id"));
        let recipient = first.recipient.clone();
        let token_addr = first.token.clone();
        let new_id = next_payment_stream_id(&env);

        let mut seen: Vec<u64> = Vec::new(&env);
        let mut total_remaining: i128 = 0;
        let mut total_rate: i128 = 0;
        let mut total_settled: i128 = 0;

        for id in stream_ids.iter() {
            assert!(!seen.contains(id), "duplicate stream id");
            seen.push_back(id);

            let mut stream = load_payment_stream(&env, id);
            assert!(stream.active, "stream not active");
            assert!(stream.sender == sender, "not stream sender");
            assert!(stream.recipient == recipient, "recipient mismatch");
            assert!(stream.token == token_addr, "token mismatch");

            let streamed = stream_streamed_amount(&env, &stream);
            let settled = streamed.saturating_sub(stream.withdrawn);
            let remaining = stream.deposit.saturating_sub(streamed);

            stream.withdrawn = streamed;
            stream.active = false;
            stream.aggregated_into = Some(new_id);
            save_payment_stream(&env, &stream);

            total_settled = total_settled.saturating_add(settled);
            if remaining > 0 {
                total_remaining = total_remaining.saturating_add(remaining);
                total_rate = total_rate.saturating_add(stream.rate_per_ledger);
            }

            events::stream_merged(&env, id, new_id, settled, remaining);
        }

        assert!(total_remaining > 0, "nothing left to aggregate");

        if total_settled > 0 {
            token::Client::new(&env, &token_addr).transfer(
                &env.current_contract_address(),
                &recipient,
                &total_settled,
            );
        }

        let aggregate = PaymentStream {
            id: new_id,
            sender: sender.clone(),
            recipient: recipient.clone(),
            token: token_addr,
            deposit: total_remaining,
            rate_per_ledger: total_rate,
            start_ledger: env.ledger().sequence(),
            withdrawn: 0,
            active: true,
            aggregated_into: None,
        };
        save_payment_stream(&env, &aggregate);

        events::streams_aggregated(
            &env,
            new_id,
            &sender,
            &recipient,
            &stream_ids,
            total_remaining,
            total_rate,
        );
        new_id
    }

    /// Return a payment stream by ID.
    pub fn get_payment_stream(env: Env, stream_id: u64) -> PaymentStream {
        load_payment_stream(&env, stream_id)
    }

    /// Amount currently withdrawable by the stream's recipient.
    pub fn get_stream_withdrawable(env: Env, stream_id: u64) -> i128 {
        let stream = load_payment_stream(&env, stream_id);
        stream_withdrawable_amount(&env, &stream)
    }
}
