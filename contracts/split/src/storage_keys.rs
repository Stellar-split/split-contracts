//! Centralised storage key registry (issue #278).
//!
//! Every storage key used by the contract is defined here so there are no
//! raw string literals scattered throughout `lib.rs`.
//!
//! Storage tiers:
//! - **instance** – shared contract-level state (`admin`, `paused`, counters, …)
//! - **persistent** – per-entity state (`invoice_key`, `nonce_key`, …)

use soroban_sdk::{symbol_short, Address, Symbol};

// ---------------------------------------------------------------------------
// Instance-tier keys (contract-level singletons)
// ---------------------------------------------------------------------------

/// Admin address — instance storage.
pub fn admin_key() -> Symbol { symbol_short!("admin") }
/// Admin map (role → address list) — instance storage.
pub fn admins_key() -> Symbol { symbol_short!("admins") }
/// Global pause flag — instance storage.
pub fn paused_key() -> Symbol { symbol_short!("paused") }
/// Per-function pause set — instance storage.
pub fn paused_fns_key() -> Symbol { symbol_short!("ps_fns") }
/// Reentrancy guard flag — instance storage.
pub fn reentrancy_key() -> Symbol { symbol_short!("re_guard") }
/// Platform treasury address — instance storage.
pub fn treasury_key() -> Symbol { symbol_short!("treasury") }
/// USDC token contract address — instance storage.
pub fn usdc_token_key() -> Symbol { symbol_short!("usdc_tok") }
/// Invoice creation fee — instance storage.
pub fn creation_fee_key() -> Symbol { symbol_short!("crt_fee") }
/// Platform fee in basis points — instance storage.
pub fn platform_fee_bps_key() -> Symbol { symbol_short!("plat_fee") }
/// Platform fee waiver address list — instance storage.
pub fn platform_fee_waiver_list_key() -> Symbol { symbol_short!("fee_wvrs") }
/// Invoice ID counter — instance storage.
pub fn counter_key() -> Symbol { symbol_short!("counter") }
/// Global payer velocity limit — instance storage.
pub fn global_payer_limit_key() -> Symbol { symbol_short!("g_vel_lim") }
/// Global payer velocity window — instance storage.
pub fn global_payer_window_key() -> Symbol { symbol_short!("g_vel_win") }
/// Stream contract address — instance storage.
pub fn stream_contract_key() -> Symbol { symbol_short!("strm_ctr") }
/// Creator whitelist flag — instance storage.
pub fn creator_whitelist_key() -> Symbol { symbol_short!("crt_wl") }
/// Compliance contract address — instance storage.
pub fn compliance_key() -> Symbol { symbol_short!("comply") }
/// KYC verification contract address — instance storage.
pub fn kyc_contract_key() -> Symbol { symbol_short!("kyc_ctr") }
/// Global rate limit (max invoices per window) — instance storage.
pub fn rate_limit_key() -> Symbol { symbol_short!("rate_lim") }
/// Global rate limit window in seconds — instance storage.
pub fn rate_window_key() -> Symbol { symbol_short!("rate_win") }
/// Maximum cancellation rate in basis points — instance storage.
pub fn max_cancel_bps_key() -> Symbol { symbol_short!("mx_cnl_bp") }
/// Receipt token factory contract address — instance storage.
pub fn receipt_factory_key() -> Symbol { symbol_short!("rcpt_fac") }
/// Dashboard contract address — instance storage.
pub fn dashboard_contract_key() -> Symbol { symbol_short!("dash_ctr") }
/// NFT gate contract address — instance storage.
pub fn nft_gate_key() -> Symbol { symbol_short!("nft_gte") }
/// Timelock duration in seconds — instance storage.
pub fn timelock_secs_key() -> Symbol { symbol_short!("tl_secs") }
/// Timelock action counter — instance storage.
pub fn timelock_action_counter_key() -> Symbol { symbol_short!("tl_cntr") }
/// Fee tiers list — instance storage.
pub fn fee_tiers_key() -> Symbol { symbol_short!("fee_trs") }
/// Pending admin proposal address — instance storage.
pub fn pending_admin_key() -> Symbol { symbol_short!("pend_adm") }
/// Timestamp of pending admin proposal — instance storage.
pub fn admin_proposal_time_key() -> Symbol { symbol_short!("adm_prop") }
/// Governance contract address — instance storage.
pub fn governance_contract_key() -> Symbol { symbol_short!("gov_ctr") }
/// Authorised factory addresses — instance storage.
pub fn factories_key() -> Symbol { symbol_short!("factories") }
/// DEX contract address — instance storage.
pub fn dex_contract_key() -> Symbol { symbol_short!("dex_ctr") }
/// Total invoices created counter — instance storage.
pub fn total_invoices_key() -> Symbol { symbol_short!("tot_inv") }
/// Total funded volume counter — instance storage.
pub fn total_volume_key() -> Symbol { symbol_short!("tot_vol") }
/// Total released volume counter — instance storage.
pub fn total_released_key() -> Symbol { symbol_short!("tot_rel") }
/// Total refunded volume counter — instance storage.
pub fn total_refunded_key() -> Symbol { symbol_short!("tot_ref") }
/// Treasury group counter — instance storage.
pub fn treasury_group_counter_key() -> Symbol { symbol_short!("grp_tr_cn") }
/// Contract version — instance storage (issue #279).
pub fn contract_version_key() -> Symbol { symbol_short!("ct_ver") }

