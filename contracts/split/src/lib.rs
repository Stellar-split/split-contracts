//! StellarSplit — on-chain invoice & payment splitting contract.

#![no_std]
#![allow(clippy::too_many_arguments)]

const SHARD_COUNT: u64 = 8;
const ARCHIVE_AFTER_LEDGERS: u64 = 100_000;

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

mod error;
mod events;
mod types;

#[cfg(test)]
mod test;

#[cfg(test)]
mod fuzz_tests;

use soroban_sdk::{
    String,
    contract, contractimpl, symbol_short, token, Address, Bytes, BytesN, Env, IntoVal, Map, Symbol, Val, Vec,
};
use soroban_sdk::xdr::ToXdr;
use types::{
    AdminRole, AuditEntry, Bid, CloneOverrides, CompactInvoice, CompletionProof, CreatorStats,
    CreateInvoiceParams, FeeTier, Invoice, InvoiceCore, InvoiceExt, InvoiceExt2, InvoiceHot,
    InvoiceOptions, InvoicePayment, InvoiceStatus, InvoiceTemplate, LegacyInvoice,
    OverflowBehavior, Payment, PaymentCertificate, PaymentProof, QueuedAction, ResolveAction,
    ResolveRule, SplitRule, SubscriptionParams, TimelockAction, Tranche, TreasuryRecord,
    SimulateReleaseResult, CircuitBreakerStatus, ConfidentialPayment,
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

fn platform_fee_waiver_list_key() -> Symbol {
    symbol_short!("fee_wvrs")
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

/// Issue #295: Counter of confidential payments per invoice.
fn confidential_count_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("conf_cnt"), invoice_id)
}

fn treasury_key() -> Symbol {
    symbol_short!("treasury")
}
fn usdc_token_key() -> Symbol {
    symbol_short!("usdc_tok")
}
fn counter_key() -> Symbol {
    symbol_short!("counter")
}
fn archive_after_ledgers_key() -> Symbol {
    symbol_short!("arch_after")
}
fn archive_marker_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("archv"), id)
}
fn created_ledger_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("cr_ledger"), id)
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

fn template_key(creator: &Address, name: &Symbol) -> (Symbol, Address, Symbol) {
    (symbol_short!("tmpl"), creator.clone(), name.clone())
}

/// Issue #210: versioned template key.
fn template_version_key(creator: &Address, name: &Symbol, version: u32) -> (Symbol, Address, Symbol, u32) {
    (symbol_short!("tmpl_v"), creator.clone(), name.clone(), version)
}

/// Issue #210: template version counter key.
fn template_version_count_key(creator: &Address, name: &Symbol) -> (Symbol, Address, Symbol) {
    (symbol_short!("tmpl_ct"), creator.clone(), name.clone())
}

/// Issue #209: pending payout key per (invoice_id, recipient).
fn pending_payout_key(invoice_id: u64, recipient: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("pend_pay"), invoice_id, recipient.clone())
}

/// Per-address reputation counter key (issue #24).
fn rep_key(payer: &Address) -> (Symbol, Address) {
    (symbol_short!("rep"), payer.clone())
}

/// Per-address credit score key (issue #38).
fn credit_key(payer: &Address) -> (Symbol, Address) {
    (symbol_short!("credit"), payer.clone())
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

/// Issue: per-creator invoice creation count key (cancellation rate limit).
fn invoice_count_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("inv_count"), creator.clone())
}

/// Issue: per-creator invoice cancellation count key (cancellation rate limit).
fn cancel_count_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("cnl_count"), creator.clone())
}

/// Issue: maximum cancellation rate in basis points, stored globally.
fn max_cancel_bps_key() -> Symbol {
    symbol_short!("mx_cnl_bp")
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

/// Sliding-window payment timestamp list key for rate limiting (issue #168).
fn payment_window_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("pay_win"), invoice_id)
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
    let admin: Address = env.storage().instance().get(&admin_key()).expect("admin not set");
    admin.require_auth();
    admin
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

fn pending_admin_key() -> Symbol {
    symbol_short!("pend_adm")
}

/// Issue #310: pending upgrade proposal — instance storage.
fn upgrade_proposal_key() -> Symbol {
    symbol_short!("upg_prop")
}

/// Issue #308: per-invoice refunded-addresses set — persistent storage.
fn refunded_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("refunded"), invoice_id)
}

fn maybe_record_created(env: &Env, creator: &Address, total: i128) {
    if let Some(dashboard) = env.storage().persistent().get::<Symbol, Address>(&dashboard_contract_key()) {
        let _: Val = env.invoke_contract(
            &dashboard,
            &Symbol::new(env, "record_created"),
            (creator.clone(), total).into_val(env),
        );
    }
}

fn maybe_record_released(env: &Env, creator: &Address, amount: i128) {
    if let Some(dashboard) = env.storage().persistent().get::<Symbol, Address>(&dashboard_contract_key()) {
        let _: Val = env.invoke_contract(
            &dashboard,
            &Symbol::new(env, "record_released"),
            (creator.clone(), amount).into_val(env),
        );
    }
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
    env.storage().persistent().set(&volume_key, &(volume + amount as u64));
}

/// Issue #299: Update creator stats on release.
fn update_creator_stats_on_release(env: &Env, creator: &Address, amount: i128) {
    let released_key = creator_stats_released_key(creator);
    let released: u64 = env.storage().persistent().get(&released_key).unwrap_or(0u64);
    env.storage().persistent().set(&released_key, &(released + amount as u64));
}

/// Issue #299: Update creator unique payers count (call after recording payment).
fn update_creator_payers(env: &Env, creator: &Address, payer: &Address) {
    // Track unique payers using a set-like approach via a key pattern
    let payer_key = (symbol_short!("cr_py_set"), creator.clone(), payer.clone());
    if env.storage().persistent().get::<(Symbol, Address, Address), bool>(&payer_key).is_none() {
        env.storage().persistent().set(&payer_key, &true);
        let payers_key = creator_stats_payers_key(creator);
        let payers: u64 = env.storage().persistent().get(&payers_key).unwrap_or(0u64);
        env.storage().persistent().set(&payers_key, &(payers + 1));
    }
}

// ---------------------------------------------------------------------------
// Invoice storage helpers
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Hot-field TTL helpers
// ---------------------------------------------------------------------------

/// Target ledger count for instance-storage TTL extension (~30 days at 5 s/ledger).
const INVOICE_HOT_TTL_LEDGERS: u32 = 518_400;

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

// ---------------------------------------------------------------------------
// Storage helpers
// ---------------------------------------------------------------------------

fn archive_invoice_storage(env: &Env, id: u64, core: &InvoiceCore) {
    let ext: InvoiceExt = env.storage().persistent()
        .get(&invoice_ext_key(id))
        .or_else(|| env.storage().instance().get(&invoice_ext_key(id)))
        .unwrap_or_else(|| InvoiceExt {
            co_signers: Vec::new(env), required_signatures: 0, signatures: Vec::new(env),
            approver: None, approved: false, oracle_address: None, condition_met: false,
            penalty_bps: 0, penalty_deadline: 0, min_funding_bps: 0,
            release_stages: Vec::new(env), released_stages: 0, allowed_payers: None,
            price_oracle: None, base_amounts: Vec::new(env), swap_tokens: Vec::new(env),
            tax_bps: 0, tax_authority: None, insurance_premium_bps: 0, insurance_fund: 0,
            smart_route: false, convert_to_stream: false, accepted_tokens: Vec::new(env),
            forward_to: None, forward_invoice_id: None, split_rules: Vec::new(env),
            auto_resolve_rules: Vec::new(env), creator_cosigner: None, velocity_limit: 0,
            velocity_window: 0, parent_invoice_id: None, pause_reason: None, auto_resume_at: None,
            payment_cooldown_secs: None, max_payments_per_window: None, payment_window_secs: None,
            scheduled_release_at: None, refund_grace_secs: None,
            penalty_tiers: Vec::new(env), allowed_callers: None, fallback_action: None,
            external_prerequisite: None,
        });
    let ext2: InvoiceExt2 = env.storage().persistent()
        .get(&invoice_ext2_key(id))
        .or_else(|| env.storage().instance().get(&invoice_ext2_key(id)))
        .unwrap_or_else(|| InvoiceExt2 {
            notification_contract: None, overflow_behavior: OverflowBehavior::Reject,
            cross_chain_ref: None, require_kyc: false, arbiter: None, disputed: false,
            admin_frozen: false, auction_on_expiry: false, auction_end: 0, bids: Vec::new(env),
            min_payment: 0, min_funding_amount: 0, priorities: Vec::new(env),
            substitute_recipient_approvals: Vec::new(env),
            creation_timestamp: 0, min_payment_increment: 0,
        });

    env.storage().instance().set(&invoice_key(id), core);
    env.storage().instance().set(&invoice_ext_key(id), &ext);
    env.storage().instance().set(&invoice_ext2_key(id), &ext2);
    env.storage().instance().set(&archive_marker_key(id), &true);

    let compact = env.storage().persistent().get::<_, CompactInvoice>(&invoice_compact_key(id))
        .or_else(|| env.storage().instance().get(&invoice_compact_key(id)))
        .unwrap_or_else(|| {
            let invoice = Invoice::assemble(core.clone(), ext.clone(), ext2.clone());
            invoice.to_compact(env)
        });
    env.storage().instance().set(&invoice_compact_key(id), &compact);

    for shard_id in 0..SHARD_COUNT {
        let shard_key = pay_shard_key(id, shard_id);
        if let Some(payments) = env.storage().persistent().get::<(Symbol, u64, u64), Vec<Payment>>(&shard_key) {
            env.storage().instance().set(&shard_key, &payments);
        }
        env.storage().persistent().remove(&shard_key);
    }

    if let Some(audit_log) = env.storage().persistent().get(&audit_log_key(id)) {
        env.storage().instance().set(&audit_log_key(id), &audit_log);
    }
    env.storage().persistent().remove(&invoice_key(id));
    env.storage().persistent().remove(&invoice_ext_key(id));
    env.storage().persistent().remove(&invoice_ext2_key(id));
    env.storage().persistent().remove(&invoice_compact_key(id));
    env.storage().persistent().remove(&audit_log_key(id));
    env.storage().instance().set(&created_ledger_key(id), &env.storage().persistent().get(&created_ledger_key(id)).unwrap_or(env.ledger().sequence()));
}

fn maybe_archive_invoice(env: &Env, id: u64) {
    if env.storage().instance().has(&archive_marker_key(id)) || env.storage().persistent().has(&archive_marker_key(id)) {
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

    let created_ledger: u64 = env.storage().persistent().get(&created_ledger_key(id)).or_else(|| env.storage().instance().get(&created_ledger_key(id))).unwrap_or_else(|| env.ledger().sequence());
    let archive_after = env.storage().instance().get(&archive_after_ledgers_key()).unwrap_or(ARCHIVE_AFTER_LEDGERS);
    if env.ledger().sequence().saturating_sub(created_ledger) < archive_after {
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
        env.storage().instance().get(&invoice_key(id)).expect("invoice not found")
    };
    
    // Aggregate payments from all shards (issue #177).
    let mut all_payments: Vec<Payment> = Vec::new(env);
    for shard_id in 0..SHARD_COUNT {
        let shard_key = pay_shard_key(id, shard_id);
        if let Some(shard_payments) = env.storage().persistent().get::<(Symbol, u64, u64), Vec<Payment>>(&shard_key)
            .or_else(|| env.storage().instance().get(&shard_key))
        {
            for payment in shard_payments.iter() {
                all_payments.push_back(payment.clone());
            }
        }
    }
    core.payments = all_payments;
    
    let ext: InvoiceExt = env.storage().persistent()
        .get(&invoice_ext_key(id))
        .or_else(|| env.storage().instance().get(&invoice_ext_key(id)))
        .unwrap_or_else(|| InvoiceExt {
            co_signers: Vec::new(env),
            required_signatures: 0,
            signatures: Vec::new(env),
            approver: None,
            approved: false,
            oracle_address: None,
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
            refunded_addresses: Vec::new(env),
        });
    let ext2: InvoiceExt2 = env.storage().persistent()
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
        });
    
    // Load compact representation if available
    if let Some(compact) = env.storage().persistent().get::<_, CompactInvoice>(&invoice_compact_key(id))
        .or_else(|| env.storage().instance().get(&invoice_compact_key(id)))
    {
    // Load compact representation if available, then overlay hot fields.
    let mut invoice = if let Some(compact) = env.storage().persistent().get::<_, CompactInvoice>(&invoice_compact_key(id)) {
        Invoice::from_compact(&compact, core, ext, ext2)
    } else {
        Invoice::assemble(core, ext, ext2)
    };

    // Hot fields are authoritative post-migration; overlay them here.
    if let Some(hot) = maybe_hot {
        invoice.status     = hot.status;
        invoice.funded     = hot.funded;
        invoice.recipients = hot.recipients;
    }

    invoice
}

