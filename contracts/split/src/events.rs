//! # Event naming convention
//!
//! All split-contracts events follow a consistent topic layout:
//! `(symbol_short!("split"), <action>, invoice_id?)`.
//!
//! - `<action>` is an 8-char symbol identifying the lifecycle stage
//!   (`created`, `paid`, `released`, `refunded`, `st_chg`, …).
//! - `invoice_id` is included as the third topic for per-invoice events
//!   so indexers can filter by invoice without inspecting event data.
//!
//! ## When to call `next_seq`
//!
//! Events that represent discrete, countable occurrences on a single invoice
//! should include an auto-incrementing `event_seq` (fetched via `next_seq`)
//! as the last field in the event data. This gives indexers a stable,
//! per-invoice ordering key. Do **not** call `next_seq` for:
//! - global/contract-level events with no `invoice_id`
//! - events that already contain a unique identifier (e.g. `action_id`,
//!   `milestone_number`, `new_id`)
//!
//! ## `symbol_short!` vs `Symbol::new`
//!
//! Prefer `symbol_short!("abbr")` for event action topics because they are
//! short, fixed strings. Use `Symbol::new(env, "LongName")` only when the
//! symbol exceeds the short-macro length limit or must be constructed
//! dynamically.

use crate::analytics::{self, UsageKind};
use crate::storage_keys::ev_seq_key;
use crate::types::{DisputeOutcome, FeeSplit, InvoicePhase, InvoiceStatus, OverfundingPolicy, RepScore, TimelockAction};
use soroban_sdk::{contracttype, symbol_short, Address, BytesN, Env, String, Symbol, Vec};

// ---------------------------------------------------------------------------
// Payment event details (issue #954)
// ---------------------------------------------------------------------------

/// Per-recipient breakdown of a payment, included in payment events so
/// indexers can attribute gross/fee/net amounts without re-deriving them.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentBreakdown {
    pub recipient: Address,
    pub gross: i128,
    pub fee: i128,
    pub net: i128,
}

/// Metadata carried alongside a payment event's breakdown.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentEventMetadata {
    pub invoice_id: u64,
    pub payer: Address,
    pub timestamp: u64,
    pub total: i128,
}

/// Full payload emitted for payment events: metadata plus the per-recipient
/// breakdown.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentEvent {
    pub metadata: PaymentEventMetadata,
    pub breakdown: Vec<PaymentBreakdown>,
}

// ---------------------------------------------------------------------------
// Refund reason tracking (issue #956)
// ---------------------------------------------------------------------------

/// Reason a refund was issued, emitted with refund events so indexers can
/// categorise refunds without inspecting contract state.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefundReason {
    Expired,
    Cancelled,
    Dispute,
    AdminOverride,
    Overpayment,
}

// ---------------------------------------------------------------------------
// Event sequence helper (per-invoice, temporary-storage counter)
// ---------------------------------------------------------------------------

/// Fetch and increment the per-invoice event sequence counter.
/// Lives in `storage::temporary` so it resets between transactions.
fn next_seq(env: &Env, invoice_id: u64) -> u64 {
    let key = ev_seq_key(invoice_id);
    let seq: u64 = env.storage().temporary().get(&key).unwrap_or(0) + 1;
    env.storage().temporary().set(&key, &seq);
    seq
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Existing events
// ---------------------------------------------------------------------------

/// Emitted when a new invoice is created.
/// Topics: (split, created, invoice_id)
/// Data: (creator, total, event_seq)
pub fn invoice_created(
    env: &Env,
    invoice_id: u64,
    creator: &Address,
    total: i128,
    cross_chain_ref: &Option<String>,
) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("created"), invoice_id),
        (creator.clone(), total, cross_chain_ref.clone(), event_seq),
    );
    analytics::record(env, UsageKind::InvoiceCreated, Some(creator), total);
}

/// Emitted at invoice creation when `forward_to` is configured, making
/// surplus-forwarding visible to indexers without waiting for a release.
/// Topics: (split, fwd_cfg, invoice_id)
/// Data: forward_to
pub fn forward_configured(env: &Env, invoice_id: u64, forward_to: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("fwd_cfg"), invoice_id),
        forward_to.clone(),
    );
}

/// Emitted when a payment is received toward an invoice.
/// Topics: (split, paid, invoice_id)
/// Data: (payer, amount, token, event_seq)
pub fn payment_received(env: &Env, invoice_id: u64, payer: &Address, amount: i128, token: &Address) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("paid"), invoice_id),
        (payer.clone(), amount, token.clone(), event_seq),
    );
    analytics::record(env, UsageKind::PaymentReceived, Some(payer), amount);
}

/// Issue #954: Emitted when a payment is received, carrying the full
/// per-recipient breakdown plus metadata.
/// Topics: (split, pay_dtl, invoice_id)
/// Data: PaymentEvent { metadata, breakdown }
pub fn payment_received_detailed(
    env: &Env,
    invoice_id: u64,
    payer: &Address,
    total: i128,
    breakdown: &Vec<PaymentBreakdown>,
) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("pay_dtl"), invoice_id),
        PaymentEvent {
            metadata: PaymentEventMetadata {
                invoice_id,
                payer: payer.clone(),
                timestamp: env.ledger().timestamp(),
                total,
            },
            breakdown: breakdown.clone(),
        },
    );
}

pub fn payment_committed(env: &Env, invoice_id: u64, payer: &Address, commit_ledger: u32) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("pay_cmt"), invoice_id),
        (payer.clone(), commit_ledger, event_seq),
    );
}

/// Emitted when a confidential (Pedersen-committed) payment is settled via
/// `reveal_confidential_payment`. Deliberately omits the amount — that is the
/// entire point of a confidential payment — even though the amount is visible
/// elsewhere on-chain after settlement (e.g. the token transfer, `funded`).
/// Topics: (split, conf_rev, invoice_id)
/// Data: (payer, event_seq)
pub fn confidential_payment_revealed(env: &Env, invoice_id: u64, payer: &Address) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("conf_rev"), invoice_id),
        (payer.clone(), event_seq),
    );
}

pub fn milestone_released(env: &Env, invoice_id: u64, milestone_bps: u32, amount_released: i128) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("mile_rel"),
            invoice_id,
        ),
        (milestone_bps, amount_released, event_seq),
    );
}

pub fn surplus_claimed(env: &Env, invoice_id: u64, payer: &Address, amount: i128) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("surplus"), invoice_id),
        (payer.clone(), amount, event_seq),
    );
}

/// Emitted when an invoice is fully funded and funds are released.
/// Topics: (split, released, invoice_id)
/// Data: (recipients, event_seq)
pub fn invoice_released(env: &Env, invoice_id: u64, recipients: &Vec<Address>) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("released"),
            invoice_id,
        ),
        (recipients.clone(), event_seq),
    );
    analytics::record(env, UsageKind::InvoiceReleased, None, 0);
}

/// Emitted when an invoice is refunded.
/// Topics: (split, refunded, invoice_id)
/// Data: (invoice_id, amount, event_seq)
pub fn invoice_refunded(env: &Env, invoice_id: u64, amount: i128) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("refunded"),
            invoice_id,
        ),
        (invoice_id, amount, event_seq),
    );
    analytics::record(env, UsageKind::InvoiceRefunded, None, amount);
}

/// Issue #956: Emitted when an invoice is refunded, carrying the reason.
/// Topics: (split, refunded, invoice_id)
/// Data: (invoice_id, amount, reason, event_seq)
pub fn invoice_refunded_with_reason(
    env: &Env,
    invoice_id: u64,
    amount: i128,
    reason: &RefundReason,
) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("refunded"),
            invoice_id,
        ),
        (invoice_id, amount, *reason, event_seq),
    );
    analytics::record(env, UsageKind::InvoiceRefunded, None, amount);
}

/// Emitted when a release-condition preimage is verified.
/// Topics: (split, cond_ok, invoice_id)
/// Data: preimage_hash
pub fn condition_verified(env: &Env, invoice_id: u64, preimage_hash: &BytesN<32>) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("cond_ok"), invoice_id),
        (preimage_hash.clone(), event_seq),
    );
}

/// Emitted when an invoice expires.
/// Topics: (split, expired, invoice_id)
/// Data: (deadline, funded, creator)
pub fn invoice_expired(env: &Env, invoice_id: u64, deadline: u64, funded: i128, creator: &Address) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("expired"),
            invoice_id,
        ),
        (deadline, funded, creator.clone()),
    );
}

/// Emitted when a recipient is added to an invoice whitelist.
/// Topics: (split, rcp_wl, invoice_id)
/// Data: address
pub fn recipient_whitelisted(env: &Env, invoice_id: u64, address: &Address) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("rcp_wl"), invoice_id),
        (address.clone(), event_seq),
    );
}

/// Emitted when a recipient is removed from an invoice whitelist.
/// Topics: (split, rcp_rl, invoice_id)
/// Data: address
pub fn recipient_removed_from_whitelist(env: &Env, invoice_id: u64, address: &Address) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("rcp_rl"), invoice_id),
        (address.clone(), event_seq),
    );
}

/// Emitted when rebate is accrued for a creator.
/// Topics: (split, rbt_acr, creator)
/// Data: (amount, tier_bps)
pub fn rebate_accrued(env: &Env, creator: &Address, amount: i128, tier_bps: u32) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("rbt_acr"),
            creator.clone(),
        ),
        (amount, tier_bps),
    );
}

/// Emitted once per payer when their refund is transferred.
/// Topics: (split, pay_ref, invoice_id)
/// Data: (payer, amount)
pub fn payer_refunded(env: &Env, invoice_id: u64, payer: &Address, amount: i128) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("pay_ref"), invoice_id),
        (payer.clone(), amount, event_seq),
    );
}