// ---------------------------------------------------------------------------
// Persistent-tier keys (per-entity)
// ---------------------------------------------------------------------------

/// Core invoice fields — persistent storage.
pub fn invoice_key(id: u64) -> (Symbol, u64) { (symbol_short!("inv"), id) }
/// Extended invoice fields (`InvoiceExt`) — persistent storage.
pub fn invoice_ext_key(id: u64) -> (Symbol, u64) { (symbol_short!("inv_ext"), id) }
/// Extended invoice fields 2 (`InvoiceExt2`) — persistent storage.
pub fn invoice_ext2_key(id: u64) -> (Symbol, u64) { (symbol_short!("inv_ex2"), id) }
/// Compact byte representation of an invoice — persistent storage.
pub fn invoice_compact_key(id: u64) -> (Symbol, u64) { (symbol_short!("inv_cpt"), id) }
/// Audit log entries for an invoice — persistent storage.
pub fn audit_log_key(id: u64) -> (Symbol, u64) { (symbol_short!("log"), id) }
/// Sharded payment list: `(invoice_id, shard_id)` — persistent storage.
pub fn payment_shard_key(invoice_id: u64, shard_id: u64) -> (Symbol, u64, u64) { (symbol_short!("pay_sh"), invoice_id, shard_id) }
/// Timelock action entry — persistent storage.
pub fn timelock_action_key(action_id: u64) -> (Symbol, u64) { (symbol_short!("tl_act"), action_id) }
/// Subscription parameters — persistent storage.
pub fn subscription_params_key(id: u64) -> (Symbol, u64) { (symbol_short!("sub"), id) }
/// Subscription subscriber list — persistent storage.
pub fn subscription_subscribers_key(sub_id: u64) -> (Symbol, u64) { (symbol_short!("sub_sub"), sub_id) }
/// External governance vote key — persistent storage.
pub fn ext_vote_key(id: u64) -> (Symbol, u64) { (symbol_short!("ext_vote"), id) }
/// Invoice group — persistent storage.
pub fn group_key(group_id: u64) -> (Symbol, u64) { (symbol_short!("grp"), group_id) }
/// Reverse lookup: invoice → group — persistent storage.
pub fn invoice_group_key(invoice_id: u64) -> (Symbol, u64) { (symbol_short!("invgrp"), invoice_id) }
/// Invoice-level treasury record — persistent storage.
pub fn invoice_treasury_key(invoice_id: u64) -> (Symbol, u64) { (symbol_short!("inv_tr"), invoice_id) }
/// Treasury group — persistent storage.
pub fn group_treasury_key(group_id: u64) -> (Symbol, u64) { (symbol_short!("grp_tr"), group_id) }
/// Invoice template — persistent storage.
pub fn template_key(creator: &Address, name: &Symbol) -> (Symbol, Address, Symbol) { (symbol_short!("tmpl"), creator.clone(), name.clone()) }
/// Versioned invoice template — persistent storage.
pub fn template_version_key(creator: &Address, name: &Symbol, version: u32) -> (Symbol, Address, Symbol, u32) { (symbol_short!("tmpl_v"), creator.clone(), name.clone(), version) }
/// Template version counter — persistent storage.
pub fn template_version_count_key(creator: &Address, name: &Symbol) -> (Symbol, Address, Symbol) { (symbol_short!("tmpl_ct"), creator.clone(), name.clone()) }
/// Pending payout per `(invoice_id, recipient)` — persistent storage.
pub fn pending_payout_key(invoice_id: u64, recipient: &Address) -> (Symbol, u64, Address) { (symbol_short!("pend_pay"), invoice_id, recipient.clone()) }
/// Per-address reputation counter — persistent storage.
pub fn rep_key(payer: &Address) -> (Symbol, Address) { (symbol_short!("rep"), payer.clone()) }
/// Per-address credit score — persistent storage.
pub fn credit_key(payer: &Address) -> (Symbol, Address) { (symbol_short!("credit"), payer.clone()) }
/// Per-address referral count — persistent storage.
pub fn referral_count_key(referrer: &Address) -> (Symbol, Address) { (symbol_short!("ref_cnt"), referrer.clone()) }
/// Payment channel state — persistent storage.
pub fn channel_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) { (symbol_short!("chan"), invoice_id, payer.clone()) }
/// Per-payer per-invoice nonce (replay protection) — persistent storage.
pub fn nonce_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) { (symbol_short!("nonce"), invoice_id, payer.clone()) }
/// Per-payer velocity window state — persistent storage.
pub fn vel_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) { (symbol_short!("vel"), invoice_id, payer.clone()) }
/// Global cross-invoice per-payer velocity — persistent storage.
pub fn global_vel_key(payer: &Address) -> (Symbol, Address) { (symbol_short!("g_vel"), payer.clone()) }
/// Per-recipient invoice ID index — persistent storage.
pub fn recipient_invoice_ids_key(recipient: &Address) -> (Symbol, Address) { (symbol_short!("rec_inv"), recipient.clone()) }
/// Delegate address for an invoice — persistent storage.
pub fn delegate_key(invoice_id: u64) -> (Symbol, u64) { (symbol_short!("delegate"), invoice_id) }
/// Delegate-pay authorization — persistent storage.
pub fn delegate_pay_key(beneficiary: &Address) -> (Symbol, Address) { (symbol_short!("dlgt_pay"), beneficiary.clone()) }
/// Per-creator rate limit usage — persistent storage.
pub fn rate_usage_key(creator: &Address) -> (Symbol, Address) { (symbol_short!("rate"), creator.clone()) }
/// Per-creator invoice creation count — persistent storage.
pub fn invoice_count_key(creator: &Address) -> (Symbol, Address) { (symbol_short!("inv_count"), creator.clone()) }
/// Per-creator invoice cancellation count — persistent storage.
pub fn cancel_count_key(creator: &Address) -> (Symbol, Address) { (symbol_short!("cnl_count"), creator.clone()) }
/// Per-payer per-invoice receipt token address — persistent storage.
pub fn receipt_token_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) { (symbol_short!("rcpt"), invoice_id, payer.clone()) }
/// Per-invoice per-payer micro-payment accumulator — persistent storage.
pub fn accum_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) { (symbol_short!("accum"), invoice_id, payer.clone()) }
/// Per-creator total invoice count — persistent storage.
pub fn creator_stats_count_key(creator: &Address) -> (Symbol, Address) { (symbol_short!("cr_cnt"), creator.clone()) }
/// Per-creator total funded volume — persistent storage.
pub fn creator_stats_volume_key(creator: &Address) -> (Symbol, Address) { (symbol_short!("cr_vol"), creator.clone()) }
/// Per-creator total released volume — persistent storage.
pub fn creator_stats_released_key(creator: &Address) -> (Symbol, Address) { (symbol_short!("cr_rel"), creator.clone()) }
/// Per-creator total refunded volume — persistent storage.
pub fn creator_stats_refunded_key(creator: &Address) -> (Symbol, Address) { (symbol_short!("cr_ref"), creator.clone()) }
/// Per-payer cooldown timestamp — persistent storage.
pub fn payer_cooldown_key(invoice_id: u64, payer: Address) -> (Symbol, u64, Address) { (symbol_short!("pyr_cd"), invoice_id, payer) }
/// Sliding-window payment timestamps for rate limiting — persistent storage.
pub fn payment_window_key(invoice_id: u64) -> (Symbol, u64) { (symbol_short!("pay_win"), invoice_id) }
/// Payment completion certificate — persistent storage.
pub fn cert_key(invoice_id: u64) -> (Symbol, u64) { (symbol_short!("cert"), invoice_id) }
/// Reminder entry — persistent storage.
pub fn reminder_key(invoice_id: u64, address: &Address) -> (Symbol, u64, Address) { (symbol_short!("rem"), invoice_id, address.clone()) }
/// Per-address pause exemption flag — persistent storage.
pub fn pause_exempt_key(address: &Address) -> (Symbol, Address) { (symbol_short!("p_exempt"), address.clone()) }
/// Creator volume cap (admin-set) — persistent storage.
pub fn creator_volume_cap_key(creator: &Address) -> (Symbol, Address) { (symbol_short!("cr_v_cap"), creator.clone()) }
/// Creator volume used — persistent storage.
pub fn creator_volume_used_key(creator: &Address) -> (Symbol, Address) { (symbol_short!("cr_v_use"), creator.clone()) }
/// Creator self-imposed spending limit — persistent storage.
pub fn creator_self_limit_key(creator: &Address) -> (Symbol, Address) { (symbol_short!("cr_slf_lim"), creator.clone()) }
/// Creator self-limit daily usage — persistent storage.
pub fn creator_self_used_key(creator: &Address) -> (Symbol, Address) { (symbol_short!("cr_slf_use"), creator.clone()) }
/// Creator self-limit last reset day — persistent storage.
pub fn creator_self_limit_day_key(creator: &Address) -> (Symbol, Address) { (symbol_short!("cr_slf_day"), creator.clone()) }
/// Creator self-limit pending raise request — persistent storage.
pub fn creator_self_limit_raise_key(creator: &Address) -> (Symbol, Address) { (symbol_short!("cr_slf_rse"), creator.clone()) }