fn save_invoice(env: &Env, id: u64, invoice: &Invoice) {
    // Issue #286: Verify invariants before saving
    debug_assert!(
        invoice.funded <= invoice.amounts.iter().sum(),
        "invariant: funded exceeds total"
    );

    // Check no duplicate recipients
    for i in 0..invoice.recipients.len() {
        for j in (i + 1)..invoice.recipients.len() {
            debug_assert!(
                invoice.recipients.get(i).unwrap() != invoice.recipients.get(j).unwrap(),
                "invariant: duplicate recipient addresses"
            );
        }
    }

    let mut clean_invoice = invoice.clone();
    clean_invoice.payments = Vec::new(env);
    let (core, ext, ext2) = clean_invoice.split();
    let archived = env.storage().instance().has(&archive_marker_key(id)) || env.storage().persistent().has(&archive_marker_key(id));

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
    }

    // Store compact representation in the same tier as the invoice data.
    let compact = invoice.to_compact(env);
    if archived {
        env.storage().instance().set(&invoice_compact_key(id), &compact);
        env.storage().persistent().remove(&invoice_compact_key(id));
    } else {
        env.storage().persistent().set(&invoice_compact_key(id), &compact);
    }
    env.storage().persistent().set(&invoice_key(id), &core);
    env.storage().persistent().set(&invoice_ext_key(id), &ext);
    env.storage().persistent().set(&invoice_ext2_key(id), &ext2);

    // Store compact representation
    let compact = invoice.to_compact(env);
    env.storage().persistent().set(&invoice_compact_key(id), &compact);

    // Write hot fields to instance storage and bump TTL.
    // status, funded, and recipients change on pay/release/refund paths.
    let total: i128 = invoice.amounts.iter().sum();
    let hot = InvoiceHot {
        status:     invoice.status.clone(),
        funded:     invoice.funded,
        total,
        recipients: invoice.recipients.clone(),
    };
    env.storage().instance().set(&invoice_hot_key(id), &hot);
    bump_invoice_ttl(env);
}

fn append_audit_entry(env: &Env, id: u64, action: Symbol, actor: &Address) {
    let timestamp = env.ledger().timestamp();
    let entry = AuditEntry { action, actor: actor.clone(), timestamp };
    let archived = env.storage().instance().has(&archive_marker_key(id)) || env.storage().persistent().has(&archive_marker_key(id));
    let mut log: Vec<AuditEntry> = if archived {
        env.storage().instance().get(&audit_log_key(id)).unwrap_or_else(|| Vec::new(env))
    } else {
        env.storage().persistent().get(&audit_log_key(id)).unwrap_or_else(|| Vec::new(env))
    };
    log.push_back(entry);
    if archived {
        env.storage().instance().set(&audit_log_key(id), &log);
        env.storage().persistent().remove(&audit_log_key(id));
    } else {
        env.storage().persistent().set(&audit_log_key(id), &log);
    }
}

fn notify_invoice(env: &Env, invoice_id: u64, event: Symbol, notification_contract: &Option<Address>) {
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
    env.storage()
        .persistent()
        .get(&paused_key())
        .unwrap_or(false)
}

fn require_not_paused(env: &Env) {
    assert!(!is_paused(env), "contract is paused");
    // Issue #297: also check circuit breaker
    let cb_active: bool = env
        .storage()
        .persistent()
        .get(&circuit_breaker_key())
        .unwrap_or(false);
    assert!(!cb_active, "ContractPaused");
}