/// Emitted when a recipient is added to a pending invoice.
/// Topics: (split, add_rec, invoice_id)
/// Data: (recipient, amount)
pub fn recipient_added(env: &Env, invoice_id: u64, recipient: &Address, amount: i128) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("add_rec"), invoice_id),
        (recipient.clone(), amount, event_seq),
    );
}

/// Emitted when the creator adjusts recipient split amounts.
/// Topics: (split, adj_spl, invoice_id)
/// Data: creator
pub fn split_adjusted(env: &Env, invoice_id: u64, creator: &Address) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("adj_spl"), invoice_id),
        (creator.clone(), event_seq),
    );
}

/// Emitted when a recipient is removed and their share redistributed among
/// the remaining recipients (issue #423).
/// Topics: (split, rebalance, invoice_id)
/// Data: (removed_address, redistributed_amount)
pub fn recipients_rebalanced(
    env: &Env,
    invoice_id: u64,
    removed_address: &Address,
    redistributed_amount: i128,
) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("rebalance"),
            invoice_id,
        ),
        (removed_address.clone(), redistributed_amount, event_seq),
    );
}

/// Emitted when an invoice is archived to instance storage.
/// Topics: (split, archived, invoice_id)
/// Data: (invoice_id, event_seq)
pub fn invoice_archived(env: &Env, invoice_id: u64) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("archived"),
            invoice_id,
        ),
        (invoice_id, event_seq),
    );
}

/// Emitted when a delegate is assigned to an invoice.
/// Topics: (split, delegated, invoice_id)
/// Data: (delegate, event_seq)
pub fn delegate_set(env: &Env, invoice_id: u64, delegate: &Address) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("delegated"),
            invoice_id,
        ),
        (delegate.clone(), event_seq),
    );
}

/// Emitted when a delegate is revoked from an invoice.
/// Topics: (split, revoked, invoice_id)
/// Data: (revoker, ledger_sequence)
pub fn delegate_revoked(env: &Env, invoice_id: u64, revoker: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("revoked"), invoice_id),
        (revoker.clone(), env.ledger().sequence()),
    );
}

/// Emitted when an invoice is partially released.
/// Topics: (split, part_rel, invoice_id)
/// Data: (recipients, event_seq)
pub fn invoice_partially_released(env: &Env, invoice_id: u64, recipients: &Vec<Address>) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("part_rel"),
            invoice_id,
        ),
        (recipients.clone(), event_seq),
    );
}

/// Emitted when a payment reminder is triggered.
/// Topics: (split, reminder, invoice_id)
/// Data: who
pub fn payment_reminder(env: &Env, invoice_id: u64, who: &Address) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("reminder"),
            invoice_id,
        ),
        who.clone(),
    );
}

/// Emitted when a payment is matched via memo.
/// Topics: (split, matched, invoice_id)
/// Data: (payer, memo)
pub fn payment_matched(env: &Env, invoice_id: u64, memo: u64, payer: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("matched"), invoice_id),
        (memo, payer.clone()),
    );
}

/// Emitted when an invoice is cloned.
/// Topics: (cloned, source_id, new_id)
/// Data: ledger_sequence
pub fn invoice_cloned(env: &Env, source_id: u64, new_id: u64) {
    env.events().publish(
        (symbol_short!("cloned"), source_id, new_id),
        (env.ledger().sequence(),),
    );
}

/// Issue #926: Emitted when an invoice is partially cloned.
/// Topics: (split, part_cln, source_id)
/// Data: (new_id, recipient_count)
pub fn invoice_partially_cloned(env: &Env, source_id: u64, new_id: u64, recipient_count: u32) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("part_cln"), source_id),
        (new_id, recipient_count),
    );
}

/// Issue #927: Emitted when an archived invoice is restored.
/// Topics: (split, restored, invoice_id)
/// Data: (caller, ledger)
pub fn invoice_restored(env: &Env, invoice_id: u64, caller: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("restored"), invoice_id),
        (caller.clone(), env.ledger().sequence()),
    );
}

/// Issue #927: Emitted when an archived invoice is purged by the cleanup policy.
/// Topics: (split, arch_purg, invoice_id)
/// Data: ledger
pub fn invoice_archive_purged(env: &Env, invoice_id: u64) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("arch_purg"), invoice_id),
        env.ledger().sequence(),
    );
}

/// Issue #927: Emitted at the end of a scheduled archival sweep.
/// Topics: (split, arch_swp)
/// Data: (archived_count, cursor)
pub fn archival_sweep_completed(env: &Env, archived_count: u32, cursor: u64) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("arch_swp")),
        (archived_count, cursor),
    );
}

/// Issue #927: Emitted when the archival policy changes.
/// Topics: (split, arch_pol, admin)
/// Data: (enabled, retention_ledgers)
pub fn archival_policy_updated(env: &Env, admin: &Address, enabled: bool, retention_ledgers: u32) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("arch_pol"), admin.clone()),
        (enabled, retention_ledgers),
    );
}

/// Issue #928: Emitted on every recipient authentication state change.
/// Topics: (split, rcp_auth, invoice_id)
/// Data: (recipient, action, ledger)
pub fn recipient_auth_changed(env: &Env, invoice_id: u64, recipient: &Address, action: soroban_sdk::Symbol) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("rcp_auth"), invoice_id),
        (recipient.clone(), action, env.ledger().sequence()),
    );
}

/// Issue #929: Emitted when a reversible operation is recorded.
/// Topics: (split, rv_rec, invoice_id)
/// Data: (op_id, kind)
pub fn reversible_op_recorded(env: &Env, invoice_id: u64, op_id: u64, kind: crate::types::ReversibleOpKind) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("rv_rec"), invoice_id),
        (op_id, kind),
    );
}

/// Issue #929: Emitted when a reversible operation is undone.
/// Topics: (split, rv_undo, invoice_id)
/// Data: (op_id, caller, ledger)
pub fn reversible_op_undone(env: &Env, invoice_id: u64, op_id: u64, caller: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("rv_undo"), invoice_id),
        (op_id, caller.clone(), env.ledger().sequence()),
    );
}

/// Emitted when an invoice is paused.
/// Topics: (split, paused, invoice_id)
/// Data: (creator, reason, auto_resume_at)
pub fn invoice_paused(
    env: &Env,
    invoice_id: u64,
    creator: &Address,
    reason: &String,
    auto_resume_at: &Option<u64>,
) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("paused"), invoice_id),
        (creator.clone(), reason.clone(), *auto_resume_at),
    );
}

/// Emitted whenever an invoice's `frozen` flag transitions to true.
/// Topics: (split, frozen, invoice_id)
/// Data: (creator, ledger)
pub fn invoice_frozen(env: &Env, invoice_id: u64, creator: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("frozen"), invoice_id),
        (creator.clone(), env.ledger().sequence()),
    );
}

/// Emitted when an invoice is resumed.
/// Topics: (split, resumed, invoice_id)
/// Data: creator
pub fn invoice_resumed(env: &Env, invoice_id: u64, creator: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("resumed"), invoice_id),
        creator.clone(),
    );
}

/// Emitted when a paused invoice is automatically resumed because
/// `auto_resume_at` has passed (checked lazily on the next `pay()` call).
/// Distinct from `invoice_resumed`, which is only for manual `resume_invoice`.
/// Topics: (split, auto_res, invoice_id)
/// Data: auto_resume_at
pub fn invoice_auto_resumed(env: &Env, invoice_id: u64, auto_resume_at: u64) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("auto_res"), invoice_id),
        auto_resume_at,
    );
}

/// Emitted when an invoice is force resumed.
/// Topics: (split, forced, invoice_id)
/// Data: admin_addr
pub fn invoice_force_resumed(env: &Env, invoice_id: u64, admin_addr: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("forced"), invoice_id),
        admin_addr.clone(),
    );
}

/// Emitted when the per-invoice contributor allowlist gating is toggled on
/// (first entry added, list goes None -> Some) or off (last entry removed,
/// list goes Some -> None).
/// Topics: (split, al_tog, invoice_id)
/// Data: (creator, enabled)
pub fn contributor_allowlist_toggled(env: &Env, invoice_id: u64, creator: &Address, enabled: bool) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("al_tog"), invoice_id),
        (creator.clone(), enabled),
    );
}

/// Emitted when a pending payout is claimed by a recipient (issue #209).
/// Topics: (split, pend_pay, invoice_id)
/// Data: (recipient, amount)
pub fn pending_payout_claimed(env: &Env, invoice_id: u64, recipient: &Address, amount: i128) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("pend_pay"),
            invoice_id,
        ),
        (recipient.clone(), amount),
    );
}

/// Issue #410: emitted when an invoice is renewed.
pub fn invoice_renewed(env: &Env, old_id: u64, new_id: u64, carried_amount: i128) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("renewed"), old_id),
        (new_id, carried_amount),
    );
}

/// Issue #412: emitted when a payer rates a released invoice.
pub fn invoice_rated(env: &Env, invoice_id: u64, payer: &Address, score: u32) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("rated"), invoice_id),
        (payer.clone(), score),
    );
}

/// Issue #413: emitted when a payer hits the payment rate limit.
pub fn rate_limit_hit(env: &Env, invoice_id: u64, payer: &Address, next_allowed_ledger: u32) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("rl_hit"), invoice_id),
        (payer.clone(), next_allowed_ledger),
    );
}

pub fn nft_gate_set(env: &Env, contract: &Option<Address>, admin: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("nft_set")),
        (contract.clone(), admin.clone()),
    );
}