// ---------------------------------------------------------------------------
// Issue #332: Recipient payout optimization
// ---------------------------------------------------------------------------

/// Contiguous Vec<Address> of all recipients for an invoice — persistent storage.
/// Replaces per-recipient storage keys so the full list can be loaded with a
/// single `env.storage().persistent().get()` call during release.
pub fn recipients_list_key(invoice_id: u64) -> (Symbol, u64) { (symbol_short!("rec_lst"), invoice_id) }

/// Contiguous Vec<i128> of amounts parallel to `recipients_list_key` — persistent storage.
pub fn amounts_list_key(invoice_id: u64) -> (Symbol, u64) { (symbol_short!("amt_lst"), invoice_id) }

/// Bit-vector (u32) of paid flags parallel to recipients list — persistent storage.
/// Bit N is set when recipients[N] has been paid. Supports up to 32 recipients per
/// compact flag word; a second word (`paid_flags_1`, etc.) would extend capacity.
pub fn paid_flags_key(invoice_id: u64) -> (Symbol, u64) { (symbol_short!("paid_flg"), invoice_id) }

// ---------------------------------------------------------------------------
// Issue #333: Payment milestone events
// ---------------------------------------------------------------------------

/// Bitmask (u8) of milestones already emitted for an invoice — instance storage.
/// Bit 0 = 25%, Bit 1 = 50%, Bit 2 = 75%, Bit 3 = 100%.
/// Stored in instance storage so it piggybacks on the instance-level TTL bump
/// that already happens on every pay() call, avoiding extra rent.
pub fn milestone_flags_key(invoice_id: u64) -> (Symbol, u64) { (symbol_short!("ms_flgs"), invoice_id) }