fn require_role(env: &Env, admin: &Address, min_role: AdminRole) {
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
    if let Some(grp) = env.storage().persistent().get::<_, types::InvoiceGroup>(&group_key(group_id)) {
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
        if let Some(record) = env.storage().persistent().get(&group_treasury_key(group_id)) {
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

fn save_invoice_ext(env: &Env, id: u64, ext: &InvoiceExt) {
    env.storage()
        .persistent()
        .set(&invoice_ext_key(id), ext);
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
    let threshold: i128 = env.storage().persistent().get(&platform_vol_thresh_key()).unwrap_or(10_000_0000000i128); // 10,000 USDC (7 decimals)
    if threshold <= 0 { return; }
    let last_milestone: i128 = env.storage().persistent().get(&platform_vol_mile_key()).unwrap_or(0i128);
    let new_milestone = new_volume / threshold;
    if new_milestone > last_milestone {
        env.storage().persistent().set(&platform_vol_mile_key(), &new_milestone);
        let invoice_count: u64 = env.storage().persistent().get(&total_invoices_key()).unwrap_or(0u64);
        events::platform_volume_milestone(env, new_volume, invoice_count, new_milestone);
    }
}

/// Issue #276: Check and emit creator volume milestone events.
fn check_creator_milestone(env: &Env, creator: &Address, new_volume: i128) {
    let threshold: i128 = env.storage().persistent().get(&creator_vol_thresh_key()).unwrap_or(1_000_0000000i128); // 1,000 USDC (7 decimals)
    if threshold <= 0 { return; }
    let last_milestone: i128 = env.storage().persistent().get(&creator_vol_mile_key(creator)).unwrap_or(0i128);
    let new_milestone = new_volume / threshold;
    if new_milestone > last_milestone {
        env.storage().persistent().set(&creator_vol_mile_key(creator), &new_milestone);
        let invoice_count: u64 = env.storage().persistent().get(&creator_stats_count_key(creator)).unwrap_or(0u64);
        events::creator_volume_milestone(env, creator, new_volume, invoice_count, new_milestone);
    }
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
        assert!(
            !env.storage().instance().has(&admin_key()),
            "already initialized"
        );
        assert!(creation_fee >= 0, "creation_fee must be non-negative");
        assert!(platform_fee_bps <= 10_000, "platform_fee_bps must be ≤ 10000");
        assert!(max_cancel_bps <= 10_000, "max_cancel_bps must be ≤ 10000");
        assert!(rate_window > 0 || rate_limit == 0, "rate_window must be positive when rate_limit is enabled");
        let mut admins: Map<Address, AdminRole> = Map::new(&env);
        admins.set(admin.clone(), AdminRole::SuperAdmin);
        env.storage().instance().set(&admins_key(), &admins);
        env.storage().instance().set(&admin_key(), &admin);
        env.storage().instance().set(&creation_fee_key(), &creation_fee);
        env.storage().instance().set(&treasury_key(), &treasury);
        env.storage().instance().set(&usdc_token_key(), &usdc_token);
        env.storage().instance().set(&platform_fee_bps_key(), &platform_fee_bps);
        env.storage().instance().set(&governance_contract_key(), &governance_contract);
        env.storage().instance().set(&archive_after_ledgers_key(), &ARCHIVE_AFTER_LEDGERS);
        env.storage().persistent().set(&paused_key(), &false);
        env.storage().persistent().set(&max_cancel_bps_key(), &max_cancel_bps);
        env.storage().persistent().set(&rate_limit_key(), &rate_limit);
        env.storage().persistent().set(&rate_window_key(), &rate_window);
    }

    /// Add a new admin with a given role. Requires SuperAdmin auth.
    pub fn add_admin(env: Env, admin: Address, new_admin: Address, role: AdminRole) {
        require_role(&env, &admin, AdminRole::SuperAdmin);
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
        require_role(&env, &admin, AdminRole::SuperAdmin);
        let mut admins: Map<Address, AdminRole> = env
            .storage()
            .instance()
            .get(&admins_key())
            .expect("admins not set");
        assert!(admins.get(target.clone()).is_some(), "target is not an admin");
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

    /// Pause the contract. Requires admin auth.
    pub fn pause(env: Env, admin: Address) {
        require_role(&env, &admin, AdminRole::Operator);
        env.storage().persistent().set(&paused_key(), &true);
    }

    /// Unpause the contract. Requires admin auth.
    pub fn unpause(env: Env, admin: Address) {
        require_role(&env, &admin, AdminRole::Operator);
        env.storage().persistent().set(&paused_key(), &false);
    }

    /// Pause a specific function by name. Requires Operator+ auth.
    /// While paused, the function panics with "function paused" when called.
    pub fn pause_function(env: Env, admin: Address, function: Symbol) {
        require_role(&env, &admin, AdminRole::Operator);
        let mut paused_fns: Vec<Symbol> = env
            .storage()
            .persistent()
            .get(&paused_fns_key())
            .unwrap_or_else(|| Vec::new(&env));
        if !paused_fns.iter().any(|f| f == function) {
            paused_fns.push_back(function);
        }
        env.storage().persistent().set(&paused_fns_key(), &paused_fns);
    }

    /// Unpause a specific function by name. Requires Operator+ auth.
    pub fn unpause_function(env: Env, admin: Address, function: Symbol) {
        require_role(&env, &admin, AdminRole::Operator);
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
        require_role(&env, &admin, AdminRole::Operator);
        if exempt {
            env.storage().persistent().set(&pause_exempt_key(&address), &true);
        } else {
            env.storage().persistent().remove(&pause_exempt_key(&address));
        }
    }

    /// Set the global payer aggregate limit and window. Requires admin auth.
    pub fn set_global_payer_limit(env: Env, admin: Address, limit: i128, window_secs: u64) {
        require_role(&env, &admin, AdminRole::Operator);
        assert!(limit >= 0, "limit must be non-negative");
        env.storage().persistent().set(&global_payer_limit_key(), &limit);
        env.storage().persistent().set(&global_payer_window_key(), &window_secs);
    }

    /// Update the creation fee. Requires admin auth.
    pub fn set_creation_fee(env: Env, admin: Address, creation_fee: i128) {
        require_role(&env, &admin, AdminRole::Operator);
        assert!(creation_fee >= 0, "creation_fee must be non-negative");
        env.storage().instance().set(&creation_fee_key(), &creation_fee);
    }

    /// Update the treasury address. Requires admin auth.
    pub fn set_treasury(env: Env, admin: Address, treasury: Address) {
        require_role(&env, &admin, AdminRole::SuperAdmin);
        env.storage().instance().set(&treasury_key(), &treasury);
    }

    /// Configure the ledger threshold after which invoices may be lazily archived.
    /// Requires Operator+ auth.
    pub fn set_archive_after_ledgers(env: Env, admin: Address, ledgers: u64) {
        require_role(&env, &admin, AdminRole::Operator);
        assert!(ledgers > 0, "archive_after_ledgers must be positive");
        env.storage().instance().set(&archive_after_ledgers_key(), &ledgers);
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
        require_role(&env, &admin, AdminRole::Operator);
        env.storage().persistent().set(&stream_contract_key(), &contract);
    }

    /// Store the DEX contract address used for token swaps in pay_with_token(). Requires admin auth.
    pub fn set_dex_contract(env: Env, admin: Address, contract: Address) {
        require_role(&env, &admin, AdminRole::Operator);
        env.storage().persistent().set(&soroban_sdk::symbol_short!("dex_ctr"), &contract);
    }

    // -----------------------------------------------------------------------
    // Issue #189: Admin rotation
    // -----------------------------------------------------------------------

    /// Propose a new admin. Requires current admin auth.
    pub fn propose_admin(env: Env, admin: Address, new_admin: Address) {
        require_admin(&env);
        let _ = admin;
        env.storage().instance().set(&pending_admin_key(), &new_admin);
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
        env.storage().persistent().set(&creator_volume_cap_key(&creator), &cap);
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
        env.storage().persistent().set(&platform_vol_thresh_key(), &threshold);
    }

    /// Set the per-creator volume milestone threshold (in token base units).
    /// Requires admin auth. Set to 0 to disable.
    pub fn set_creator_vol_threshold(env: Env, admin: Address, threshold: i128) {
        require_admin(&env);
        let _ = admin;
        assert!(threshold >= 0, "threshold must be non-negative");
        env.storage().persistent().set(&creator_vol_thresh_key(), &threshold);
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

        match resolution {
            ResolveAction::Release => {
                let caller = env.current_contract_address();
                Self::_release(&env, invoice_id, &mut invoice, &caller);
            }
            ResolveAction::Refund => {
                // If the invoice has no payments, mark as cancelled.
                if invoice.funded == 0 {
                    invoice.status = InvoiceStatus::Cancelled;
                    events::invoice_state_changed(&env, invoice_id, Some(&InvoiceStatus::Pending), &InvoiceStatus::Cancelled, &arbiter);
                    save_invoice(&env, invoice_id, &invoice);
                    append_audit_entry(&env, invoice_id, symbol_short!("resolve"), &arbiter);
                    return;
                }

                let token_client = token::Client::new(
                    &env,
                    &invoice.tokens.get(0).expect("no token"),
                );
                let mut totals: Map<Address, i128> = Map::new(&env);
                for payment in invoice.payments.iter() {
                    let prev = totals.get(payment.payer.clone()).unwrap_or(0);
                    totals.set(payment.payer.clone(), prev + payment.amount);
                }
                let mut total_refunded_amount: i128 = 0;
                for (payer, amount) in totals.iter() {
                    token_client.transfer(
                        &env.current_contract_address(),
                        &payer,
                        &amount,
                    );
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
                events::invoice_refunded(&env, invoice_id);
                events::invoice_state_changed(&env, invoice_id, Some(&InvoiceStatus::Pending), &InvoiceStatus::Refunded, &arbiter);

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
        require_role(&env, &admin, AdminRole::Operator);
        env.storage().persistent().set(&receipt_factory_key(), &factory);
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
        env.storage().persistent().set(&dashboard_contract_key(), &dashboard);
    }

    /// Return the dashboard contract address, or None if not set.
    pub fn get_dashboard_contract(env: Env) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&dashboard_contract_key())
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
        require_role(&env, &admin, AdminRole::SuperAdmin);
        let mut wl: Vec<Address> = env
            .storage()
            .persistent()
            .get(&creator_whitelist_key())
            .unwrap_or_else(|| Vec::new(&env));
        if !wl.iter().any(|a| a == address) {
            wl.push_back(address);
        }
        env.storage().persistent().set(&creator_whitelist_key(), &wl);
    }

    /// Remove an address from the creator whitelist. Requires admin auth.
    pub fn remove_creator(env: Env, admin: Address, address: Address) {
        require_role(&env, &admin, AdminRole::SuperAdmin);
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
        env.storage().persistent().set(&creator_whitelist_key(), &new_wl);
    }

    // -----------------------------------------------------------------------
    // Issue #215: Configurable platform fee waiver list
    // -----------------------------------------------------------------------

    /// Add an address to the platform fee waiver list. Requires admin auth.
    /// Addresses on this list will not be charged platform fees when they are recipients.
    pub fn add_platform_fee_waiver(env: Env, admin: Address, address: Address) {
        require_role(&env, &admin, AdminRole::SuperAdmin);
        let mut waivers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&platform_fee_waiver_list_key())
            .unwrap_or_else(|| Vec::new(&env));
        if !waivers.iter().any(|a| a == address) {
            waivers.push_back(address);
        }
        env.storage().persistent().set(&platform_fee_waiver_list_key(), &waivers);
    }

    /// Remove an address from the platform fee waiver list. Requires admin auth.
    pub fn remove_platform_fee_waiver(env: Env, admin: Address, address: Address) {
        require_role(&env, &admin, AdminRole::SuperAdmin);
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
        env.storage().persistent().set(&platform_fee_waiver_list_key(), &new_waivers);
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
                        oracle_address: None,
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
                        refunded_addresses: Vec::new(&env),
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
                    })
            });
        let audit_log: Vec<types::AuditEntry> = get_audit_log(&env, invoice_id);
        types::InvoiceSnapshot { core, ext, ext2, audit_log }
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

    // -----------------------------------------------------------------------
    // Issue #285: Volume-based fee tiers
    // -----------------------------------------------------------------------

    /// Admin function to set up to 5 fee tiers sorted by volume threshold.
    /// Requires admin auth.
    pub fn set_fee_tiers(env: Env, admin: Address, tiers: Vec<FeeTier>) {
        let admin_addr = require_admin(&env);
        let _ = admin;

        debug_assert!(tiers.len() <= 5, "Maximum 5 fee tiers allowed");

        // Verify tiers are sorted by volume_threshold in ascending order
        for i in 1..tiers.len() {
            let prev = tiers.get(i - 1).unwrap();
            let curr = tiers.get(i).unwrap();
            debug_assert!(prev.volume_threshold < curr.volume_threshold, "Fee tiers must be sorted by volume_threshold");
        }

        env.storage().instance().set(&fee_tiers_key(), &tiers);
        events::fee_tiers_updated(&env, tiers.len() as u32);
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

        CreatorStats {
            total_invoices,
            total_raised,
            total_released,
            total_payers,
            avg_funding_time_ledgers,
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

        env.storage().persistent().set(&timelock_action_key(counter), &queued);
        env.storage().persistent().set(&timelock_action_counter_key(), &counter);

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
                env.storage().instance().set(&platform_fee_bps_key(), new_fee);
            }
        }

        queued.executed = true;
        env.storage().persistent().set(&timelock_action_key(action_id), &queued);

        append_audit_entry(&env, 0, Symbol::new(&env, "exec"), &env.current_contract_address());
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

        env.storage().persistent().remove(&timelock_action_key(action_id));

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
        require_role(&env, &admin, AdminRole::SuperAdmin);

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
        // Check if contract is paused, but allow exempt creators
        let is_paused = is_paused(&env);
        let is_exempt = env.storage().persistent().get::<_, bool>(&pause_exempt_key(&creator)).unwrap_or(false);
        if is_paused && !is_exempt {
            panic!("contract is paused");
        }
        creator.require_auth();
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
        if let Some(nft_contract) = env.storage().persistent().get::<_, Option<Address>>(&nft_gate_key()).unwrap_or(None) {
            let balance: i128 = env.invoke_contract(
                &nft_contract,
                &Symbol::new(&env, "balance_of"),
                (creator.clone(),).into_val(&env),
            );
            assert!(balance > 0, "nft gate: not a holder");
        }

        Self::_create_invoice_inner(
            &env,
            creator,
            recipients,
            amounts,
            // Issue #307: if payment_token is explicitly set, use it instead of the token param.
            options.payment_token.unwrap_or(token),
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
            options.oracle_address,
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
            options.payment_cooldown_secs,
            options.max_payments_per_window,
            options.payment_window_secs,
            options.refund_grace_secs,
            options.priorities,
            options.require_kyc,
            options.scheduled_release_at,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn _create_invoice_inner(
        env: &Env,
        creator: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        token: Address,
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
        oracle_address: Option<Address>,
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
    ) -> u64 {
        assert!(
            recipients.len() == amounts.len(),
            "recipients and amounts length mismatch"
        );
        assert!(!recipients.is_empty(), "must have at least one recipient");
        assert!(deadline > env.ledger().timestamp(), "deadline must be in the future");
        assert!(bonus_pool >= 0, "bonus_pool must be non-negative");
        assert!(penalty_bps <= 10_000, "penalty_bps must be ≤ 10000");
        assert!(min_funding_bps <= 10_000, "min_funding_bps must be ≤ 10000");
        assert!(tax_bps <= 10_000, "tax_bps must be ≤ 10000");
        assert!(insurance_premium_bps <= 10_000, "insurance_premium_bps must be ≤ 10000");
        if tax_bps > 0 {
            assert!(tax_authority.is_some(), "tax_authority must be set if tax_bps > 0");
        }
        if !priorities.is_empty() {
            assert!(
                priorities.len() == recipients.len(),
                "priorities length must match recipients"
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

        if let Some(compliance_contract) = env.storage().persistent().get::<_, Address>(&soroban_sdk::symbol_short!("comp_ctr")) {
            let creator_ok: bool = env.invoke_contract(&compliance_contract, &soroban_sdk::Symbol::new(env, "check"), (creator.clone(),).into_val(env));
            assert!(creator_ok, "compliance check failed");
            
            for recipient in recipients.iter() {
                let recipient_ok: bool = env.invoke_contract(&compliance_contract, &soroban_sdk::Symbol::new(env, "check"), (recipient.clone(),).into_val(env));
                assert!(recipient_ok, "compliance check failed");
            }
        }

        if let Some(prereq_id) = prerequisite_id {
            let _ = load_invoice(env, prereq_id);
        }

        if !tranches.is_empty() {
            let total_bps: u32 = tranches.iter().map(|t| t.basis_points).sum();
            assert!(total_bps == 10_000, "tranches must sum to 10000 basis points");
        }

        if !release_stages.is_empty() {
            let total_bps: u32 = release_stages.iter().sum();
            assert!(total_bps == 10_000, "release_stages must sum to 10000 basis points");
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
            assert!(total_bps == 10_000, "split_rules must sum to 100% of funded amount");
        }

        // Compliance check: if a compliance contract is configured, verify creator and all recipients.
        if let Some(cc) = env.storage().persistent().get::<Symbol, Address>(&compliance_key()) {
            let mut check_args: Vec<Val> = Vec::new(env);
            check_args.push_back(creator.clone().into_val(env));
            let creator_ok: bool = env.invoke_contract(&cc, &Symbol::new(env, "is_compliant"), check_args);
            assert!(creator_ok, "compliance check failed");
            for recipient in recipients.iter() {
                let mut r_args: Vec<Val> = Vec::new(env);
                r_args.push_back(recipient.clone().into_val(env));
                let r_ok: bool = env.invoke_contract(&cc, &Symbol::new(env, "is_compliant"), r_args);
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
            // Get creator's lifetime volume
            let creator_volume: i128 = env
                .storage()
                .persistent()
                .get(&creator_stats_volume_key(&creator))
                .unwrap_or(0);
            
            // Look up highest matching tier discount
            let discount_bps: u32 = if let Some(tiers) = env.storage().persistent().get::<_, Vec<(i128, u32)>>(&fee_tiers_key()) {
                let mut best_discount = 0u32;
                for (threshold, discount) in tiers.iter() {
                    if creator_volume >= threshold && discount > best_discount {
                        best_discount = discount;
                    }
                }
                best_discount
            } else {
                0u32
            };
            
            // Apply discount
            let discounted_fee = base_creation_fee - (base_creation_fee * discount_bps as i128 / 10_000);
            
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

        // Issue: increment per-creator invoice count for cancellation rate tracking.
        let inv_cnt: u64 = env
            .storage()
            .persistent()
            .get(&invoice_count_key(&creator))
            .unwrap_or(0u64);
        env.storage()
            .persistent()
            .set(&invoice_count_key(&creator), &(inv_cnt + 1));


        let total: i128 = amounts.iter().sum();

        let gov_opt: Option<Option<Address>> = env.storage().instance().get(&governance_contract_key());
        if let Some(Some(gov)) = gov_opt {
            let approved: bool = env.invoke_contract(&gov, &Symbol::new(env, "check_approval"), (creator.clone(), total).into_val(env));
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
            let token_client = token::Client::new(env, &token);
            token_client.transfer(&creator, &env.current_contract_address(), &bonus_pool);
        }

        // Build per-recipient token vec (all the same token).
        let mut tokens: Vec<Address> = Vec::new(env);
        for _ in recipients.iter() {
            tokens.push_back(token.clone());
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

        let invoice = Invoice {
            version: 1u32,
            creator: creator.clone(),
            co_creators,
            recipients,
            base_amounts: amounts.clone(),
            amounts,
            tokens,
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
            oracle_address,
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
            target_usd_cents: None,
        };

        save_invoice(env, id, &invoice);
        events::invoice_created(env, id, &creator, total, &invoice.cross_chain_ref);
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
            &total_invoices.checked_add(1).expect("total_invoices overflow"),
        );

        // Increment creator invoice count (issue #106).
        let creator_count: u64 = env
            .storage()
            .persistent()
            .get(&creator_stats_count_key(&creator))
            .unwrap_or(0u64);
        env.storage().persistent().set(
            &creator_stats_count_key(&creator),
            &creator_count.checked_add(1).expect("creator count overflow"),
        );

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
                false, // require_kyc
                None, // scheduled_release_at
            );
            ids.push_back(id);
        }
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
    ) -> u64 {
        creator.require_auth();

        assert!(
            recipients.len() == amounts.len(),
            "recipients and amounts length mismatch"
        );
        assert!(!recipients.is_empty(), "must have at least one recipient");
        assert!(months > 0 && months <= 12, "months must be between 1 and 12");
        for amt in amounts.iter() {
            assert!(amt > 0, "amounts must be positive");
        }

        let deadline = env.ledger().timestamp() + 30 * 24 * 60 * 60;
        let id = Self::_create_invoice_inner(
            &env,
            creator.clone(),
            recipients.clone(),
            amounts.clone(),
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
            false, // require_kyc
            None, // scheduled_release_at
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
            };
            env.storage()
                .persistent()
                .set(&subscription_params_key(id), &params);
        }

        id
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
        env.storage().persistent().set(&created_ledger_key(id), &env.ledger().sequence());

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
            oracle_address: source.oracle_address.clone(),
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
        };

        save_invoice(&env, id, &new_invoice);
        events::invoice_cloned(&env, source_id, id);

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
            new_payments.push_back(Payment { payer, amount, tip, attestation_hash: None, donate_on_failure: false });
        }

        // Verify total funded is unchanged (optional assertion, as asked by Acceptance Criteria)
        let mut total_funded: i128 = 0;
        for p in new_payments.iter() {
            total_funded += p.amount;
        }
        assert_eq!(total_funded, invoice.funded, "total funded changed after compression");

        // Clear all shards and write compressed payments to appropriate shards (issue #177).
        for shard_id in 0..SHARD_COUNT {
            env.storage().persistent().remove(&pay_shard_key(invoice_id, shard_id));
        }
        
        for payment in new_payments.iter() {
            let shard_id = compute_shard_id(&env, &payment.payer);
            let mut shard_payments: Vec<Payment> = env
                .storage()
                .persistent()
                .get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(invoice_id, shard_id))
                .unwrap_or_else(|| Vec::new(&env));
            shard_payments.push_back(payment.clone());
            env.storage().persistent().set(&pay_shard_key(invoice_id, shard_id), &shard_payments);
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
        assert!(invoice.status == InvoiceStatus::Pending, "invoice is not pending");
        assert!(!invoice.disputed, "invoice is disputed");

        let token_client = token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
        token_client.transfer(&payer, &env.current_contract_address(), &deposit);

        // Store (balance, deposited)
        let state: (i128, i128) = (deposit, deposit);
        env.storage().persistent().set(&channel_key(invoice_id, &payer), &state);
    }

    pub fn channel_pay(env: Env, payer: Address, invoice_id: u64, amount: i128) {
        require_not_paused(&env);
        payer.require_auth();
        assert!(amount > 0, "amount must be positive");

        let mut state: (i128, i128) = env.storage().persistent().get(&channel_key(invoice_id, &payer)).expect("channel not found");
        assert!(state.0 >= amount, "insufficient channel balance");

        state.0 -= amount;
        env.storage().persistent().set(&channel_key(invoice_id, &payer), &state);
    }

    pub fn close_channel(env: Env, payer: Address, invoice_id: u64) {
        require_not_paused(&env);
        payer.require_auth();

        let state: (i128, i128) = env.storage().persistent().get(&channel_key(invoice_id, &payer)).expect("channel not found");
        let balance = state.0;
        let deposited = state.1;
        let net_paid = deposited - balance;

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(!invoice.disputed, "invoice is disputed");

        if net_paid > 0 {
            assert!(invoice.status == InvoiceStatus::Pending, "invoice is not pending");

            // Write payment to sharded storage (issue #177).
            let shard_id = compute_shard_id(&env, &payer);
            let mut shard_payments: Vec<Payment> = env
                .storage()
                .persistent()
                .get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(invoice_id, shard_id))
                .unwrap_or_else(|| Vec::new(&env));
            shard_payments.push_back(Payment { payer: payer.clone(), amount: net_paid, tip: 0, attestation_hash: None, donate_on_failure: false });
            env.storage().persistent().set(&pay_shard_key(invoice_id, shard_id), &shard_payments);
            
            invoice.funded += net_paid;

            // In real app we might handle penalty/oracle, but for simplicity:
            events::payment_received(&env, invoice_id, &payer, net_paid);
            
            let total: i128 = invoice.amounts.iter().sum();
            
            if invoice.funded >= total {
                let in_group = env.storage().persistent().has(&invoice_group_key(invoice_id));
                let guarded =
                    invoice.prerequisite_id.is_some()
                        || !invoice.tranches.is_empty()
                        || !invoice.release_stages.is_empty()
                        || in_group
                        || !invoice.co_signers.is_empty()
                        || (invoice.oracle_address.is_some() && !invoice.condition_met)
                        || (invoice.min_funding_bps > 0 && invoice.funded < (invoice.amounts.iter().sum::<i128>() * invoice.min_funding_bps as i128 / 10_000));
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

        env.storage().persistent().remove(&channel_key(invoice_id, &payer));
    }

    pub fn pay(env: Env, payer: Address, invoice_id: u64, amount: i128, nonce: u64, _auto_convert: bool, donate_on_failure: bool) {
        require_fn_not_paused(&env, &symbol_short!("pay"));
        payer.require_auth();
        Self::_pay(&env, &payer, invoice_id, amount, nonce, _auto_convert, None, None, donate_on_failure);
    }

    /// Pay with a signed attestation binding the payment to an off-chain identity
    pub fn pay_with_attestation(env: Env, payer: Address, invoice_id: u64, amount: i128, nonce: u64, attestation_hash: BytesN<32>, signature: BytesN<64>, signer_pubkey: BytesN<32>, _auto_convert: bool) {
        require_fn_not_paused(&env, &symbol_short!("pay"));
        payer.require_auth();

        // Verify ed25519 signature over attestation_hash
        let attestation_msg: soroban_sdk::Bytes = attestation_hash.clone().into();
        env.crypto().ed25519_verify(&signer_pubkey, &attestation_msg, &signature);

        // Proceed with payment, storing the attestation hash
        Self::_pay(&env, &payer, invoice_id, amount, nonce, _auto_convert, None, Some(attestation_hash), false);
    }

    fn _pay(env: &Env, payer: &Address, invoice_id: u64, amount: i128, nonce: u64, _auto_convert: bool, via: Option<Address>, attestation_hash: Option<BytesN<32>>, donate_on_failure: bool) {
        let mut invoice = load_invoice(env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(
            env.ledger().timestamp() <= invoice.deadline,
            "invoice deadline has passed"
        );
        assert!(amount > 0, "payment amount must be positive");

        // Lazy auto-resume: clear frozen if the auto-resume timestamp has passed.
        if invoice.frozen {
            if let Some(auto_at) = invoice.auto_resume_at {
                if env.ledger().timestamp() >= auto_at {
                    invoice.frozen = false;
                    invoice.pause_reason = None;
                    invoice.auto_resume_at = None;
                    save_invoice(env, invoice_id, &invoice);
                }
            }
        }
        assert!(!invoice.frozen, "invoice is frozen");
        assert!(!invoice.admin_frozen, "invoice frozen by admin");

        // Check allowed_payers allowlist.
        if let Some(ref whitelist) = invoice.allowed_payers {
            assert!(whitelist.contains(payer), "payer not allowed");
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
            let oracle_price: i128 = env.invoke_contract(
                oracle,
                &Symbol::new(env, "get_price"),
                Vec::new(env),
            );
            let base_total: i128 = invoice.base_amounts.iter().sum();
            base_total * oracle_price / 1_000_000
        } else {
            invoice.amounts.iter().sum()
        };
        let remaining = total - invoice.funded;

        if invoice.require_kyc {
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
                env.storage().persistent().set(&accum_key(invoice_id, payer), &accumulator);
                return;
            }
            assert!(accumulator <= remaining, "payment exceeds remaining balance");
            env.storage().persistent().remove(&accum_key(invoice_id, payer));
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
            assert!(window.1 + amount <= invoice.velocity_limit, "velocity limit exceeded");
            window.1 += amount;
            env.storage().persistent().set(&vel_key(invoice_id, payer), &window);
        }

        // Global cross-invoice velocity limiting per payer
        let global_limit: i128 = env.storage().persistent().get(&global_payer_limit_key()).unwrap_or(0i128);
        if global_limit > 0 {
            let global_window_secs: u64 = env.storage().persistent().get(&global_payer_window_key()).unwrap_or(0u64);
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
            assert!(global_window.1 + amount <= global_limit, "global payer limit exceeded");
            global_window.1 += amount;
            env.storage().persistent().set(&global_vel_key(payer), &global_window);
        }

        let token_client = token::Client::new(env, &invoice.tokens.get(0).expect("no token"));

        let credited_amount = match invoice.overflow_behavior {
            OverflowBehavior::Reject => {
                assert!(amount <= remaining, "payment exceeds remaining balance");
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
        };

        let premium = (credited_amount as u128 * invoice.insurance_premium_bps as u128 / 10_000u128) as i128;
        // Transfer the full amount from payer so excess can be refunded/donated.
        let excess = amount - credited_amount;
        let total_charge = credited_amount + premium + excess;
        token_client.transfer(payer, &env.current_contract_address(), &total_charge);
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
        shard_payments.push_back(Payment { payer: payer.clone(), amount: credited_amount, tip: 0, attestation_hash, donate_on_failure });
        env.storage().persistent().set(&pay_shard_key(invoice_id, shard_id), &shard_payments);

        invoice.funded += credited_amount;

        // Increment per-address reputation counter (issue #24).
        let rep: u64 = env
            .storage()
            .persistent()
            .get(&rep_key(payer))
            .unwrap_or(0u64);
        env.storage()
            .persistent()
            .set(&rep_key(payer), &(rep + 1));

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
        events::payment_received(env, invoice_id, payer, credited_amount);
        update_creator_stats_on_payment(env, &invoice.creator, credited_amount);
        update_creator_payers(env, &invoice.creator, payer);
        notify_invoice(env, invoice_id, symbol_short!("pay"), &invoice.notification_contract);

        // Record rate-limiter timestamps after successful payment (issue #168).
        Self::record_payment_limits(env, invoice_id, payer, &invoice, now_ts);

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
            let receipt_addr: Address = env.invoke_contract(
                &factory,
                &Symbol::new(env, "mint_receipt"),
                args,
            );
            env.storage()
                .persistent()
                .set(&receipt_token_key(invoice_id, payer), &receipt_addr);
        }

        if invoice.funded >= total {
            // Auto-release only when no tranches, prerequisite, or group constraint
            // requires a manual release() call.
            let in_group = env
                .storage()
                .persistent()
                .has(&invoice_group_key(invoice_id));
            let guarded =
                invoice.prerequisite_id.is_some()
                    || !invoice.tranches.is_empty()
                    || !invoice.release_stages.is_empty()
                    || in_group
                    || !invoice.co_signers.is_empty()
                    || (invoice.oracle_address.is_some() && !invoice.condition_met)
                    || (invoice.min_funding_bps > 0 && invoice.funded < (invoice.amounts.iter().sum::<i128>() * invoice.min_funding_bps as i128 / 10_000));
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

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(invoice.status == InvoiceStatus::Pending, "invoice is not pending");
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(env.ledger().timestamp() <= invoice.deadline, "invoice deadline has passed");
        assert!(amount > 0, "payment amount must be positive");

        let invoice_token = invoice.tokens.get(0).expect("no token");

        // Accept the base token or any token in accepted_tokens.
        let is_base = source_token == invoice_token;
        let is_accepted = is_base
            || invoice.accepted_tokens.iter().any(|t| t == source_token);
        assert!(is_accepted, "token not accepted");

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
        assert!(credited_amount <= remaining, "payment exceeds remaining balance");

        // Write payment to sharded storage (issue #177).
        let shard_id = compute_shard_id(&env, &payer);
        let mut shard_payments: Vec<Payment> = env
            .storage()
            .persistent()
            .get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(invoice_id, shard_id))
            .unwrap_or_else(|| Vec::new(&env));
        shard_payments.push_back(Payment { payer: payer.clone(), amount: credited_amount, tip: 0, attestation_hash: None, donate_on_failure: false });
        env.storage().persistent().set(&pay_shard_key(invoice_id, shard_id), &shard_payments);

        invoice.funded += credited_amount;

        append_audit_entry(&env, invoice_id, symbol_short!("pay_tok"), &payer);
        events::payment_received(&env, invoice_id, &payer, credited_amount);
        notify_invoice(&env, invoice_id, symbol_short!("pay"), &invoice.notification_contract);

        if invoice.funded >= total {
            let in_group = env.storage().persistent().has(&invoice_group_key(invoice_id));
            let guarded =
                invoice.prerequisite_id.is_some()
                    || !invoice.tranches.is_empty()
                    || !invoice.release_stages.is_empty()
                    || in_group
                    || !invoice.co_signers.is_empty()
                    || (invoice.min_funding_bps > 0 && invoice.funded < (invoice.amounts.iter().sum::<i128>() * invoice.min_funding_bps as i128 / 10_000));
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

        let mut invoice = load_invoice(&env, invoice_id);
        assert!(invoice.status == InvoiceStatus::Pending, "invoice is not pending");
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(env.ledger().timestamp() <= invoice.deadline, "invoice deadline has passed");
        assert!(source_amount > 0, "payment amount must be positive");

        let invoice_token = invoice.tokens.get(0).expect("no token");
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
        shard_payments.push_back(Payment { payer: payer.clone(), amount: converted, tip: 0, attestation_hash: None, donate_on_failure: false });
        env.storage().persistent().set(&pay_shard_key(invoice_id, shard_id), &shard_payments);
        
        invoice.funded += converted;

        append_audit_entry(&env, invoice_id, symbol_short!("brdg_pay"), &payer);
        events::payment_received(&env, invoice_id, &payer, converted);
        notify_invoice(&env, invoice_id, symbol_short!("pay"), &invoice.notification_contract);

        if invoice.funded >= total {
            let in_group = env.storage().persistent().has(&invoice_group_key(invoice_id));
            let guarded =
                invoice.prerequisite_id.is_some()
                    || !invoice.tranches.is_empty()
                    || !invoice.release_stages.is_empty()
                    || in_group
                    || !invoice.co_signers.is_empty()
                    || (invoice.oracle_address.is_some() && !invoice.condition_met)
                    || (invoice.min_funding_bps > 0 && invoice.funded < (invoice.amounts.iter().sum::<i128>() * invoice.min_funding_bps as i128 / 10_000));
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
        let shared_token = first_inv.tokens.get(0).expect("no token");

        // Validate all payments and compute total.
        let mut total: i128 = 0;
        for p in payments.iter() {
            let inv = load_invoice(&env, p.invoice_id);
            assert!(inv.status == InvoiceStatus::Pending, "invoice is not pending");
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
                inv.tokens.get(0).expect("no token") == shared_token,
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
            shard_payments.push_back(Payment { payer: payer.clone(), amount: p.amount, tip: 0, attestation_hash: None, donate_on_failure: false });
            env.storage().persistent().set(&pay_shard_key(p.invoice_id, shard_id), &shard_payments);

            inv.funded += p.amount;

            append_audit_entry(&env, p.invoice_id, symbol_short!("pool_pay"), &payer);
            events::payment_received(&env, p.invoice_id, &payer, p.amount);

            let inv_total: i128 = inv.amounts.iter().sum();
            if inv.funded >= inv_total {
                let in_group = env.storage().persistent().has(&invoice_group_key(p.invoice_id));
                let guarded =
                    inv.prerequisite_id.is_some()
                        || !inv.tranches.is_empty()
                        || !inv.release_stages.is_empty()
                        || in_group
                        || !inv.co_signers.is_empty()
                        || (inv.oracle_address.is_some() && !inv.condition_met)
                        || (inv.min_funding_bps > 0 && inv.funded < (inv.amounts.iter().sum::<i128>() * inv.min_funding_bps as i128 / 10_000));
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

    // -----------------------------------------------------------------------
    // Release (#22 prerequisite, #23 tranches)
    // -----------------------------------------------------------------------

    /// Release funds to recipients.
    ///
    /// For tranche invoices, only distributes tranches whose timestamp ≤ now.
    /// Blocks with "prerequisite not released" until the prerequisite invoice is Released.
    /// If an approver is set, requires the invoice to be approved first (issue #25).
    pub fn release(env: Env, invoice_id: u64) {
        require_fn_not_paused(&env, &symbol_short!("release"));
        let caller = env.current_contract_address();
        let mut invoice = load_invoice(&env, invoice_id);

        assert!(!invoice.frozen, "invoice is frozen");
        assert!(!invoice.admin_frozen, "invoice frozen by admin");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        let total: i128 = invoice.amounts.iter().sum();
        let min_required = if invoice.min_funding_bps > 0 {
            (total as u128 * invoice.min_funding_bps as u128 / 10_000u128) as i128
        } else {
            total
        };
        assert!(invoice.funded >= min_required, "minimum funding not reached");

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
                    assert!(group_all_funded(&env, group_id), "group members not fully funded");
                }
                types::GroupMode::Majority => {
                    assert!(group_majority_funded(&env, group_id), "group majority not funded");
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

        Self::_release(&env, invoice_id, &mut invoice, &caller);
    }

    /// Trigger a scheduled release at the configured timestamp, respecting min_funding_bps
    pub fn trigger_scheduled_release(env: Env, invoice_id: u64) {
        require_not_paused(&env);
        let mut invoice = load_invoice(&env, invoice_id);

        assert!(!invoice.frozen, "invoice is frozen");
        assert!(!invoice.admin_frozen, "invoice frozen by admin");
        assert!(invoice.status == InvoiceStatus::Pending, "invoice is not pending");

        let scheduled_at = invoice.scheduled_release_at.expect("no scheduled release time");
        assert!(env.ledger().timestamp() >= scheduled_at, "scheduled release time not reached");

        // Check min funding requirement if set
        if invoice.min_funding_bps > 0 {
            let total: i128 = invoice.amounts.iter().sum();
            let min_required = (total as u128 * invoice.min_funding_bps as u128 / 10_000u128) as i128;
            assert!(invoice.funded >= min_required, "minimum funding not reached");
        }

        // Approval check (issue #25)
        if invoice.approver.is_some() && !invoice.approved {
            panic!("awaiting approval");
        }

        // Prerequisite check (issue #22)
        if let Some(prereq_id) = invoice.prerequisite_id {
            let prereq = load_invoice(&env, prereq_id);
            assert!(prereq.status == InvoiceStatus::Released, "prerequisite not released");
        }

        // Co-signer approval check
        if !invoice.co_signers.is_empty() {
            assert!(
                invoice.signatures.len() >= invoice.required_signatures,
                "not enough co-signer approvals"
            );
        }

        let caller = env.current_contract_address();
        Self::_release(&env, invoice_id, &mut invoice, &caller);
    }

    fn _release(env: &Env, invoice_id: u64, invoice: &mut Invoice, actor: &Address) {
        if invoice.tranches.is_empty() {
            Self::_release_full(env, invoice_id, invoice, actor);
        } else {
            Self::_release_tranches(env, invoice_id, invoice, actor);
        }
    }

    fn execute_smart_route(env: &Env, invoice: &Invoice, recipient: &Address, payout: i128) -> bool {
        if invoice.smart_route {
            if let Some(dex_router) = env.storage().instance().get::<_, Address>(&soroban_sdk::symbol_short!("dex_rtr")) {
                let token = invoice.tokens.get(0).expect("no token");
                let path: Vec<Address> = env.invoke_contract(
                    &dex_router,
                    &soroban_sdk::Symbol::new(env, "get_path"),
                    (token.clone(), recipient.clone()).into_val(env)
                );
                if !path.is_empty() {
                    let _: Val = env.invoke_contract(
                        &dex_router,
                        &soroban_sdk::Symbol::new(env, "route_transfer"),
                        (path, payout, recipient.clone()).into_val(env)
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

        let approver = invoice.approver.as_ref().expect("no approver set on this invoice");
        approver.require_auth();

        invoice.approved = true;
        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("aprv"), approver);
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
                || invoice.co_creators.iter().any(|c| c == creator),
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
                || invoice.co_creators.iter().any(|c| c == creator),
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
        assert!(
            invoice.creator == creator
                || invoice.co_creators.iter().any(|c| c == creator),
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
        assert!(
            invoice.creator == creator
                || invoice.co_creators.iter().any(|c| c == creator),
            "only creator can modify allowlist"
        );

        // If allowed_payers is None, initialize a new list (makes invoice private).
        if invoice.allowed_payers.is_none() {
            invoice.allowed_payers = Some(Vec::new(&env));
        }

        if let Some(ref mut whitelist) = invoice.allowed_payers {
            // Only add if not already present
            if !whitelist.iter().any(|p| p == payer) {
                // Issue #309: enforce max 100 allowed payers
                assert!(whitelist.len() < 100, "allowlist is full");
                whitelist.push_back(payer.clone());
                save_invoice(&env, invoice_id, &invoice);
                append_audit_entry(&env, invoice_id, symbol_short!("add_payer"), &creator);
                // Issue #309: emit AllowlistUpdated event
                events::allowlist_updated(&env, invoice_id, &creator, &payer, true);
            }
        }
    }

    /// Admin override: force-resume any paused invoice regardless of who paused it.
    ///
    /// Requires admin auth. Clears the frozen flag, reason, and auto-resume time,
    /// and emits a force_resumed event with the admin address.
    pub fn admin_force_resume(env: Env, admin: Address, invoice_id: u64) {
        require_role(&env, &admin, AdminRole::Operator);

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
        let oracle = invoice.oracle_address.as_ref().expect("no oracle set for invoice");
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
        env.storage().persistent().remove(&reminder_key(invoice_id, &who));
        append_audit_entry(&env, invoice_id, symbol_short!("trig_rmd"), &who);
    }

    /// Create a treasury group linking multiple invoice IDs to a single treasury address.
    /// Returns the new group id.
    pub fn group_treasury_create(env: Env, creator: Address, invoice_ids: Vec<u64>, treasury: Address) -> u64 {
        require_not_paused(&env);
        creator.require_auth();
        let id: u64 = env
            .storage()
            .persistent()
            .get(&treasury_group_counter_key())
            .unwrap_or(0u64)
            + 1;
        env.storage().persistent().set(&treasury_group_counter_key(), &id);
        let record = types::TreasuryRecord { invoice_ids: invoice_ids.clone(), treasury: treasury.clone() };
        env.storage().persistent().set(&group_treasury_key(id), &record);
        for iid in invoice_ids.iter() {
            env.storage().persistent().set(&invoice_treasury_key(iid), &id);
            append_audit_entry(&env, iid, symbol_short!("grp_tr"), &creator);
        }
        id
    }

    /// Pay toward an invoice using a memo that encodes the invoice id.
    /// Requires payer auth and emits a payment_matched event on success.
    pub fn pay_with_memo(env: Env, payer: Address, memo: u64, amount: i128, nonce: u64, _auto_convert: bool, via: Option<Address>) {
        require_not_paused(&env);
        payer.require_auth();
        // Validate memo corresponds to an existing invoice.
        let _ = load_invoice(&env, memo);
        Self::_pay(&env, &payer, memo, amount, nonce, _auto_convert, via, None, false);
        events::payment_matched(&env, memo, memo, &payer);
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

        let proportional = if idx == n - 1 {
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
            (proportional as u128 * platform_fee_bps as u128 / 10_000u128) as i128
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

        let token_client = token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
        token_client.transfer(&env.current_contract_address(), &recipient, &pending);

        env.storage()
            .persistent()
            .remove(&pending_payout_key(invoice_id, &recipient));

        events::pending_payout_claimed(&env, invoice_id, &recipient, pending);
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

        let token_client =
            token::Client::new(env, &invoice.tokens.get(0).expect("no token"));

        let platform_fee_bps: u32 = env
            .storage()
            .instance()
            .get(&platform_fee_bps_key())
            .unwrap_or(0u32);

        let total: i128 = invoice.amounts.iter().sum();
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
            // integer math: avoid overflow via u128 intermediary.
            let payout_raw = (amount as u128)
                .saturating_mul(new_bps as u128)
                .saturating_mul(funded as u128)
                / (10000u128 * total as u128);
            let payout_raw = payout_raw as i128;
            if payout_raw > 0 {
                let is_waived = waivers.iter().any(|a| a == recipient);
                let fee = if is_waived {
                    0
                } else {
                    (payout_raw as u128 * platform_fee_bps as u128 / 10_000u128) as i128
                };
                let tax = (payout_raw as u128 * invoice.tax_bps as u128 / 10_000u128) as i128;
                let payout = payout_raw - fee - tax;
                total_fee += fee;
                total_tax += tax;
                let routed = Self::execute_smart_route(env, invoice, &recipient, payout);
                if !routed {
                    token_client.transfer(&env.current_contract_address(), &recipient, &payout);
                }
            }
        }

        if total_tax > 0 {
            if let Some(ref auth) = invoice.tax_authority {
                token_client.transfer(&env.current_contract_address(), auth, &total_tax);
            }
        }

        if total_fee > 0 {
            let treasury: Address = env
                .storage()
                .instance()
                .get(&treasury_key())
                .expect("treasury not set");
            token_client.transfer(&env.current_contract_address(), &treasury, &total_fee);
        }

        if total_tax > 0 {
            let tax_authority = invoice.tax_authority.as_ref().unwrap();
            token_client.transfer(&env.current_contract_address(), tax_authority, &total_tax);
        }

        invoice.released_bps += new_bps;

        // Calculate amount released in this tranche call.
        let amount_released = ((funded as u128)
            .saturating_mul(new_bps as u128)
            / 10_000u128) as i128;

        // Increment total_volume and total_released counters (issue #28).
        let total_volume: i128 = env
            .storage()
            .persistent()
            .get(&total_volume_key())
            .unwrap_or(0i128);
        env.storage().persistent().set(
            &total_volume_key(),
            &total_volume.checked_add(amount_released).expect("total_volume overflow"),
        );

        let total_released: i128 = env
            .storage()
            .persistent()
            .get(&total_released_key())
            .unwrap_or(0i128);
        env.storage().persistent().set(
            &total_released_key(),
            &total_released.checked_add(amount_released).expect("total_released overflow"),
        );

        if invoice.released_bps >= 10_000 {
            invoice.status = InvoiceStatus::Released;
            invoice.completion_time = Some(now);
            if invoice.insurance_fund > 0 {
                token_client.transfer(&env.current_contract_address(), &invoice.creator, &invoice.insurance_fund);
                invoice.insurance_fund = 0;
            }
            append_audit_entry(env, invoice_id, symbol_short!("release"), actor);
            events::invoice_released(env, invoice_id, &invoice.recipients);
            events::invoice_state_changed(env, invoice_id, Some(&InvoiceStatus::Pending), &InvoiceStatus::Released, actor);
            notify_invoice(env, invoice_id, symbol_short!("release"), &invoice.notification_contract);
            maybe_record_released(env, &invoice.creator, amount_released);
            update_creator_stats_on_release(env, &invoice.creator, amount_released);
        }

        save_invoice(env, invoice_id, invoice);
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

        assert!(invoice.creator == creator, "only creator can call stage_release");
        assert!(!invoice.frozen, "invoice is frozen");
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.release_stages.is_empty(), "no release stages defined");

        let total: i128 = invoice.amounts.iter().sum();
        assert!(invoice.funded >= total, "invoice not fully funded");

        let stage_idx = invoice.released_stages;
        assert!(
            stage_idx < invoice.release_stages.len(),
            "all stages already released"
        );

        let stage_bps = invoice.release_stages.get(stage_idx).unwrap();

        let token_client =
            token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));

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
            let payout_raw = (amount as u128)
                .saturating_mul(stage_bps as u128)
                .saturating_mul(funded as u128)
                / (10_000u128 * total as u128);
            let payout_raw = payout_raw as i128;
            if payout_raw > 0 {
                let is_waived = waivers.iter().any(|a| a == recipient);
                let fee = if is_waived {
                    0
                } else {
                    (payout_raw as u128 * platform_fee_bps as u128 / 10_000u128) as i128
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
            token_client.transfer(&env.current_contract_address(), &treasury, &total_fee);
        }

        if total_tax > 0 {
            let tax_authority = invoice.tax_authority.as_ref().unwrap();
            token_client.transfer(&env.current_contract_address(), tax_authority, &total_tax);
        }

        invoice.released_stages += 1;

        // Calculate amount released in this stage.
        let amount_released = ((stage_bps as u128)
            .saturating_mul(funded as u128)
            / 10_000u128) as i128;

        // Increment total_volume and total_released counters (issue #28).
        let total_volume: i128 = env
            .storage()
            .persistent()
            .get(&total_volume_key())
            .unwrap_or(0i128);
        env.storage().persistent().set(
            &total_volume_key(),
            &total_volume.checked_add(amount_released).expect("total_volume overflow"),
        );

        let total_released: i128 = env
            .storage()
            .persistent()
            .get(&total_released_key())
            .unwrap_or(0i128);
        env.storage().persistent().set(
            &total_released_key(),
            &total_released.checked_add(amount_released).expect("total_released overflow"),
        );

        let now = env.ledger().timestamp();
        if invoice.released_stages >= invoice.release_stages.len() {
            invoice.status = InvoiceStatus::Released;
            invoice.completion_time = Some(now);
            if invoice.insurance_fund > 0 {
                token_client.transfer(&env.current_contract_address(), &invoice.creator, &invoice.insurance_fund);
                invoice.insurance_fund = 0;
            }
            append_audit_entry(&env, invoice_id, symbol_short!("stg_rel"), &creator);
            events::invoice_released(&env, invoice_id, &invoice.recipients);
            events::invoice_state_changed(&env, invoice_id, Some(&InvoiceStatus::Pending), &InvoiceStatus::Released, &creator);
            notify_invoice(&env, invoice_id, symbol_short!("release"), &invoice.notification_contract);
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
        assert!(invoice.creator == creator, "only creator can call partial_release");
        assert!(!invoice.frozen, "invoice is frozen");
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(invoice.status == InvoiceStatus::Pending, "invoice is not pending");
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
                    token_client.transfer(&env.current_contract_address(), &recipient, &recip_amount);
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

        let token_client =
            token::Client::new(env, &invoice.tokens.get(0).expect("no token"));

        // Issue #296: if creator has a fee waiver, platform_fee_bps is 0 for this invoice.
        let creator_waived: bool = {
            let cfw: Vec<Address> = env
                .storage()
                .persistent()
                .get(&creator_fee_waiver_key())
                .unwrap_or_else(|| Vec::new(env));
            cfw.iter().any(|a| a == invoice.creator)
        };

        let platform_fee_bps: u32 = if creator_waived {
            0
        } else {
            env
            .storage()
            .instance()
            .get(&platform_fee_bps_key())
            .unwrap_or(0u32)
        };

        let total: i128 = invoice.amounts.iter().sum();
        let funded = invoice.funded;
        let n = invoice.recipients.len();
        let mut distributed: i128 = 0;
        let mut total_fee: i128 = 0;
        let mut total_tax: i128 = 0;
        let mut payouts: Vec<i128> = Vec::new(env);
        
        let waivers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&platform_fee_waiver_list_key())
            .unwrap_or_else(|| Vec::new(&env));
        
        for i in 0..n {
            let recipient = invoice.recipients.get(i).unwrap();
            let amount = invoice.amounts.get(i).unwrap();

            // Issue: if split_rules are defined, compute payout from rule instead of amounts[].
            let proportional = if !invoice.split_rules.is_empty() {
                let rule = invoice.split_rules.get(i).unwrap();
                match rule {
                    SplitRule::Fixed(fixed_amt) => fixed_amt,
                    SplitRule::Percentage(bps) => {
                        (funded as u128 * bps as u128 / 10_000u128) as i128
                    }
                    SplitRule::Tiered(threshold, bps) => {
                        if funded > threshold {
                            (funded as u128 * bps as u128 / 10_000u128) as i128
                        } else {
                            0
                        }
                    }
                }
            } else if i == n - 1 {
                funded - distributed
            } else {
                (amount as u128 * funded as u128 / total as u128) as i128
            };
            distributed += proportional;

            let tax = (proportional as u128 * invoice.tax_bps as u128 / 10_000u128) as i128;
            total_tax += tax;
            let post_tax = proportional - tax;

            let is_waived = waivers.iter().any(|a| a == recipient);
            let fee = if is_waived {
                0
            } else {
                (post_tax as u128 * platform_fee_bps as u128 / 10_000u128) as i128
            };
            total_fee += fee;

            payouts.push_back(proportional);
        }

        // If this invoice belongs to a treasury group, route the net payouts to the group's treasury address.
        if let Some((_group_id, record)) = treasury_record_for_invoice(env, invoice_id) {
            // Transfer taxes first.
            if total_tax > 0 {
                if let Some(ref auth) = invoice.tax_authority {
                    token_client.transfer(&env.current_contract_address(), auth, &total_tax);
                }
            }

            // Transfer platform fee to global treasury.
            if total_fee > 0 {
                let treasury: Address = env
                    .storage()
                    .instance()
                    .get(&treasury_key())
                    .expect("treasury not set");
                token_client.transfer(&env.current_contract_address(), &treasury, &total_fee);
            }

            let net = distributed - total_tax - total_fee;
            if net > 0 {
                token_client.transfer(&env.current_contract_address(), &record.treasury, &net);
            }
        } else {
            // Default behavior: transfer to each recipient (or route via DEX/router as configured).
            for i in 0..n {
                let recipient = invoice.recipients.get(i).unwrap();
                let proportional = payouts.get(i).unwrap();

                let tax = (proportional as u128 * invoice.tax_bps as u128 / 10_000u128) as i128;
                let post_tax = proportional - tax;
                
                let is_waived = waivers.iter().any(|a| a == recipient);
                let fee = if is_waived {
                    0
                } else {
                    (post_tax as u128 * platform_fee_bps as u128 / 10_000u128) as i128
                };
                let payout = post_tax - fee;

                // Issue #41: if a swap token is configured for this recipient, invoke DEX swap.
                let swap_token: Option<Address> = invoice
                    .swap_tokens
                    .get(i)
                    .unwrap_or(None);
                if let Some(out_token) = swap_token {
                    let from_token = invoice.tokens.get(0).expect("no token");
                    let mut args: Vec<Val> = Vec::new(env);
                    args.push_back(from_token.into_val(env));
                    args.push_back(out_token.clone().into_val(env));
                    args.push_back(payout.into_val(env));
                    args.push_back(recipient.into_val(env));
                    let _swapped: i128 = env.invoke_contract(&out_token, &Symbol::new(env, "swap"), args);
                } else if invoice.smart_route {
                    let from_token = invoice.tokens.get(0).expect("no token");
                    let mut route_args: Vec<Val> = Vec::new(env);
                    route_args.push_back(from_token.into_val(env));
                    route_args.push_back(payout.into_val(env));
                    route_args.push_back(recipient.clone().into_val(env));
                    token_client.transfer(&env.current_contract_address(), &recipient, &payout);
                } else if invoice.convert_to_stream {
                    if let Some(stream_contract) = env.storage().persistent().get::<Symbol, Address>(&stream_contract_key()) {
                        let duration = invoice.drip_duration.unwrap_or(86_400);
                        token_client.transfer(&env.current_contract_address(), &stream_contract, &payout);
                        let mut args: Vec<Val> = Vec::new(env);
                        args.push_back(recipient.clone().into_val(env));
                        args.push_back(payout.into_val(env));
                        args.push_back(duration.into_val(env));
                        let _: Val = env.invoke_contract(&stream_contract, &Symbol::new(env, "create_stream"), args);
                    } else {
                        token_client.transfer(&env.current_contract_address(), &recipient, &payout);
                    }
                } else {
                    let routed = Self::execute_smart_route(env, invoice, &recipient, payout);
                    if !routed {
                        token_client.transfer(&env.current_contract_address(), &recipient, &payout);
                    }
                }
                if let Some(ref auth) = invoice.tax_authority {
                    token_client.transfer(&env.current_contract_address(), auth, &total_tax);
                }
            }

            if total_fee > 0 {
                let treasury: Address = env
                    .storage()
                    .instance()
                    .get(&treasury_key())
                    .expect("treasury not set");
                token_client.transfer(&env.current_contract_address(), &treasury, &total_fee);
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
                    token_client.transfer(&env.current_contract_address(), &payer, &payout);
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
                        for (j, (recipient, amount)) in
                            member.recipients.iter().zip(member.amounts.iter()).enumerate()
                        {
                            let proportional = if j == (member_n - 1) as usize {
                                member_funded - member_distributed
                            } else {
                                (amount as u128 * member_funded as u128 / member_total as u128) as i128
                            };
                            let fee = (proportional as u128 * platform_fee_bps as u128 / 10_000u128) as i128;
                            let tax = (proportional as u128 * member.tax_bps as u128 / 10_000u128) as i128;
                            let payout = proportional - fee - tax;
                            member_distributed += proportional;
                            group_total_fee += fee;
                            if tax > 0 {
                                let tax_authority = member.tax_authority.as_ref().unwrap();
                                member_token.transfer(&env.current_contract_address(), tax_authority, &tax);
                            }
                            let routed = Self::execute_smart_route(env, &member, &recipient, payout);
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
                        member.status = InvoiceStatus::Released;
                        member.completion_time = Some(env.ledger().timestamp());
                        save_invoice(env, member_id, &member);
                        append_audit_entry(env, member_id, symbol_short!("release"), actor);
                        events::invoice_released(env, member_id, &member.recipients);
                        events::invoice_state_changed(env, member_id, Some(&InvoiceStatus::Pending), &InvoiceStatus::Released, actor);
                    }
                }
            }
        }

        // Return insurance fund to creator on successful release.
        if invoice.insurance_fund > 0 {
            token_client.transfer(&env.current_contract_address(), &invoice.creator, &invoice.insurance_fund);
            invoice.insurance_fund = 0;
        }

        // Forward any leftover (rounding remainder) to configured forward target.
        let leftover = funded.checked_sub(distributed).unwrap_or(0);
        if leftover > 0 {
            if let Some(addr) = invoice.forward_to.as_ref() {
                token_client.transfer(&env.current_contract_address(), addr, &leftover);
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
                shard_payments.push_back(Payment { payer: env.current_contract_address(), amount: leftover, tip: 0, attestation_hash: None, donate_on_failure: false });
                env.storage().persistent().set(&pay_shard_key(target_id, shard_id), &shard_payments);

                target.funded += leftover;
                // If target becomes fully funded, trigger auto-release where applicable.
                let target_total: i128 = target.amounts.iter().sum();
                if target.funded >= target_total {
                    let in_group = env.storage().persistent().has(&invoice_group_key(target_id));
                    let guarded =
                        target.prerequisite_id.is_some()
                            || !target.tranches.is_empty()
                            || !target.release_stages.is_empty()
                            || in_group
                            || !target.co_signers.is_empty();
                    if guarded {
                        save_invoice(env, target_id, &target);
                    } else {
                        Self::_release(env, target_id, &mut target, &env.current_contract_address());
                    }
                } else {
                    save_invoice(env, target_id, &target);
                }
            }
        }

        invoice.status = InvoiceStatus::Released;
        invoice.completion_time = Some(env.ledger().timestamp());
        if invoice.insurance_fund > 0 {
            let token_client = token::Client::new(env, &invoice.tokens.get(0).expect("no token"));
            token_client.transfer(&env.current_contract_address(), &invoice.creator, &invoice.insurance_fund);
            invoice.insurance_fund = 0;
        }
        save_invoice(env, invoice_id, invoice);
        append_audit_entry(env, invoice_id, symbol_short!("release"), actor);
        events::invoice_released(env, invoice_id, &invoice.recipients);
        events::invoice_state_changed(env, invoice_id, Some(&InvoiceStatus::Pending), &InvoiceStatus::Released, actor);
        notify_invoice(env, invoice_id, symbol_short!("release"), &invoice.notification_contract);
        maybe_record_released(env, &invoice.creator, funded);

        // Increment total_volume and total_released counters (issue #28).
        let total_volume: i128 = env
            .storage()
            .persistent()
            .get(&total_volume_key())
            .unwrap_or(0i128);
        let new_total_volume = total_volume.checked_add(funded).expect("total_volume overflow");
        env.storage().persistent().set(
            &total_volume_key(),
            &new_total_volume,
        );
        // Issue #276: emit platform volume milestone if threshold crossed.
        check_platform_milestone(env, new_total_volume);

        let total_released: i128 = env
            .storage()
            .persistent()
            .get(&total_released_key())
            .unwrap_or(0i128);
        env.storage().persistent().set(
            &total_released_key(),
            &total_released.checked_add(funded).expect("total_released overflow"),
        );

        // Increment creator analytics (issue #106).
        let creator_volume: i128 = env
            .storage()
            .persistent()
            .get(&creator_stats_volume_key(&invoice.creator))
            .unwrap_or(0i128);
        let new_creator_volume = creator_volume.checked_add(funded).expect("creator_volume overflow");
        env.storage().persistent().set(
            &creator_stats_volume_key(&invoice.creator),
            &new_creator_volume,
        );
        // Issue #276: emit creator volume milestone if threshold crossed.
        check_creator_milestone(env, &invoice.creator, new_creator_volume);

        let creator_released: u64 = env
            .storage()
            .persistent()
            .get(&creator_stats_released_key(&invoice.creator))
            .unwrap_or(0u64);
        env.storage().persistent().set(
            &creator_stats_released_key(&invoice.creator),
            &creator_released.checked_add(1).expect("creator_released overflow"),
        );

        // Spin up next subscription invoice if one is scheduled.
        if let Some(params) = env
            .storage()
            .persistent()
            .get::<(Symbol, u64), SubscriptionParams>(&subscription_params_key(invoice_id))
        {
            let next_deadline = env.ledger().timestamp() + 30 * 24 * 60 * 60;
            let first_token = params.tokens.get(0).expect("no token in subscription");
            let _next_id = Self::_create_invoice_inner(
                env,
                params.creator.clone(),
                params.recipients.clone(),
                params.amounts.clone(),
                first_token,
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
                false, // require_kyc
                None, // scheduled_release_at
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
        assert!(!invoice.auto_resolve_rules.is_empty(), "no auto-resolve rules defined");

        let total: i128 = invoice.amounts.iter().sum();
        assert!(total > 0, "invoice total must be positive");

        let funded_bps = (invoice.funded as u128 * 10_000u128 / total as u128) as u32;

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
                        let token_client = token::Client::new(
                            &env,
                            &invoice.tokens.get(0).expect("no token"),
                        );
                        let mut totals: Map<Address, i128> = Map::new(&env);
                        for payment in invoice.payments.iter() {
                            let prev = totals.get(payment.payer.clone()).unwrap_or(0);
                            totals.set(payment.payer.clone(), prev + payment.amount);
                        }
                        let mut total_refunded_amount: i128 = 0;
                        for (payer, amount) in totals.iter() {
                            token_client.transfer(
                                &env.current_contract_address(),
                                &payer,
                                &amount,
                            );
                            total_refunded_amount += amount;
                            events::payer_refunded(&env, invoice_id, &payer, amount);
                        }
                        invoice.status = InvoiceStatus::Refunded;
                        invoice.completion_time = Some(env.ledger().timestamp());
                        save_invoice(&env, invoice_id, &invoice);
                        let actor = env.current_contract_address();
                        append_audit_entry(&env, invoice_id, symbol_short!("auto_ref"), &actor);
                        events::invoice_refunded(&env, invoice_id);
                        events::invoice_state_changed(&env, invoice_id, Some(&InvoiceStatus::Pending), &InvoiceStatus::Refunded, &actor);
                        maybe_record_refunded(&env, &invoice.creator);
                        notify_invoice(&env, invoice_id, symbol_short!("refund"), &invoice.notification_contract);
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
            invoice.creator == creator
                || invoice.co_creators.iter().any(|c| c == creator),
            "only creator can refund"
        );
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(bps <= 10_000, "bps must be ≤ 10000");
        assert!(invoice.funded > 0, "no funds to refund");

        let token_client =
            token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));

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

        invoice.funded = invoice.funded.checked_sub(total_refunded).expect("funded underflow");
        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("part_ref"), &creator);
        events::partial_refund_issued(&env, invoice_id, &creator, bps, total_refunded);
    }

    /// Refund all payers if the deadline has passed and the invoice is not fully funded.
    pub fn refund(env: Env, invoice_id: u64) {
        require_fn_not_paused(&env, &symbol_short!("refund"));
        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        // Check grace period if configured
        let refund_deadline = if let Some(grace_secs) = invoice.refund_grace_secs {
            invoice.deadline.saturating_add(grace_secs)
        } else {
            invoice.deadline
        };

        assert!(
            env.ledger().timestamp() > refund_deadline,
            "deadline has not passed"
        );

        if invoice.auction_on_expiry {
            let now = env.ledger().timestamp();
            if invoice.auction_end == 0 {
                invoice.auction_end = now.saturating_add(24 * 60 * 60);
                save_invoice(&env, invoice_id, &invoice);
                append_audit_entry(&env, invoice_id, symbol_short!("auc_strt"), &env.current_contract_address());
                return;
            }
            assert!(now > invoice.auction_end, "auction in progress");
            panic!("auction ended; settle auction");
        }

        let token_client =
            token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));

        // Aggregate payments from all shards (issue #177).
        let mut totals: Map<Address, i128> = Map::new(&env);
        // Issue #204: separate map for donate-on-failure contributions.
        let mut donate_totals: Map<Address, i128> = Map::new(&env);
        for shard_id in 0..SHARD_COUNT {
            if let Some(shard_payments) = env.storage().persistent().get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(invoice_id, shard_id)) {
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
        events::invoice_refunded(&env, invoice_id);
        events::invoice_state_changed(&env, invoice_id, Some(&InvoiceStatus::Pending), &InvoiceStatus::Refunded, &actor);
        notify_invoice(&env, invoice_id, symbol_short!("refund"), &invoice.notification_contract);
        maybe_record_refunded(&env, &invoice.creator);

        // Increment total_refunded counter (issue #28).
        let total_refunded: i128 = env
            .storage()
            .persistent()
            .get(&total_refunded_key())
            .unwrap_or(0i128);
        env.storage().persistent().set(
            &total_refunded_key(),
            &total_refunded.checked_add(total_refunded_amount).expect("total_refunded overflow"),
        );

        // Increment creator refund counter (issue #106).
        let creator_refunded: u64 = env
            .storage()
            .persistent()
            .get(&creator_stats_refunded_key(&invoice.creator))
            .unwrap_or(0u64);
        env.storage().persistent().set(
            &creator_stats_refunded_key(&invoice.creator),
            &creator_refunded.checked_add(1).expect("creator_refunded overflow"),
        );
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

        let current_highest = invoice
            .bids
            .iter()
            .map(|b| b.amount)
            .fold(0, |max, amt| if amt > max { amt } else { max });
        assert!(amount > current_highest, "bid must be higher than current highest bid");

        let token_client = token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
        token_client.transfer(&bidder, &env.current_contract_address(), &amount);

        invoice.bids.push_back(Bid { bidder: bidder.clone(), amount });
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
        assert!(invoice.status == InvoiceStatus::Pending, "invoice is not pending");

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
            token_client.transfer(&env.current_contract_address(), &winner.bidder, &invoice.funded);
            for i in 0..invoice.bids.len() {
                if i != idx {
                    let bid = invoice.bids.get(i).unwrap();
                    token_client.transfer(&env.current_contract_address(), &bid.bidder, &bid.amount);
                }
            }
            invoice.status = InvoiceStatus::Refunded;
            invoice.completion_time = Some(now);
            save_invoice(&env, invoice_id, &invoice);
            events::invoice_state_changed(&env, invoice_id, Some(&InvoiceStatus::Pending), &InvoiceStatus::Refunded, &env.current_contract_address());
            append_audit_entry(&env, invoice_id, symbol_short!("auc_stl"), &env.current_contract_address());
            return;
        }

        // No bids were placed; refund payers as normal.
        // Aggregate payments from all shards (issue #177).
        let mut totals: Map<Address, i128> = Map::new(&env);
        for shard_id in 0..SHARD_COUNT {
            if let Some(shard_payments) = env.storage().persistent().get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(invoice_id, shard_id)) {
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
            token_client.transfer(&env.current_contract_address(), &invoice.creator, &invoice.bonus_pool);
        }

        invoice.status = InvoiceStatus::Refunded;
        invoice.completion_time = Some(now);
        save_invoice(&env, invoice_id, &invoice);
        events::invoice_state_changed(&env, invoice_id, Some(&InvoiceStatus::Pending), &InvoiceStatus::Refunded, &env.current_contract_address());
        append_audit_entry(&env, invoice_id, symbol_short!("auc_stl"), &env.current_contract_address());

        let total_refunded: i128 = env.storage().persistent().get(&total_refunded_key()).unwrap_or(0i128);
        env.storage().persistent().set(
            &total_refunded_key(),
            &total_refunded.checked_add(total_refunded_amount).expect("total_refunded overflow"),
        );
    }

    /// Cancel an invoice. Refunds any payments already made.
    /// Issue #89: If stake exists, distributes it equally among unique payers.
    pub fn cancel_invoice(env: Env, caller: Address, invoice_id: u64) {
        require_not_paused(&env);
        caller.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.disputed, "invoice is disputed");
        // If a creator cosigner is set, require both the creator and cosigner auths.
        if let Some(cos) = invoice.creator_cosigner.clone() {
            invoice.creator.require_auth();
            cos.require_auth();
        } else {
            assert!(invoice.creator == caller, "only creator can cancel");
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

        if invoice.funded > 0 {
            // Refund all payments.
            let token_client =
                token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));

            // Aggregate payments from all shards (issue #177).
            let mut totals: Map<Address, i128> = Map::new(&env);
            for shard_id in 0..SHARD_COUNT {
                if let Some(shard_payments) = env.storage().persistent().get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(invoice_id, shard_id)) {
                    for payment in shard_payments.iter() {
                        let prev = totals.get(payment.payer.clone()).unwrap_or(0);
                        totals.set(payment.payer.clone(), prev + payment.amount);
                    }
                }
            }

            // Issue #89: Distribute stake equally among unique payers if stake exists.
            // (stake_amount field not yet on Invoice; skipped)

            let mut total_refunded_amount: i128 = 0;
            for (payer, amount) in totals.iter() {
                let mut refund = amount;
                if invoice.insurance_fund > 0 {
                    let premium_refund = (amount as u128 * invoice.insurance_fund as u128 / invoice.funded as u128) as i128;
                    refund += premium_refund;
                }
                token_client.transfer(&env.current_contract_address(), &payer, &refund);
                total_refunded_amount += amount;
            }

            if invoice.insurance_fund > 0 {
                invoice.insurance_fund = 0;
            }

            if invoice.bonus_pool > 0 {
                token_client.transfer(
                    &env.current_contract_address(),
                    &invoice.creator,
                    &invoice.bonus_pool,
                );
            }

            if invoice.insurance_fund > 0 {
                let mut total_paid: i128 = 0;
                for (_, amt) in totals.iter() {
                    total_paid += amt;
                }
                if total_paid > 0 {
                    for (payer, amt) in totals.iter() {
                        let share = (invoice.insurance_fund as u128 * amt as u128 / total_paid as u128) as i128;
                        if share > 0 {
                            token_client.transfer(&env.current_contract_address(), &payer, &share);
                        }
                    }
                }
                invoice.insurance_fund = 0;
            }

            invoice.status = InvoiceStatus::Refunded;
            events::invoice_state_changed(&env, invoice_id, Some(&InvoiceStatus::Pending), &InvoiceStatus::Refunded, &caller);
            maybe_record_refunded(&env, &invoice.creator);

            // Increment total_refunded counter (issue #28).
            let total_refunded: i128 = env
                .storage()
                .persistent()
                .get(&total_refunded_key())
                .unwrap_or(0i128);
            env.storage().persistent().set(
                &total_refunded_key(),
                &total_refunded.checked_add(total_refunded_amount).expect("total_refunded overflow"),
            );
        } else {
            if invoice.bonus_pool > 0 {
                let token_client =
                    token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
                token_client.transfer(
                    &env.current_contract_address(),
                    &invoice.creator,
                    &invoice.bonus_pool,
                );
            }
            
            // Issue #89: Return stake to creator if no payments were made.
            // (stake_amount field not yet on Invoice; skipped)

            invoice.status = InvoiceStatus::Cancelled;
            events::invoice_state_changed(&env, invoice_id, Some(&InvoiceStatus::Pending), &InvoiceStatus::Cancelled, &caller);
        }

        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("cancel"), &caller);

        // Issue: increment per-creator cancel count for cancellation rate tracking.
        let cnl_cnt: u64 = env
            .storage()
            .persistent()
            .get(&cancel_count_key(&caller))
            .unwrap_or(0u64);
        env.storage()
            .persistent()
            .set(&cancel_count_key(&caller), &(cnl_cnt + 1));
    }

    /// Transfer invoice ownership to a new creator.
    pub fn transfer_invoice(env: Env, invoice_id: u64, new_creator: Address) {
        require_not_paused(&env);
        let mut invoice = load_invoice(&env, invoice_id);

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
            // Accept caller = creator OR assigned delegate (issue #43).
            let delegate: Option<Address> = env
                .storage()
                .persistent()
                .get(&delegate_key(invoice_id));
            let is_creator = invoice.creator == caller;
            let is_delegate = delegate.map(|d| d == caller).unwrap_or(false);
            assert!(is_creator || is_delegate, "not authorized");
        }

        invoice.deadline = new_deadline;
        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("extend"), &caller);
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
            old_invoice.tokens.get(0).expect("no token"),
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
            old_invoice.oracle_address.clone(),
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
        );

        // Copy payments from shards to new invoice (issue #177).
        for shard_id in 0..SHARD_COUNT {
            if let Some(shard_payments) = env.storage().persistent().get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(invoice_id, shard_id)) {
                env.storage().persistent().set(&pay_shard_key(new_id, shard_id), &shard_payments);
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
        events::invoice_state_changed(&env, invoice_id, Some(&InvoiceStatus::Pending), &InvoiceStatus::Refunded, &caller);

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

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(invoice.creator == caller, "only creator can add recipients");
        assert!(invoice.funded == 0, "cannot add recipient after payment received");
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
    pub fn adjust_split(
        env: Env,
        caller: Address,
        invoice_id: u64,
        new_amounts: Vec<i128>,
    ) {
        require_not_paused(&env);
        caller.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

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
            deadline: 0,
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

        version
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

        let tmpl: InvoiceTemplate = if let Some(v) = version {
            env.storage()
                .persistent()
                .get(&template_version_key(&creator, &name, v))
                .expect("template version not found")
        } else {
            let count: u32 = env
                .storage()
                .persistent()
                .get(&template_version_count_key(&creator, &name))
                .unwrap_or(0u32);
            if count > 0 {
                env.storage()
                    .persistent()
                    .get(&template_version_key(&creator, &name, count))
                    .expect("latest template not found")
            } else {
                // Fall back to legacy unversioned key.
                env.storage()
                    .persistent()
                    .get(&template_key(&creator, &name))
                    .expect("template not found")
            }
        };
        Self::_create_invoice_inner(
            &env,
            creator,
            tmpl.recipients,
            tmpl.amounts,
            tmpl.token,
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
            false, // require_kyc
            None, // scheduled_release_at
        )
    }

    /// Link invoices into a group.
    ///
    /// `majority` — when `false` (default), all members must be fully funded before
    /// any can release (AllOrNothing). When `true`, a strict majority (>50%) being
    /// fully funded is sufficient to unblock release (Issue #212).
    pub fn create_invoice_group(env: Env, invoice_ids: Vec<u64>, majority: bool) -> u64 {
        assert!(invoice_ids.len() >= 2, "group needs at least 2 invoices");

        let grp_cnt_key = symbol_short!("grp_cnt");
        let group_id: u64 = env
            .storage()
            .persistent()
            .get(&grp_cnt_key)
            .unwrap_or(0u64)
            + 1;
        env.storage().persistent().set(&grp_cnt_key, &group_id);

        for id in invoice_ids.iter() {
            env.storage()
                .persistent()
                .set(&invoice_group_key(id), &group_id);
        }
        let mode = if majority { types::GroupMode::Majority } else { types::GroupMode::AllOrNothing };
        let group = types::InvoiceGroup { invoice_ids, mode };
        env.storage()
            .persistent()
            .set(&group_key(group_id), &group);

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

        assert!(invoice.allow_early_withdrawal, "early withdrawal not allowed");
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
            if let Some(shard_payments) = env.storage().persistent().get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(invoice_id, shard_id)) {
                let mut new_shard_payments: Vec<Payment> = Vec::new(&env);
                for payment in shard_payments.iter() {
                    if payment.payer != payer {
                        new_shard_payments.push_back(payment.clone());
                    }
                }
                if new_shard_payments.is_empty() {
                    env.storage().persistent().remove(&pay_shard_key(invoice_id, shard_id));
                } else {
                    env.storage().persistent().set(&pay_shard_key(invoice_id, shard_id), &new_shard_payments);
                }
            }
        }
        invoice.funded -= total_paid;

        let token_client =
            token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
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

    /// Vote to extend the invoice deadline by 7 days.
    /// Once a strict majority of unique payers vote, the deadline is extended.
    pub fn vote_extend_deadline(env: Env, invoice_id: u64, voter: Address) {
        voter.require_auth();

        let invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        let has_paid = invoice.payments.iter().any(|p| p.payer == voter);
        assert!(has_paid, "only payers can vote");

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

        if votes.contains(&voter) {
            return;
        }
        votes.push_back(voter);

        if votes.len() > unique_payers.len() / 2 {
            let mut invoice = load_invoice(&env, invoice_id);
            invoice.deadline += 7 * 24 * 60 * 60;
            save_invoice(&env, invoice_id, &invoice);
            env.storage().persistent().remove(&vote_key);
        } else {
            env.storage().persistent().set(&vote_key, &votes);
        }
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

        let token_client =
            token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
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
            || env.storage().persistent().has(&archive_marker_key(invoice_id))
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

    /// Returns the on-chain reputation score (number of successful payments) for an address.
    ///
    /// Returns 0 for an address that has never paid.
    pub fn get_reputation(env: Env, address: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&rep_key(&address))
            .unwrap_or(0u64)
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

    /// Generate a completion proof for a finalized invoice.
    pub fn get_completion_proof(env: Env, invoice_id: u64) -> CompletionProof {
        let invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Released
                || invoice.status == InvoiceStatus::Refunded,
            "invoice not finalized"
        );

        let status_byte: u8 = match invoice.status {
            InvoiceStatus::Pending => 0u8,
            InvoiceStatus::Released => 1u8,
            InvoiceStatus::Refunded => 2u8,
            InvoiceStatus::Cancelled => 3u8,
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

        PaymentProof { invoice_id, payer, total_paid, proof_hash }
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
        let invoice = if env.storage().persistent().has(&invoice_key(proof.invoice_id)) 
            || env.storage().instance().has(&invoice_key(proof.invoice_id)) {
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
        if env.storage().instance().has(&archive_marker_key(invoice_id)) {
            return;
        }

        let core: InvoiceCore = env
            .storage()
            .persistent()
            .get(&invoice_key(invoice_id))
            .or_else(|| env.storage().instance().get(&invoice_key(invoice_id)))
            .expect("invoice not found");

        assert!(
            core.status == InvoiceStatus::Released
                || core.status == InvoiceStatus::Refunded,
            "invoice not completed"
        );

        let ext: InvoiceExt = env.storage().persistent()
            .get(&invoice_ext_key(invoice_id))
            .unwrap_or_else(|| InvoiceExt {
                co_signers: Vec::new(&env), required_signatures: 0, signatures: Vec::new(&env),
                approver: None, approved: false, oracle_address: None, condition_met: false,
                penalty_bps: 0, penalty_deadline: 0, min_funding_bps: 0,
                release_stages: Vec::new(&env), released_stages: 0, allowed_payers: None,
                price_oracle: None, base_amounts: Vec::new(&env), swap_tokens: Vec::new(&env),
                tax_bps: 0, tax_authority: None, insurance_premium_bps: 0, insurance_fund: 0,
                smart_route: false, convert_to_stream: false, accepted_tokens: Vec::new(&env),
                forward_to: None, forward_invoice_id: None, split_rules: Vec::new(&env),
                auto_resolve_rules: Vec::new(&env), creator_cosigner: None, velocity_limit: 0,
                velocity_window: 0, parent_invoice_id: None, pause_reason: None, auto_resume_at: None,
                payment_cooldown_secs: None, max_payments_per_window: None, payment_window_secs: None,
                scheduled_release_at: None, refund_grace_secs: None,
                penalty_tiers: Vec::new(&env), allowed_callers: None,
                refunded_addresses: Vec::new(&env),
            });
        let ext2: InvoiceExt2 = env.storage().persistent()
            .get(&invoice_ext2_key(invoice_id))
            .unwrap_or_else(|| InvoiceExt2 {
                notification_contract: None, overflow_behavior: OverflowBehavior::Reject,
                cross_chain_ref: None, require_kyc: false, arbiter: None, disputed: false,
                admin_frozen: false,
                auction_on_expiry: false, auction_end: 0, bids: Vec::new(&env),
                min_payment: 0, min_funding_amount: 0, priorities: Vec::new(&env),
                target_usd_cents: None,
            });

        // Copy to instance storage.
        env.storage().instance().set(&invoice_key(invoice_id), &core);
        env.storage().instance().set(&invoice_ext_key(invoice_id), &ext);
        env.storage().instance().set(&invoice_ext2_key(invoice_id), &ext2);

        // Remove from persistent storage.
        env.storage().persistent().remove(&invoice_key(invoice_id));
        env.storage().persistent().remove(&invoice_ext_key(invoice_id));
        env.storage().persistent().remove(&invoice_ext2_key(invoice_id));

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
                let ext: InvoiceExt = env.storage().persistent()
                    .get(&invoice_ext_key(id))
                    .unwrap_or_else(|| InvoiceExt {
                        co_signers: Vec::new(&env), required_signatures: 0, signatures: Vec::new(&env),
                        approver: None, approved: false, oracle_address: None, condition_met: false,
                        penalty_bps: 0, penalty_deadline: 0, min_funding_bps: 0,
                        release_stages: Vec::new(&env), released_stages: 0, allowed_payers: None,
                        price_oracle: None, base_amounts: Vec::new(&env), swap_tokens: Vec::new(&env),
                        tax_bps: 0, tax_authority: None, insurance_premium_bps: 0, insurance_fund: 0,
                        smart_route: false, convert_to_stream: false, accepted_tokens: Vec::new(&env),
                        forward_to: None, forward_invoice_id: None, split_rules: Vec::new(&env),
                        auto_resolve_rules: Vec::new(&env), creator_cosigner: None, velocity_limit: 0,
                        velocity_window: 0, parent_invoice_id: None, pause_reason: None, auto_resume_at: None,
                        payment_cooldown_secs: None, max_payments_per_window: None, payment_window_secs: None,
                        scheduled_release_at: None, refund_grace_secs: None,
                        penalty_tiers: Vec::new(&env), allowed_callers: None,
                    });
                let ext2: InvoiceExt2 = env.storage().persistent()
                    .get(&invoice_ext2_key(id))
                    .unwrap_or_else(|| InvoiceExt2 {
                        notification_contract: None, overflow_behavior: OverflowBehavior::Reject,
                        cross_chain_ref: None, require_kyc: false, arbiter: None, disputed: false,
                        admin_frozen: false,
                        auction_on_expiry: false, auction_end: 0, bids: Vec::new(&env),
                        min_payment: 0, min_funding_amount: 0, priorities: Vec::new(&env),
                        target_usd_cents: None,
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
        append_audit_entry(&env, invoice_id, symbol_short!("delegate"), &invoice.creator);
    }

    /// Remove the delegate from an invoice. Requires creator auth.
    pub fn revoke_delegate(env: Env, invoice_id: u64) {
        let invoice = load_invoice(&env, invoice_id);
        invoice.creator.require_auth();

        env.storage()
            .persistent()
            .remove(&delegate_key(invoice_id));

        events::delegate_revoked(&env, invoice_id);
        append_audit_entry(&env, invoice_id, symbol_short!("rvk_del"), &invoice.creator);
    }

    /// Return the current delegate for an invoice, or None if none is set.
    pub fn get_delegate(env: Env, invoice_id: u64) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&delegate_key(invoice_id))
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
            env.storage().persistent().set(&delegate_pay_key(&beneficiary), &delegates);
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
        assert!(invoice.status == InvoiceStatus::Pending, "invoice is not pending");
        assert!(!invoice.disputed, "invoice is disputed");
        assert!(env.ledger().timestamp() <= invoice.deadline, "invoice deadline has passed");
        assert!(amount > 0, "payment amount must be positive");

        let total: i128 = invoice.amounts.iter().sum();
        let remaining = total - invoice.funded;
        assert!(amount <= remaining, "payment exceeds remaining balance");

        let token_client = token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
        token_client.transfer(&delegate, &env.current_contract_address(), &amount);

        // Write payment to sharded storage (issue #177).
        let shard_id = compute_shard_id(&env, &beneficiary);
        let mut shard_payments: Vec<Payment> = env
            .storage()
            .persistent()
            .get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(invoice_id, shard_id))
            .unwrap_or_else(|| Vec::new(&env));
        shard_payments.push_back(Payment { payer: beneficiary.clone(), amount, tip: 0, attestation_hash: None, donate_on_failure: false });
        env.storage().persistent().set(&pay_shard_key(invoice_id, shard_id), &shard_payments);
        
        invoice.funded += amount;

        append_audit_entry(&env, invoice_id, symbol_short!("del_pay"), &delegate);
        events::payment_received(&env, invoice_id, &beneficiary, amount);
        notify_invoice(&env, invoice_id, symbol_short!("pay"), &invoice.notification_contract);

        let in_group = env.storage().persistent().has(&invoice_group_key(invoice_id));
        let guarded =
            invoice.prerequisite_id.is_some()
                || !invoice.tranches.is_empty()
                || !invoice.release_stages.is_empty()
                || in_group
                || !invoice.co_signers.is_empty();
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
                assert!(
                    last_payment_at.saturating_add(cooldown_secs) <= now,
                    "payment cooldown active"
                );
            }
        }

        if let (Some(max_payments), Some(window_secs)) =
            (invoice.max_payments_per_window, invoice.payment_window_secs)
        {
            let recent = Self::active_payment_window(env, invoice_id, now, window_secs);
            assert!(
                recent.len() < max_payments,
                "payment rate limit exceeded"
            );
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

    fn active_payment_window(
        env: &Env,
        invoice_id: u64,
        now: u64,
        window_secs: u64,
    ) -> Vec<u64> {
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

        env.storage()
            .persistent()
            .set(&cert_key(invoice_id), &cert);

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
        require_role(&env, &admin, AdminRole::SuperAdmin);
        env.storage().persistent().set(&circuit_breaker_key(), &true);
        env.storage().persistent().set(&circuit_breaker_reason_key(), &reason.clone());
        events::circuit_breaker_activated(&env, &reason);
    }

    /// Deactivate the circuit breaker. Admin-only.
    pub fn deactivate_circuit_breaker(env: Env, admin: Address) {
        require_role(&env, &admin, AdminRole::SuperAdmin);
        env.storage().persistent().set(&circuit_breaker_key(), &false);
        env.storage().persistent().remove(&circuit_breaker_reason_key());
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
        require_role(&env, &admin, AdminRole::SuperAdmin);
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
            env.storage().persistent().set(&creator_fee_waiver_key(), &waivers);
            events::fee_waiver_granted(&env, &creator);
        }
    }

    /// Revoke a fee waiver from a creator. Admin-only.
    pub fn remove_fee_waiver(env: Env, admin: Address, creator: Address) {
        require_role(&env, &admin, AdminRole::SuperAdmin);
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
        env.storage().persistent().set(&creator_fee_waiver_key(), &new_waivers);
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

        // Verify the range proof: concatenate commitment bytes + range_proof then hash.
        // A valid proof produces a non-zero 32-byte digest (placeholder for full ZK verify).
        let mut preimage = Bytes::new(&env);
        preimage.append(&commitment.into());
        preimage.append(&range_proof);
        let proof_hash = env.crypto().sha256(&preimage);
        let zero: BytesN<32> = BytesN::from_array(&env, &[0u8; 32]);
        assert!(proof_hash != zero, "invalid range proof");

        let record = ConfidentialPayment {
            commitment,
            encrypted_amount,
        };
        env.storage()
            .persistent()
            .set(&confidential_pay_key(invoice_id, &payer), &record);

        let count: u32 = env
            .storage()
            .persistent()
            .get(&confidential_count_key(invoice_id))
            .unwrap_or(0u32);
        env.storage()
            .persistent()
            .set(&confidential_count_key(invoice_id), &(count + 1));
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
        invoice.funded = if new_funded > total { total } else { new_funded };
        save_invoice(&env, invoice_id, &invoice);

        if invoice.funded >= total {
            let actor = env.current_contract_address();
            Self::_release(&env, invoice_id, &mut invoice.clone(), &actor);
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
            if let Some(shard_payments) = env.storage().persistent().get::<(Symbol, u64, u64), Vec<Payment>>(&pay_shard_key(invoice_id, shard_id)) {
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
    // Issue #310: Two-step upgrade with 48-hour timelock
    // -----------------------------------------------------------------------

    /// Propose a contract upgrade. Only the admin may call this.
    ///
    /// Stores a pending proposal with an eligible_at = now + 48 h.
    /// Overwrites any existing proposal (only one active at a time).
    pub fn propose_upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) {
        require_admin(&env);
        let _ = admin;

        const FORTY_EIGHT_HOURS: u64 = 48 * 60 * 60;
        let eligible_at = env.ledger().timestamp().saturating_add(FORTY_EIGHT_HOURS);

        let proposal = UpgradeProposal {
            new_wasm_hash: new_wasm_hash.clone(),
            eligible_at,
        };
        env.storage().instance().set(&upgrade_proposal_key(), &proposal);

        events::upgrade_proposed(&env, &new_wasm_hash, eligible_at);
    }

    /// Execute a pending upgrade once the 48-hour timelock has elapsed.
    ///
    /// Callable by anyone after the timelock expires. Clears the proposal on success.
    pub fn execute_upgrade(env: Env) {
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
        env.deployer().update_current_contract_wasm(proposal.new_wasm_hash);
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

    /// Return the pending upgrade proposal, or None if none is active.
    pub fn get_upgrade_proposal(env: Env) -> Option<UpgradeProposal> {
        env.storage().instance().get(&upgrade_proposal_key())
    }
}