pub fn action_queued(env: &Env, action_id: u64, action: &TimelockAction, admin: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("tl_queue"), action_id),
        (action.clone(), admin.clone()),
    );
}

pub fn action_executed(env: &Env, action_id: u64, action: &TimelockAction) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("tl_exec"), action_id),
        action.clone(),
    );
}

pub fn action_cancelled(env: &Env, action_id: u64, action: &TimelockAction, admin: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("tl_cncl"), action_id),
        (action.clone(), admin.clone()),
    );
}

pub fn invoice_admin_frozen(env: &Env, invoice_id: u64, admin: &Address, reason: &String) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("adm_frz"), invoice_id),
        (admin.clone(), reason.clone()),
    );
}

pub fn invoice_admin_unfrozen(env: &Env, invoice_id: u64, admin: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("adm_unf"), invoice_id),
        admin.clone(),
    );
}

pub fn batch_archived(env: &Env, count: u32, ids: &Vec<u64>) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("bat_arc")),
        (count, ids.clone()),
    );
}

pub fn partial_refund_issued(
    env: &Env,
    invoice_id: u64,
    creator: &Address,
    bps: u32,
    amount: i128,
) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("prt_ref"), invoice_id),
        (creator.clone(), bps, amount),
    );
}

/// Issue #276: Emitted when cumulative platform volume crosses a milestone threshold.
/// Topics: (split, plt_v_ms, milestone_number)
/// Data: (total_volume, invoice_count, ledger)
pub fn platform_volume_milestone(
    env: &Env,
    total_volume: i128,
    invoice_count: u64,
    milestone_number: i128,
) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("plt_v_ms"),
            milestone_number,
        ),
        (total_volume, invoice_count, env.ledger().sequence()),
    );
}

/// Issue #276: Emitted when a creator's lifetime volume crosses a milestone threshold.
/// Topics: (split, cr_v_ms, creator)
/// Data: (total_volume, invoice_count, milestone_number, ledger)
pub fn creator_volume_milestone(
    env: &Env,
    creator: &Address,
    total_volume: i128,
    invoice_count: u64,
    milestone_number: i128,
) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("cr_v_ms"),
            creator.clone(),
        ),
        (
            total_volume,
            invoice_count,
            milestone_number,
            env.ledger().sequence(),
        ),
    );
}

/// Issue #297: Emitted when the circuit breaker is activated.
/// Topics: (split, cb_act)
/// Data: reason
pub fn circuit_breaker_activated(env: &Env, reason: &String) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("cb_act")),
        reason.clone(),
    );
}

/// Issue #297: Emitted when the circuit breaker is deactivated.
/// Topics: (split, cb_deact)
/// Data: ()
pub fn circuit_breaker_deactivated(env: &Env) {
    env.events()
        .publish((symbol_short!("split"), symbol_short!("cb_dact")), ());
}

/// Issue #925: Emitted when a payment batch is opened.
/// Topics: (split, pb_open, payer)
/// Data: (batch_id, token)
pub fn payment_batch_opened(env: &Env, batch_id: u64, payer: &Address, token: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("pb_open"), payer.clone()),
        (batch_id, token.clone()),
    );
}

/// Issue #925: Emitted when a payment batch is executed.
/// Topics: (split, pb_exec, payer)
/// Data: (batch_id, item_count, total_amount, savings_bps)
pub fn payment_batch_executed(
    env: &Env,
    batch_id: u64,
    payer: &Address,
    item_count: u32,
    total_amount: i128,
    savings_bps: u32,
) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("pb_exec"), payer.clone()),
        (batch_id, item_count, total_amount, savings_bps),
    );
}

/// Issue #925: Emitted when a payment batch is cancelled.
/// Topics: (split, pb_cncl, payer)
/// Data: batch_id
pub fn payment_batch_cancelled(env: &Env, batch_id: u64, payer: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("pb_cncl"), payer.clone()),
        batch_id,
    );
}

/// Issue #923: Emitted on every circuit breaker state transition.
/// Topics: (split, cb_state)
/// Data: (from, to, actor, reason, ledger)
pub fn circuit_breaker_state_changed(
    env: &Env,
    from: CircuitBreakerState,
    to: CircuitBreakerState,
    actor: &Address,
    reason: &String,
) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("cb_state")),
        (from, to, actor.clone(), reason.clone(), env.ledger().sequence()),
    );
}

/// Issue #923: Emitted when a failure is reported against the circuit breaker.
/// Topics: (split, cb_fail, reporter)
/// Data: (failure_count, failure_threshold)
pub fn circuit_breaker_failure_reported(
    env: &Env,
    reporter: &Address,
    failure_count: u32,
    failure_threshold: u32,
) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("cb_fail"),
            reporter.clone(),
        ),
        (failure_count, failure_threshold),
    );
}

/// Issue #923: Emitted when an operation is added to or removed from the exemption list.
/// Topics: (split, cb_exmpt)
/// Data: (function, exempt)
pub fn circuit_breaker_exemption_set(env: &Env, function: &Symbol, exempt: bool) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("cb_exmpt")),
        (function.clone(), exempt),
    );
}

/// Issue #296: Emitted when a fee waiver is granted to a creator.
/// Topics: (split, fw_grant, creator)
/// Data: ()
pub fn fee_waiver_granted(env: &Env, creator: &Address) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("fw_grant"),
            creator.clone(),
        ),
        (),
    );
}

/// Issue #296: Emitted when a fee waiver is revoked from a creator.
/// Topics: (split, fw_rev, creator)
/// Data: ()
pub fn fee_waiver_revoked(env: &Env, creator: &Address) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("fw_rev"),
            creator.clone(),
        ),
        (),
    );
}

/// Issue #922: Emitted when a fee waiver's expiry is set or extended.
/// Topics: (split, fw_exp, creator)
/// Data: expires_at (0 = never)
pub fn fee_waiver_expiry_set(env: &Env, creator: &Address, expires_at: u64) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("fw_exp"),
            creator.clone(),
        ),
        expires_at,
    );
}

/// Issue #922: Emitted when an expired fee waiver is purged from the whitelist.
/// Topics: (split, fw_purge, creator)
/// Data: expires_at
pub fn fee_waiver_expired(env: &Env, creator: &Address, expires_at: u64) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("fw_purge"),
            creator.clone(),
        ),
        expires_at,
    );
}

/// Issue #285: Emitted when fee tiers are updated.
/// Topics: (split, fee_tiers_updated)
/// Data: count of tiers
pub fn fee_tiers_updated(env: &Env, tier_count: u32) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("fee_upd")),
        tier_count,
    );
}

/// Issue #285: Emitted when a fee tier is applied at release time.
/// Topics: (split, fee_tier_applied, creator)
/// Data: (tier_index, fee_bps, creator_volume)
#[allow(dead_code)]
pub fn fee_tier_applied(
    env: &Env,
    creator: &Address,
    tier_index: u32,
    fee_bps: u32,
    creator_volume: u64,
) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("fee_app"),
            creator.clone(),
        ),
        (tier_index, fee_bps, creator_volume),
    );
}

/// Issue #299: Emitted when creator stats are updated.
/// Topics: (split, creator_stats_updated, creator)
/// Data: (total_invoices, total_raised, total_released, total_payers, avg_funding_time)
#[allow(dead_code)]
pub fn creator_stats_updated(
    env: &Env,
    creator: &Address,
    total_invoices: u32,
    total_raised: u64,
    total_released: u64,
    total_payers: u32,
    avg_funding_time_ledgers: u32,
) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("stats_upd"),
            creator.clone(),
        ),
        (
            total_invoices,
            total_raised,
            total_released,
            total_payers,
            avg_funding_time_ledgers,
        ),
    );
}

/// Issue #283: Unified state transition event emitted on every invoice status change.
///
/// # Indexer Guide
/// Indexers can reconstruct the full invoice lifecycle by filtering events with
/// topic[1] == "st_chg". Each event carries:
///   - `from`: the previous status (as a Symbol: "none", "pending", "released", "refunded", "cancelled")
///   - `to`: the new status (same encoding)
///   - `actor`: the address that triggered the transition
///   - `ledger`: the ledger sequence number at transition time
///
/// To build a per-invoice state machine, collect all "st_chg" events for a given
/// `invoice_id` ordered by ledger, then replay `from -> to` pairs.
///
/// Topics: (split, st_chg, invoice_id)
/// Data: (from_status, to_status, actor, ledger)
pub fn invoice_state_changed(
    env: &Env,
    invoice_id: u64,
    from_status: Option<&InvoiceStatus>,
    to_status: &InvoiceStatus,
    actor: &Address,
) {
    let from_sym = match from_status {
        None => symbol_short!("none"),
        Some(InvoiceStatus::Pending) => symbol_short!("pending"),
        Some(InvoiceStatus::Released) => symbol_short!("released"),
        Some(InvoiceStatus::Refunded) => symbol_short!("refunded"),
        Some(InvoiceStatus::Expired) => symbol_short!("expired"),
        Some(InvoiceStatus::Cancelled) => symbol_short!("cancld"),
        Some(InvoiceStatus::Disputed) => symbol_short!("disputed"),
        Some(InvoiceStatus::PartiallyReleased) => symbol_short!("part_rel"),
        Some(InvoiceStatus::Finalised) => symbol_short!("finald"),
        Some(InvoiceStatus::Deleted) => symbol_short!("deleted"),
        Some(InvoiceStatus::PayoutInProgress) => symbol_short!("pay_prg"),
    };
    let to_sym = match to_status {
        InvoiceStatus::Pending => symbol_short!("pending"),
        InvoiceStatus::Released => symbol_short!("released"),
        InvoiceStatus::Refunded => symbol_short!("refunded"),
        InvoiceStatus::Expired => symbol_short!("expired"),
        InvoiceStatus::Cancelled => symbol_short!("cancld"),
        InvoiceStatus::Disputed => symbol_short!("disputed"),
        InvoiceStatus::PartiallyReleased => symbol_short!("part_rel"),
        InvoiceStatus::Finalised => symbol_short!("finald"),
        InvoiceStatus::Deleted => symbol_short!("deleted"),
        InvoiceStatus::PayoutInProgress => symbol_short!("pay_prg"),
    };
    env.events().publish(
        (symbol_short!("split"), symbol_short!("st_chg"), invoice_id),
        (from_sym, to_sym, actor.clone(), env.ledger().sequence()),
    );
}