// ---------------------------------------------------------------------------
// Issue #334: Compact XDR storage
// ---------------------------------------------------------------------------

/// Compact status byte (u8) for an invoice — persistent storage (overlay).
/// 0 = Pending, 1 = Released, 2 = Refunded, 3 = Cancelled.
/// Replaces storing the full InvoiceStatus enum variant for minimal XDR cost.
pub fn compact_status_key(invoice_id: u64) -> (Symbol, u64) { (symbol_short!("cpt_sts"), invoice_id) }

/// Compact deadline stored as u32 ledger sequence — persistent storage.
/// Used when wall-clock time is not required (opt-in via compact_migrate).
pub fn compact_deadline_ledger_key(invoice_id: u64) -> (Symbol, u64) { (symbol_short!("cpt_dlg"), invoice_id) }

pub fn fallback_escrow_key(invoice_id: u64, recipient: Address) -> (Symbol, u64, Address) {
    (symbol_short!("fb_esc"), invoice_id, recipient)
}

pub fn plan_key(invoice_id: u64, payer: Address) -> (Symbol, u64, Address) {
    (symbol_short!("inst_pl"), invoice_id, payer)
}

pub fn fee_brackets_key() -> Symbol {
    symbol_short!("fee_brks")
}
// ---------------------------------------------------------------------------
// Issue #438: Invoice anonymity mode
// ---------------------------------------------------------------------------