/// Issue #309: Emitted when a payer is added/removed from an invoice's allowlist.
/// Topics: (split, al_upd, invoice_id)
/// Data: (creator, payer, added)
pub fn allowlist_updated(
    env: &Env,
    invoice_id: u64,
    creator: &Address,
    payer: &Address,
    added: bool,
) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("al_upd"), invoice_id),
        (creator.clone(), payer.clone(), added),
    );
}

/// Emitted when a creator clears an invoice's entire payer allowlist,
/// transitioning it from restricted to open (allowed_payers set to None).
/// Topics: (split, al_open, invoice_id)
/// Data: (creator, ledger)
pub fn allowlist_removed(env: &Env, invoice_id: u64, creator: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("al_open"), invoice_id),
        (creator.clone(), env.ledger().sequence()),
    );
}

/// Issue #308: Emitted when a payer claims their per-payer refund.
/// Topics: (split, ref_clm, invoice_id)
/// Data: (payer, amount)
pub fn refund_claimed(env: &Env, invoice_id: u64, payer: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("ref_clm"), invoice_id),
        (payer.clone(), amount),
    );
}

/// Issue #956: Emitted when a payer claims their per-payer refund, carrying
/// the reason the refund was issued.
/// Topics: (split, ref_clm, invoice_id)
/// Data: (payer, amount, reason)
pub fn refund_claimed_with_reason(
    env: &Env,
    invoice_id: u64,
    payer: &Address,
    amount: i128,
    reason: &RefundReason,
) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("ref_clm"), invoice_id),
        (payer.clone(), amount, *reason),
    );
}

/// Issue #310: Emitted when an upgrade is proposed via the timelock.
/// Topics: (split, upg_prop)
/// Data: (new_wasm_hash, eligible_at)
pub fn upgrade_proposed(env: &Env, new_wasm_hash: &soroban_sdk::BytesN<32>, eligible_at: u64) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("upg_prop")),
        (new_wasm_hash.clone(), eligible_at),
    );
}

/// Issue #310: Emitted when a pending upgrade is executed.
/// Topics: (split, upg_exec)
/// Data: new_wasm_hash
pub fn upgrade_executed(env: &Env, new_wasm_hash: &soroban_sdk::BytesN<32>) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("upg_exec")),
        new_wasm_hash.clone(),
    );
}

/// Issue #310: Emitted when a pending upgrade proposal is cancelled.
/// Topics: (split, upg_cncl)
/// Data: admin
pub fn upgrade_cancelled(env: &Env, admin: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("upg_cncl")),
        admin.clone(),
    );
}

/// Issue #328: Emitted when the contract is emergency-paused by an admin.
/// Topics: (split, ct_paused)
/// Data: (admin, ledger)
pub fn contract_paused(env: &Env, admin: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("ct_paused")),
        (admin.clone(), env.ledger().sequence()),
    );
}

/// Issue #328: Emitted when the contract is unpaused by an admin.
/// Topics: (split, ct_unpsd)
/// Data: (admin, ledger)
pub fn contract_unpaused(env: &Env, admin: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("ct_unpsd")),
        (admin.clone(), env.ledger().sequence()),
    );
}

/// Issue #327: Emitted when funds become releasable after the time-lock delay expires.
/// Topics: (split, fnd_unlk, invoice_id)
/// Data: unlock_ledger
pub fn funds_unlocked(env: &Env, invoice_id: u64, unlock_ledger: u32) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("fnd_unlk"),
            invoice_id,
        ),
        unlock_ledger,
    );
}

/// Issue #329: Emitted when a creator updates an invoice's off-chain metadata hash.
/// Topics: (split, meta_upd, invoice_id)
/// Data: (old_hash, new_hash, ledger)
pub fn metadata_updated(
    env: &Env,
    invoice_id: u64,
    old_hash: &Option<BytesN<32>>,
    new_hash: &BytesN<32>,
) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("meta_upd"),
            invoice_id,
        ),
        (old_hash.clone(), new_hash.clone(), env.ledger().sequence()),
    );
}

/// Issue #330: Emitted when a single recipient is paid via release_to_recipient.
/// Topics: (split, rec_paid, invoice_id)
/// Data: (recipient, amount, ledger)
pub fn recipient_paid(env: &Env, invoice_id: u64, recipient: &Address, amount: i128) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("rec_paid"),
            invoice_id,
        ),
        (recipient.clone(), amount, env.ledger().sequence()),
    );
}

/// Issue #333: Emitted when an invoice crosses a funding milestone (25%, 50%, 75%, 100%).
///
/// # Indexer Guide
/// Indexers can subscribe to milestone crossings by filtering events with
/// topic[1] == "milestone" (optionally narrowed further by topic[2] == invoice_id). Each
/// event carries:
///   - `milestone_bps`: the crossed threshold in basis points relative to the invoice total —
///     2500 = 25%, 5000 = 50%, 7500 = 75%, 10000 = 100%.
///   - `funded_amount`: the invoice's cumulative funded amount at the moment the threshold
///     was crossed (in the invoice's payment token's base units).
///   - `ledger`: the ledger sequence number at which the crossing was recorded.
///
/// Multiple events can be emitted in a single `pay()` call when a large payment
/// crosses several thresholds at once — do not assume one event per payment; instead
/// group by `invoice_id` and treat each `milestone_bps` as an independent crossing.
///
/// Topics: (split, milestone, invoice_id)
/// Data: (milestone_bps, funded_amount, ledger)
pub fn milestone_reached(env: &Env, invoice_id: u64, milestone_bps: u32, funded_amount: i128) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("milestone"),
            invoice_id,
        ),
        (milestone_bps, funded_amount, env.ledger().sequence()),
    );
}

/// Emitted when an invoice crosses an admin-configured funding checkpoint.
///
/// Checkpoints are contract-level thresholds expressed in basis points where
/// `10_000 = 100%`. A single payment can emit multiple checkpoint events when it
/// crosses several configured thresholds at once.
///
/// # Indexer Guide
/// Filter events with topic[1] == "fnd_chk" (topic[2] is the `invoice_id`, so narrow to a
/// single invoice by matching that topic too). Unlike `milestone_reached`, whose thresholds
/// are the fixed 25/50/75/100% set, `funding_checkpoint` thresholds are admin-configurable,
/// so `threshold_bps` must always be read from the event data rather than assumed. The
/// event's `FundingCheckpoint` payload carries:
///   - `invoice_id`: redundant with topic[2], included in the data for convenience so the
///     event can be decoded without also decoding topics.
///   - `threshold_bps`: the configured checkpoint that was crossed, in basis points of the
///     invoice total (`10_000 = 100%`).
///   - `funded`: the invoice's cumulative funded amount at the moment of crossing.
///   - `total`: the invoice's total amount, i.e. `funded / total` (scaled to bps) is
///     approximately `threshold_bps` at the instant the event fires.
///
/// As with `milestone_reached`, a single payment may cross several configured checkpoints,
/// emitting one event per checkpoint — group by `invoice_id` and treat each as independent.
///
/// Topics: (split, fnd_chk, invoice_id)
/// Data: FundingCheckpoint { invoice_id, threshold_bps, funded, total }
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FundingCheckpoint {
    pub invoice_id: u64,
    pub threshold_bps: u32,
    pub funded: i128,
    pub total: i128,
}

pub fn funding_checkpoint(
    env: &Env,
    invoice_id: u64,
    threshold_bps: u32,
    funded: i128,
    total: i128,
) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("fnd_chk"), invoice_id),
        FundingCheckpoint {
            invoice_id,
            threshold_bps,
            funded,
            total,
        },
    );
}

/// Issue #315: Emitted when a delegated payment is executed.
/// Topics: (split, dlgt_pay, invoice_id)
/// Data: (payer, executor, amount, ledger)
pub fn delegated_payment(
    env: &Env,
    invoice_id: u64,
    payer: &Address,
    executor: &Address,
    amount: i128,
) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("dlgt_pay"),
            invoice_id,
        ),
        (
            payer.clone(),
            executor.clone(),
            amount,
            env.ledger().sequence(),
        ),
    );
}

/// Issue #325: Emitted when a payer raises a dispute.
/// Topics: (split, disp_rsd, invoice_id)
/// Data: (payer, reason_hash, ledger)
pub fn dispute_raised(env: &Env, invoice_id: u64, payer: &Address, reason_hash: &BytesN<32>) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("disp_rsd"),
            invoice_id,
        ),
        (payer.clone(), reason_hash.clone(), env.ledger().sequence()),
    );
}

/// Issue #325: Emitted when admin resolves a dispute.
/// Topics: (split, disp_res, invoice_id)
/// Data: (admin, outcome, ledger)
pub fn dispute_resolved(env: &Env, invoice_id: u64, admin: &Address, outcome: &DisputeOutcome) {
    let outcome_sym = match outcome {
        DisputeOutcome::Approved => symbol_short!("approved"),
        DisputeOutcome::Refunded => symbol_short!("refunded"),
        DisputeOutcome::Release => symbol_short!("release"),
        DisputeOutcome::Refund => symbol_short!("refund"),
    };
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("disp_res"),
            invoice_id,
        ),
        (admin.clone(), outcome_sym, env.ledger().sequence()),
    );
}

/// Issue #325: Emitted when a dispute auto-expires and funds are released.
/// Topics: (split, disp_exp, invoice_id)
/// Data: ledger
pub fn dispute_expired(env: &Env, invoice_id: u64) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("disp_exp"),
            invoice_id,
        ),
        env.ledger().sequence(),
    );
}

/// Issue #521: Emitted when the fee recipients list is updated.
pub fn fee_recipients_updated(env: &Env, recipients: &Vec<FeeSplit>) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("fee_rc_up"),
        ),
        (recipients.clone(),),
    );
}

/// Issue #326: Emitted when a protocol fee is paid to treasury on release.
///
/// Topics: (split, fee_paid, invoice_id)
/// Data: (amount, treasury, ledger)
pub fn fee_paid(env: &Env, invoice_id: u64, amount: i128, treasury: &Address) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("fee_paid"),
            invoice_id,
        ),
        (amount, treasury.clone(), env.ledger().sequence()),
    );
}

/// Emitted when an oracle-priced invoice fetches a fresh rate at payment time.
/// `rate` is the raw price returned by the oracle (USD cents per 1 whole token,
/// scaled by `ORACLE_RATE_SCALE`); `computed_amount` is the resulting required
/// token total derived from the invoice's fixed USD-cents target.
///
/// Topics: (split, orc_pf, invoice_id)
/// Data: (rate, computed_amount, ledger)
pub fn oracle_price_fetched(env: &Env, invoice_id: u64, rate: i128, computed_amount: i128) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("orc_pf"), invoice_id),
        (rate, computed_amount, env.ledger().sequence()),
    );
}

/// Emitted when a single graduated tranche is released via `release_tranche()`.
/// Topics: (split, tr_rel, invoice_id)
/// Data: (tranche_index, amount, ledger)
pub fn tranche_released(env: &Env, invoice_id: u64, tranche_index: u32, amount: i128) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("tr_rel"), invoice_id),
        (tranche_index, amount, env.ledger().sequence()),
    );
}

/// Issue #349: Emitted when an address's reputation score is updated.
/// Topics: (split, rep_upd, address)
/// Data: (score_struct, computed_score)
///
/// # Computed score formula
///
/// The `computed_score` is a single `u32` summary of the raw `RepScore`
/// counters. The formula rewards consistent on-time payment and successful
/// invoice completion while penalising late payments and refunds:
///
/// ```text
/// base        = paid_on_time * 10 + invoices_released * 5
/// deductions  = late_pays * 5 + invoices_refunded * 2
/// computed    = base.saturating_sub(deductions)
/// ```
///
/// Indexers are encouraged to use `computed_score` directly rather than
/// re-implementing the formula off-chain.
pub fn rep_updated(env: &Env, address: &Address, score: &RepScore, computed_score: u32) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("rep_upd"),
            address.clone(),
        ),
        (score.clone(), computed_score),
    );
}

/// Issue #504: Emitted when a payout transfer fails for a recipient during batch release.
/// Topics: (split, PayoutFailed, invoice_id)
/// Data: (recipient, amount, reason)
pub fn payout_failed(env: &Env, invoice_id: u64, recipient: &Address, amount: i128, reason: &String) {
    env.events().publish(
        (
            symbol_short!("split"),
            soroban_sdk::Symbol::new(env, "PayoutFailed"),
            invoice_id,
        ),
        (recipient.clone(), amount, reason.clone()),
    );
}

pub fn instalment_tranche_paid(
    env: &Env,
    invoice_id: u64,
    payer: &Address,
    tranche_index: u32,
    amount: i128,
) {
    env.events().publish(
        (
            symbol_short!("split"),
            soroban_sdk::Symbol::new(env, "InstalmentTranchePaid"),
            invoice_id,
        ),
        (payer.clone(), tranche_index, amount),
    );
}

pub fn escrow_hold_started(env: &Env, invoice_id: u64, held_until: u32) {
    env.events().publish(
        (
            symbol_short!("split"),
            soroban_sdk::Symbol::new(env, "EscrowHoldStarted"),
            invoice_id,
        ),
        held_until,
    );
}

pub fn escrow_resolved(env: &Env, invoice_id: u64, resolution_hash: &BytesN<32>) {
    env.events().publish(
        (
            symbol_short!("split"),
            soroban_sdk::Symbol::new(env, "EscrowResolved"),
            invoice_id,
        ),
        resolution_hash.clone(),
    );
}

/// Issue #437: Emitted when a delayed payout is scheduled on release.
/// Topics: (split, dlypay_s, invoice_id)
/// Data: (recipient, claimable_at_ledger)
#[allow(dead_code)]
pub fn delayed_payout_scheduled(
    env: &Env,
    invoice_id: u64,
    recipient: &Address,
    claimable_at: u32,
) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("dlypay_s"),
            invoice_id,
        ),
        (recipient.clone(), claimable_at),
    );
}

/// Issue #437: Emitted when a delayed payout is claimed by the recipient.
/// Topics: (split, dlypay_c, invoice_id)
/// Data: (recipient, amount)
pub fn delayed_payout_claimed(env: &Env, invoice_id: u64, recipient: &Address, amount: i128) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("dlypay_c"),
            invoice_id,
        ),
        (recipient.clone(), amount),
    );
}

/// Issue #435: Emitted when the contract is frozen for upgrade.
/// Topics: (split, upg_frz, ())
/// Data: (checkpoint_hash, frozen_at_ledger)
pub fn contract_frozen_for_upgrade(env: &Env, checkpoint_hash: &BytesN<32>) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("upg_frz"),
            symbol_short!(""),
        ),
        (checkpoint_hash.clone(), env.ledger().sequence()),
    );
}

/// Issue #435: Emitted when the contract is thawed (upgrade freeze removed).
/// Topics: (split, upg_thw, ())
/// Data: admin
pub fn contract_thawed(env: &Env, admin: &Address) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("upg_thw"),
            symbol_short!(""),
        ),
        admin.clone(),
    );
}

/// Issue #431: Emitted when a duplicate payment is detected and rejected.
/// Topics: (split, dup_pay, invoice_id)
/// Data: (payer, amount, fingerprint_hash)
#[allow(dead_code)]
pub fn duplicate_payment_rejected(
    env: &Env,
    invoice_id: u64,
    payer: &Address,
    amount: i128,
    fingerprint: &BytesN<32>,
) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("dup_pay"), invoice_id),
        (payer.clone(), amount, fingerprint.clone()),
    );
}

/// Issue #432: Emitted when a referrer receives a reward share.
/// Topics: (split, ref_rwd, invoice_id)
/// Data: (referrer, amount)
#[allow(dead_code)]
pub fn referrer_rewarded(env: &Env, invoice_id: u64, referrer: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("ref_rwd"), invoice_id),
        (referrer.clone(), amount),
    );
}

/// Issue #434: Emitted when a group member expires unfunded.
/// Topics: (split, grp_exp, invoice_id)
/// Data: group_id
#[allow(dead_code)]
pub fn group_member_expired(env: &Env, invoice_id: u64, group_id: u64) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("grp_exp"), invoice_id),
        group_id,
    );
}

/// Issue #434: Emitted when a group rollback is triggered.
/// Topics: (split, grp_roll, group_id)
/// Data: member_count
#[allow(dead_code)]
pub fn group_rollback_triggered(env: &Env, group_id: u64, member_count: u32) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("grp_roll"), group_id),
        member_count,
    );
}

/// Issue #489: Emitted when a contribution qualifies for the early-bird discounted
/// platform fee.
/// Topics: (split, ebird_pay, invoice_id)
/// Data: (payer, discount_amount)
pub fn early_bird_payment(env: &Env, invoice_id: u64, payer: &Address, discount_amount: i128) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("ebird_pay"), invoice_id),
        (payer.clone(), discount_amount),
    );
}

/// Issue #439: Emitted when a creator's cancellation cooldown is set.
/// Topics: (split, cr_cool, creator)
/// Data: (until_ledger, cooldown_ledgers)
pub fn creator_cooldown_set(
    env: &Env,
    creator: &Address,
    until_ledger: u64,
    cooldown_ledgers: u64,
) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("cr_cool"),
            creator.clone(),
        ),
        (until_ledger, cooldown_ledgers),
    );
}

/// Emitted when admin sweeps unclaimed failed-payout funds to treasury.
/// Topics: (split, swept, invoice_id)
/// Data: (amount, treasury)
pub fn funds_swept(env: &Env, invoice_id: u64, amount: i128, treasury: &Address) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("swept"),
            invoice_id,
        ),
        (amount, treasury.clone()),
    );
}

/// Emitted when a trusted caller is added to the whitelist.
/// Topics: (split, tc_add, caller)
/// Data: ()
pub fn trusted_caller_added(env: &Env, caller: &Address) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("tc_add"),
            caller.clone(),
        ),
        (),
    );
}

/// Emitted when a trusted caller is removed from the whitelist.
/// Topics: (split, tc_rem, caller)
/// Data: ()
pub fn trusted_caller_removed(env: &Env, caller: &Address) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("tc_rem"),
            caller.clone(),
        ),
        (),
    );
}