/// Per-invoice anonymity mode flag — persistent storage.
pub fn anonymous_recipients_key(invoice_id: u64) -> (Symbol, u64) { (symbol_short!("anon_rec"), invoice_id) }

/// Recipient commitment hash for anonymous invoice — persistent storage.
/// Key: (invoice_id, recipient_index)
pub fn recipient_commitment_key(invoice_id: u64, index: u32) -> (Symbol, u64, u32) { (symbol_short!("rec_cmt"), invoice_id, index) }

// ---------------------------------------------------------------------------
// Issue #437: Recipient payout delay
// ---------------------------------------------------------------------------

/// Delayed payout record — persistent storage.
/// Key: (invoice_id, recipient)
pub fn delayed_payout_key(invoice_id: u64, recipient: &Address) -> (Symbol, u64, Address) { (symbol_short!("del_pay"), invoice_id, recipient.clone()) }

// ---------------------------------------------------------------------------
// Issue #436: Payment proof commitment
// ---------------------------------------------------------------------------

/// Rolling payment root hash for an invoice — persistent storage.
pub fn payment_root_key(invoice_id: u64) -> (Symbol, u64) { (symbol_short!("pay_root"), invoice_id) }

// ---------------------------------------------------------------------------
// Issue #435: Contract upgrade freeze
// ---------------------------------------------------------------------------

/// Contract upgrade freeze flag — instance storage.
pub fn upgrade_freeze_key() -> Symbol { symbol_short!("upg_frz") }

/// Contract upgrade checkpoint hash — instance storage.
pub fn upgrade_checkpoint_key() -> Symbol { symbol_short!("upg_ckpt") }

/// Issue #451: per-invoice required memo hash — persistent storage.
pub fn required_memo_hash_key(invoice_id: u64) -> (Symbol, u64) { (symbol_short!("req_memo"), invoice_id) }
/// Issue #452: per-invoice tags — persistent storage.
pub fn invoice_tags_key(invoice_id: u64) -> (Symbol, u64) { (symbol_short!("inv_tags"), invoice_id) }