/// RBAC: Emitted when an admin grants a role to an address.
/// Topics: (split, role_grt, grantee)
/// Data: (role_discriminant, admin)
#[allow(dead_code)]
pub fn role_granted(env: &Env, grantee: &Address, role_discriminant: u32, admin: &Address) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("role_grt"),
            grantee.clone(),
        ),
        (role_discriminant, admin.clone()),
    );
}

/// RBAC: Emitted when an admin revokes a role from an address.
/// Topics: (split, role_rev, grantee)
/// Data: (role_discriminant, admin)
#[allow(dead_code)]
pub fn role_revoked(env: &Env, grantee: &Address, role_discriminant: u32, admin: &Address) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("role_rev"),
            grantee.clone(),
        ),
        (role_discriminant, admin.clone()),
    );
}

/// Issue #474: Emitted when a creator cancels an open invoice and all contributors are refunded.
/// Topics: (split, inv_cncl, invoice_id)
/// Data: (creator, total_refunded, ledger)
#[allow(dead_code)]
pub fn invoice_cancelled(env: &Env, invoice_id: u64, creator: &Address, total_refunded: i128) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("inv_cncl"),
            invoice_id,
        ),
        (creator.clone(), total_refunded, env.ledger().sequence()),
    );
}

/// Issue #475: Emitted when a multi-sig admin action is proposed.
/// Topics: (split, adm_prop, action_hash)
/// Data: (proposer, ledger)
pub fn admin_action_proposed(env: &Env, action_hash: &BytesN<32>, proposer: &Address) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("adm_prop"),
            action_hash.clone(),
        ),
        (proposer.clone(), env.ledger().sequence()),
    );
}

/// Issue #475: Emitted when a signer approves a pending admin action.
/// Topics: (split, adm_appr, action_hash)
/// Data: (approver, approval_count, ledger)
pub fn admin_action_approved(
    env: &Env,
    action_hash: &BytesN<32>,
    approver: &Address,
    approval_count: u32,
) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("adm_appr"),
            action_hash.clone(),
        ),
        (approver.clone(), approval_count, env.ledger().sequence()),
    );
}

/// Issue #470: Emitted when an overpayment results in a partial refund.
/// Topics: (split, RefundIssued, invoice_id)
/// Data: (payer, refund_amount)
pub fn refund_issued(env: &Env, invoice_id: u64, payer: &Address, refund_amount: i128) {
    env.events().publish(
        (
            symbol_short!("split"),
            soroban_sdk::Symbol::new(env, "RefundIssued"),
            invoice_id,
        ),
        (payer.clone(), refund_amount),
    );
}

/// Issue #956: Emitted when a partial refund is issued, carrying the reason.
/// Topics: (split, RefundIssued, invoice_id)
/// Data: (payer, refund_amount, reason)
pub fn refund_issued_with_reason(
    env: &Env,
    invoice_id: u64,
    payer: &Address,
    refund_amount: i128,
    reason: &RefundReason,
) {
    env.events().publish(
        (
            symbol_short!("split"),
            soroban_sdk::Symbol::new(env, "RefundIssued"),
            invoice_id,
        ),
        (payer.clone(), refund_amount, *reason),
    );
}

/// Issue #471: Emitted when a recipient rotates their payout address.
/// Topics: (split, RecipientAddressRotated, invoice_id)
/// Data: (old_address, new_address)
pub fn recipient_address_rotated(
    env: &Env,
    invoice_id: u64,
    old_address: &Address,
    new_address: &Address,
) {
    env.events().publish(
        (
            symbol_short!("split"),
            soroban_sdk::Symbol::new(env, "RecipientAddressRotated"),
            invoice_id,
        ),
        (old_address.clone(), new_address.clone()),
    );
}

/// Issue #475: Emitted when a multi-sig admin action reaches threshold and executes.
/// Topics: (split, adm_exec, action_hash)
/// Data: ledger
pub fn admin_action_executed(env: &Env, action_hash: &BytesN<32>) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("adm_exec"),
            action_hash.clone(),
        ),
        env.ledger().sequence(),
    );
}

/// Issue #476: Emitted when a creator stores a new reusable invoice template.
/// Topics: (split, tmpl_crt, creator)
/// Data: (template_id, ledger)
pub fn template_created(env: &Env, creator: &Address, template_id: u64) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("tmpl_crt"),
            creator.clone(),
        ),
        (template_id, env.ledger().sequence()),
    );
}

/// Issue #476: Emitted when a creator deletes a stored invoice template.
/// Topics: (split, tmpl_del, creator)
/// Data: (template_id, ledger)
pub fn template_deleted(env: &Env, creator: &Address, template_id: u64) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("tmpl_del"),
            creator.clone(),
        ),
        (template_id, env.ledger().sequence()),
    );
}

/// Issue #476: Emitted when an invoice is instantiated from a template.
/// Topics: (split, tmpl_inv, invoice_id)
/// Data: (creator, template_id, ledger)
pub fn invoice_from_template(env: &Env, invoice_id: u64, creator: &Address, template_id: u64) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("tmpl_inv"),
            invoice_id,
        ),
        (creator.clone(), template_id, env.ledger().sequence()),
    );
}

/// Emitted when a co-creator is added to an invoice.
/// Topics: (split, co_creatr_add, invoice_id)
/// Data: (creator, co_creator, ledger)
pub fn co_creator_added(env: &Env, invoice_id: u64, creator: &Address, co_creator: &Address) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("co_c_add"),
            invoice_id,
        ),
        (creator.clone(), co_creator.clone(), env.ledger().sequence()),
    );
}

/// Emitted when a co-creator is removed from an invoice.
/// Topics: (split, co_creatr_rem, invoice_id)
/// Data: (creator, co_creator, ledger)
pub fn co_creator_removed(env: &Env, invoice_id: u64, creator: &Address, co_creator: &Address) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("co_c_rem"),
            invoice_id,
        ),
        (creator.clone(), co_creator.clone(), env.ledger().sequence()),
    );
}

/// Emitted when a payer hits the global spending cap for the current ledger window.
/// Topics: (split, pay_spend_lim, payer)
/// Data: (window_total, cap, ledger)
pub fn payer_spend_limit_reached(
    env: &Env,
    payer: &Address,
    window_total: i128,
    cap: i128,
) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("pay_sp_lm"),
            payer.clone(),
        ),
        (window_total, cap, env.ledger().sequence()),
    );
}

/// Issue #XXX: Emitted when a contributor raises an invoice dispute.
/// Topics: (split, inv_disp_rsd, invoice_id)
/// Data: (disputer, reason_hash, ledger)
pub fn invoice_dispute_raised(
    env: &Env,
    invoice_id: u64,
    disputer: &Address,
    reason_hash: &BytesN<32>,
) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("inv_d_rsd"),
            invoice_id,
        ),
        (disputer.clone(), reason_hash.clone(), env.ledger().sequence()),
    );
}

/// Emitted on every individual cosigner approval recorded via `approve_release`.
/// Topics: (split, CosignerApproved, invoice_id)
/// Data: (cosigner, approvals_so_far, ledger)
pub fn cosigner_approved(env: &Env, invoice_id: u64, cosigner: &Address, approvals_so_far: u32) {
    env.events().publish(
        (
            symbol_short!("split"),
            soroban_sdk::Symbol::new(env, "CosignerApproved"),
            invoice_id,
        ),
        (cosigner.clone(), approvals_so_far, env.ledger().sequence()),
    );
}

/// Emitted once, the moment `approve_release` collects enough approvals to
/// meet the invoice's configured `cosigner_threshold`.
/// Topics: (split, CosignerThresholdReached, invoice_id)
/// Data: ledger
pub fn cosigner_threshold_reached(env: &Env, invoice_id: u64) {
    env.events().publish(
        (
            symbol_short!("split"),
            soroban_sdk::Symbol::new(env, "CosignerThresholdReached"),
            invoice_id,
        ),
        env.ledger().sequence(),
    );
}


/// Issue #503: Emitted when admin updates the per-creator open-invoice cap.
/// Topics: (INVOICE_LIMIT_UPDATED_V, split, inv_lim)
/// Data: new_limit
pub fn invoice_limit_updated(env: &Env, new_limit: u32) {
    env.events().publish(
        (
            INVOICE_LIMIT_UPDATED_V,
            symbol_short!("split"),
            symbol_short!("inv_lim"),
        ),
        new_limit,
    );
}

/// Issue #505: Emitted when a payout recipient account does not exist on-ledger.
/// Topics: (RECIPIENT_ACCOUNT_MISSING_V, split, rcp_mis, invoice_id)
/// Data: recipient
pub fn recipient_account_missing(env: &Env, invoice_id: u64, recipient: &Address) {
    env.events().publish(
        (
            RECIPIENT_ACCOUNT_MISSING_V,
            symbol_short!("split"),
            symbol_short!("rcp_mis"),
            invoice_id,
        ),
        recipient.clone(),
    );
}

// ---------------------------------------------------------------------------
// #522 — Cross-Invoice Split Linkage
// ---------------------------------------------------------------------------

/// Emitted on the first successful release of a child invoice, once the
/// parent is confirmed to be finalised.
///
/// Topics: `("child_unblk", child_id)`
/// Data:   `parent_id`
#[allow(dead_code)]
pub fn child_invoice_unblocked(env: &Env, child_id: u64, parent_id: u64) {
    env.events().publish(
        (symbol_short!("chld_ubl"), child_id),
        parent_id,
    );
}

// ---------------------------------------------------------------------------
// #523 — Late Payment Penalty Fee
// ---------------------------------------------------------------------------

/// Emitted every time a late-payment penalty is charged.
///
/// Topics: `("late_pen", invoice_id)`
/// Data:   `(invoice_id, payer, penalty_amount)`
///
/// `invoice_id` is included in both the topics (for indexer filtering) and
/// the data payload (for data-only decoders that do not inspect topics).
#[allow(dead_code)]
pub fn late_payment_penalty_charged(
    env: &Env,
    invoice_id: u64,
    payer: &Address,
    penalty_amount: i128,
) {
    env.events().publish(
        (symbol_short!("late_pen"), invoice_id),
        (invoice_id, payer.clone(), penalty_amount),
    );
}

// ---------------------------------------------------------------------------
// #524 — Invoice Batch Creation
// ---------------------------------------------------------------------------

/// Emitted once after a successful `batch_create_invoices` call, carrying
/// all newly created invoice IDs in creation order.
///
/// Topics: `("batch_crt",)`
/// Data:   `ids: Vec<u64>`
#[allow(dead_code)]
pub fn batch_invoice_created(env: &Env, ids: &Vec<u64>) {
    env.events()
        .publish((symbol_short!("btch_crt"),), ids.clone());
}

// ---------------------------------------------------------------------------
// Issue #559: Creator Revenue Share
// ---------------------------------------------------------------------------

/// Emitted when the creator fee is deducted during invoice release.
///
/// Topics: `("creator_fee", invoice_id)`
/// Data:   `(creator, fee_amount)`
pub fn creator_fee_paid(env: &Env, invoice_id: u64, creator: &Address, fee_amount: i128) {
    env.events().publish(
        (symbol_short!("crtr_fee"), invoice_id),
        (creator.clone(), fee_amount),
    );
}

/// Issue #685: Emitted when a creator-declared fee (`creator_fee_bps`) is
/// deducted from recipient payouts at release time.
///
/// Topics: (split, creator_fee_collected, invoice_id)
/// Data:   (creator, fee_amount)
pub fn creator_fee_collected(env: &Env, invoice_id: u64, creator: &Address, fee_amount: i128) {
    env.events().publish(
        (
            symbol_short!("split"),
            soroban_sdk::Symbol::new(env, "creator_fee_collected"),
            invoice_id,
        ),
        (creator.clone(), fee_amount),
    );
}

// ---------------------------------------------------------------------------
// Issue #560: Creator Migration
// ---------------------------------------------------------------------------

/// Emitted when a new creator is nominated for an invoice.
///
/// Topics: `("creator_nom", invoice_id)`
/// Data:   `(successor)`
pub fn creator_nominated(env: &Env, invoice_id: u64, successor: &Address) {
    env.events().publish(
        (symbol_short!("crtr_nom"), invoice_id),
        successor.clone(),
    );
}

/// Emitted when a creator role is accepted and the creator is migrated.
///
/// Topics: `("creator_mig", invoice_id)`
/// Data:   `(new_creator)`
pub fn creator_migrated(env: &Env, invoice_id: u64, new_creator: &Address) {
    env.events().publish(
        (symbol_short!("crtr_mig"), invoice_id),
        new_creator.clone(),
    );
}

// ---------------------------------------------------------------------------
// Issue #561: Payout Ordering
// ---------------------------------------------------------------------------

/// Emitted when a payout is initiated for a recipient.
///
/// Topics: `("payout_init", invoice_id, recipient_index)`
/// Data:   `(recipient, amount)`
pub fn payout_initiated(
    env: &Env,
    invoice_id: u64,
    recipient_index: u32,
    recipient: &Address,
    amount: i128,
) {
    env.events().publish(
        (symbol_short!("pyt_init"), invoice_id, recipient_index),
        (recipient.clone(), amount),
    );
}

// ---------------------------------------------------------------------------
// Event version constants used by newer event functions
// ---------------------------------------------------------------------------

const INVOICE_LIMIT_UPDATED_V: u32 = 1;
const RECIPIENT_ACCOUNT_MISSING_V: u32 = 1;

// ---------------------------------------------------------------------------
// Missing event functions (added by Wave 7 PRs)
// ---------------------------------------------------------------------------

/// Emitted when a contributor withdraws their contribution from a pending invoice.
pub fn contribution_withdrawn(env: &Env, invoice_id: u64, payer: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("ctb_wdrw"), invoice_id),
        (payer.clone(), amount),
    );
}

/// Emitted when an admin locks a recipient's share of an invoice.
pub fn recipient_share_locked(
    env: &Env,
    invoice_id: u64,
    recipient: &Address,
    admin: &Address,
) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("sh_lock"), invoice_id),
        (recipient.clone(), admin.clone()),
    );
}

/// Issue #684: Emitted at every `InvoicePhase` transition (Draft -> Active ->
/// Locked -> Released).
/// Topics: (split, phase_chg, invoice_id)
/// Data: (old_phase, new_phase, event_seq)
pub fn invoice_phase_changed(
    env: &Env,
    invoice_id: u64,
    old_phase: &InvoicePhase,
    new_phase: &InvoicePhase,
) {
    let phase_sym = |phase: &InvoicePhase| match phase {
        InvoicePhase::Draft => symbol_short!("draft"),
        InvoicePhase::Active => symbol_short!("active"),
        InvoicePhase::Locked => symbol_short!("locked"),
        InvoicePhase::Released => symbol_short!("released"),
    };
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("phase_chg"), invoice_id),
        (phase_sym(old_phase), phase_sym(new_phase), event_seq),
    );
}

/// Emitted when an admin unlocks a recipient's share of an invoice.
pub fn recipient_share_unlocked(
    env: &Env,
    invoice_id: u64,
    recipient: &Address,
    admin: &Address,
) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("sh_unlk"), invoice_id),
        (recipient.clone(), admin.clone()),
    );
}

/// Issue #528: Emitted when an admin transfer is proposed.
/// Topics: (split, adm_prop)
/// Data: (current_admin, proposed_admin)
pub fn admin_transfer_proposed(env: &Env, current_admin: &Address, proposed_admin: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("adm_prop")),
        (current_admin.clone(), proposed_admin.clone()),
    );
}

/// Issue #528: Emitted when an admin transfer is completed.
/// Topics: (split, adm_done)
/// Data: new_admin
pub fn admin_transfer_completed(env: &Env, new_admin: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("adm_done")),
        new_admin.clone(),
    );
}

/// Emitted when a recipient is added to or removed from an invoice whitelist.
/// Topics: (split, wl_upd, invoice_id)
/// Data: (enabled, added_count, removed_count)
pub fn recipient_whitelist_updated(
    env: &Env,
    invoice_id: u64,
    enabled: bool,
    added: &Vec<Address>,
    removed: &Vec<Address>,
) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("wl_upd"), invoice_id),
        (enabled, added.len(), removed.len()),
    );
}

/// Emitted when an overfunding event is triggered on an invoice.
/// Topics: (split, overfund, invoice_id)
/// Data: (payer, surplus)
pub fn overfunding_triggered(
    env: &Env,
    invoice_id: u64,
    payer: &Address,
    _policy: &OverfundingPolicy,
    surplus: i128,
) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("overfund"), invoice_id),
        (payer.clone(), surplus),
    );
}

/// Emitted when a late-payment penalty is applied.
/// Topics: (split, penalty, invoice_id)
/// Data: (payer, penalty_amount, penalty_bps)
pub fn penalty_applied(
    env: &Env,
    invoice_id: u64,
    payer: &Address,
    penalty_amount: i128,
    penalty_bps: u32,
) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("penalty"), invoice_id),
        (payer.clone(), penalty_amount, penalty_bps),
    );
}

/// Emitted when an invoice deadline is extended.
/// Topics: (split, dl_ext, invoice_id)
/// Data: (old_deadline, new_deadline)
pub fn deadline_extended(env: &Env, invoice_id: u64, old_deadline: u64, new_deadline: u64) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("dl_ext"), invoice_id),
        (old_deadline, new_deadline),
    );
}

// ---------------------------------------------------------------------------
// Issue #892: Security audit framework events
// ---------------------------------------------------------------------------

/// Emitted when an auditor is registered (`added = true`) or removed.
/// Topics: (split, sec_adtr)
/// Data: (auditor, added)
pub fn security_auditor_updated(env: &Env, auditor: &Address, added: bool) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("sec_adtr")),
        (auditor.clone(), added),
    );
}

/// Emitted when a security audit run completes.
/// Topics: (split, sec_aud, audit_id)
/// Data: (auditor, invoice_id, failed_count, risk_score, ledger)
pub fn security_audit_completed(
    env: &Env,
    audit_id: u64,
    auditor: &Address,
    invoice_id: Option<u64>,
    failed_count: u32,
    risk_score: u32,
) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("sec_aud"), audit_id),
        (
            auditor.clone(),
            invoice_id,
            failed_count,
            risk_score,
            env.ledger().sequence(),
        ),
    );
}

/// Emitted when an auditor logs a security event.
/// Topics: (split, sec_evt, kind)
/// Data: (reporter, severity, invoice_id, timestamp)
pub fn security_event_logged(
    env: &Env,
    reporter: &Address,
    kind: &Symbol,
    severity: u32,
    invoice_id: Option<u64>,
) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("sec_evt"), kind.clone()),
        (
            reporter.clone(),
            severity,
            invoice_id,
            env.ledger().timestamp(),
        ),
    );
}

// ---------------------------------------------------------------------------
// Issue #911: Partial invoice payment tracking events
// ---------------------------------------------------------------------------

/// Emitted when partial payment tracking is initialised for an invoice.
/// Topics: (split, pt_init, invoice_id)
/// Data: (total_due, milestone_count, event_seq)
pub fn payment_tracking_initialised(env: &Env, invoice_id: u64, total_due: i128, milestone_count: u32) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("pt_init"), invoice_id),
        (total_due, milestone_count, event_seq),
    );
}

/// Emitted when a partial payment to a recipient is recorded.
/// Topics: (split, pt_pay, invoice_id)
/// Data: (recipient, amount, total_paid_to_recipient, fully_paid, event_seq)
pub fn partial_payment_recorded(
    env: &Env,
    invoice_id: u64,
    recipient: &Address,
    amount: i128,
    recipient_paid: i128,
    fully_paid: bool,
) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("pt_pay"), invoice_id),
        (recipient.clone(), amount, recipient_paid, fully_paid, event_seq),
    );
}

/// Emitted when a payment milestone changes state (1 = Reached, 2 = Settled).
/// Topics: (split, pt_mile, invoice_id)
/// Data: (threshold_bps, state, event_seq)
pub fn payment_milestone_state(env: &Env, invoice_id: u64, threshold_bps: u32, state: u32) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("pt_mile"), invoice_id),
        (threshold_bps, state, event_seq),
    );
}

// ---------------------------------------------------------------------------
// Issue #912: Payment velocity limit events
// ---------------------------------------------------------------------------

/// Emitted when the default (`payer = None`) or a per-payer velocity config
/// is set. A cleared override is reported with all limits at zero.
/// Topics: (split, vel_cfg)
/// Data: (payer, max_payments, max_amount, window_secs)
pub fn velocity_config_set(
    env: &Env,
    payer: Option<Address>,
    max_payments: u32,
    max_amount: i128,
    window_secs: u64,
) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("vel_cfg")),
        (payer, max_payments, max_amount, window_secs),
    );
}

// ---------------------------------------------------------------------------
// Issue #913: Split rules engine events
// ---------------------------------------------------------------------------

/// Emitted when an invoice's split rules are set (`rule_count > 0`) or cleared.
/// Topics: (split, srules, invoice_id)
/// Data: (rule_count, event_seq)
pub fn split_rules_set(env: &Env, invoice_id: u64, rule_count: u32) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("srules"), invoice_id),
        (rule_count, event_seq),
    );
}

// ---------------------------------------------------------------------------
// Unit tests for the per-invoice event sequence counter (issue #708)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Issue #918: Invoice prioritization
// ---------------------------------------------------------------------------

/// Issue #918: Emitted when an invoice's priority is set or updated.
/// Topics: (split, pri_set, invoice_id)
/// Data: (set_by, old_score, new_score, tier, event_seq)
pub fn invoice_priority_set(
    env: &Env,
    invoice_id: u64,
    set_by: &Address,
    old_score: Option<u32>,
    new_score: u32,
    tier: u32,
) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("pri_set"), invoice_id),
        (set_by.clone(), old_score, new_score, tier, event_seq),
    );
}

/// Issue #918: Emitted when an invoice's priority is cleared.
/// Topics: (split, pri_clr, invoice_id)
/// Data: (cleared_by, event_seq)
pub fn invoice_priority_cleared(env: &Env, invoice_id: u64, cleared_by: &Address) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("pri_clr"), invoice_id),
        (cleared_by.clone(), event_seq),
    );
}

// ---------------------------------------------------------------------------
// Issue #919: Creator fee tier system
// ---------------------------------------------------------------------------

/// Issue #919: Emitted when the creator fee tier table is replaced, updated or
/// cleared (`version == 0`, `tier_count == 0` on clear).
/// Topics: (split, cfee_cfg)
/// Data: (version, tier_count)
pub fn creator_fee_tiers_configured(env: &Env, version: u32, tier_count: u32) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("cfee_cfg")),
        (version, tier_count),
    );
}

/// Issue #919: Emitted when a creator's tracked fee tier changes.
/// Topics: (split, cfee_chg, creator)
/// Data: (old_tier_id, new_tier_id, fee_bps, volume)
pub fn creator_fee_tier_changed(
    env: &Env,
    creator: &Address,
    old_tier_id: Option<u32>,
    new_tier_id: u32,
    fee_bps: u32,
    volume: i128,
) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("cfee_chg"), creator.clone()),
        (old_tier_id, new_tier_id, fee_bps, volume),
    );
}

// ---------------------------------------------------------------------------
// Issue #920: Invoice metadata storage
// ---------------------------------------------------------------------------

/// Issue #920: Emitted when a metadata entry is created or overwritten.
/// Topics: (split, meta_set, invoice_id)
/// Data: (key, version, updated_by, event_seq)
pub fn invoice_metadata_set(
    env: &Env,
    invoice_id: u64,
    key: &String,
    version: u32,
    updated_by: &Address,
) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("meta_set"), invoice_id),
        (key.clone(), version, updated_by.clone(), event_seq),
    );
}

/// Issue #920: Emitted when a metadata entry is removed.
/// Topics: (split, meta_del, invoice_id)
/// Data: (key, removed_by, event_seq)
pub fn invoice_metadata_removed(env: &Env, invoice_id: u64, key: &String, removed_by: &Address) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("meta_del"), invoice_id),
        (key.clone(), removed_by.clone(), event_seq),
    );
}

/// Issue #920: Emitted when an invoice's metadata is permanently locked.
/// Topics: (split, meta_lck, invoice_id)
/// Data: (locked_by, event_seq)
pub fn invoice_metadata_locked(env: &Env, invoice_id: u64, locked_by: &Address) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("meta_lck"), invoice_id),
        (locked_by.clone(), event_seq),
    );
}

// ---------------------------------------------------------------------------
// Issue #921: Confidential payment tracking
// ---------------------------------------------------------------------------

/// Issue #921: Emitted when a confidential payment is recorded or replaced.
/// Carries only a digest of the commitment — never an amount.
/// Topics: (split, ctrk_rec, invoice_id)
/// Data: (payer, commitment_digest, revision, event_seq)
pub fn confidential_payment_recorded(
    env: &Env,
    invoice_id: u64,
    payer: &Address,
    commitment_digest: &BytesN<32>,
    revision: u32,
) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("ctrk_rec"), invoice_id),
        (payer.clone(), commitment_digest.clone(), revision, event_seq),
    );
}

/// Issue #921: Emitted when the creator proves the confidential invoice total.
/// Topics: (split, ctrk_ver, invoice_id)
/// Data: (verified_by, total, event_seq)
pub fn confidential_total_verified(env: &Env, invoice_id: u64, verified_by: &Address, total: i128) {
    let event_seq = next_seq(env, invoice_id);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("ctrk_ver"), invoice_id),
        (verified_by.clone(), total, event_seq),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{Address, Env};

    fn contract_id(env: &Env) -> Address {
        env.register(crate::SplitContract, ())
    }

    /// `next_seq` returns 1 on first call and increments on each subsequent
    /// call for the same invoice ID.
    #[test]
    fn test_next_seq_increments_per_invoice() {
        let env = Env::default();
        let id = contract_id(&env);
        env.as_contract(&id, || {
            assert_eq!(next_seq(&env, 1), 1);
            assert_eq!(next_seq(&env, 1), 2);
            assert_eq!(next_seq(&env, 1), 3);
        });
    }

    /// Sequences for different invoice IDs are independent — incrementing the
    /// counter for invoice A must not affect invoice B's counter.
    #[test]
    fn test_next_seq_independent_for_different_invoice_ids() {
        let env = Env::default();
        let id = contract_id(&env);
        env.as_contract(&id, || {
            // Advance invoice 10 twice.
            assert_eq!(next_seq(&env, 10), 1);
            assert_eq!(next_seq(&env, 10), 2);

            // Invoice 20 should still start at 1.
            assert_eq!(next_seq(&env, 20), 1);

            // Invoice 10 continues independently from where it left off.
            assert_eq!(next_seq(&env, 10), 3);

            // Invoice 20 is still at 2 after one more call.
            assert_eq!(next_seq(&env, 20), 2);
        });
    }

    /// Issue #954: `PaymentBreakdown` preserves gross/fee/net values and the
    /// `PaymentEvent` metadata is populated correctly.
    #[test]
    fn test_payment_event_breakdown_accuracy() {
        let env = Env::default();
        let recipient = Address::generate(&env);
        let payer = Address::generate(&env);

        let entry = PaymentBreakdown {
            recipient: recipient.clone(),
            gross: 1_000,
            fee: 25,
            net: 975,
        };
        assert_eq!(entry.gross - entry.fee, entry.net);

        let mut breakdown = Vec::new(&env);
        breakdown.push_back(entry.clone());

        let event = PaymentEvent {
            metadata: PaymentEventMetadata {
                invoice_id: 42,
                payer: payer.clone(),
                timestamp: 1234,
                total: 1_000,
            },
            breakdown: breakdown.clone(),
        };

        assert_eq!(event.metadata.invoice_id, 42);
        assert_eq!(event.metadata.payer, payer);
        assert_eq!(event.metadata.timestamp, 1234);
        assert_eq!(event.metadata.total, 1_000);
        assert_eq!(event.breakdown.len(), 1);
        assert_eq!(event.breakdown.get(0).unwrap(), entry);
    }

    /// Issue #956: `RefundReason` variants are distinct and comparable.
    #[test]
    fn test_refund_reason_variants() {
        let reasons = [
            RefundReason::Expired,
            RefundReason::Cancelled,
            RefundReason::Dispute,
            RefundReason::AdminOverride,
            RefundReason::Overpayment,
        ];
        for (i, a) in reasons.iter().enumerate() {
            for (j, b) in reasons.iter().enumerate() {
                assert_eq!(i == j, a == b);
            }
        }
    }
}
