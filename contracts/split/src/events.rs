use crate::types::{DisputeOutcome, InvoiceStatus, RepScore, TimelockAction};
use soroban_sdk::{symbol_short, Address, BytesN, Env, String, Vec};

/// Emitted when a new invoice is created.
/// Topics: (split, created, invoice_id)
/// Data: (creator, total)
pub fn invoice_created(
    env: &Env,
    invoice_id: u64,
    creator: &Address,
    total: i128,
    cross_chain_ref: &Option<String>,
) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("created"), invoice_id),
        (creator.clone(), total, cross_chain_ref.clone()),
    );
}

/// Emitted when a payment is received toward an invoice.
/// Topics: (split, paid, invoice_id)
/// Data: (payer, amount)
pub fn payment_received(env: &Env, invoice_id: u64, payer: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("paid"), invoice_id),
        (payer.clone(), amount),
    );
}

/// Emitted when an invoice is fully funded and funds are released.
/// Topics: (split, released, invoice_id)
/// Data: recipients
pub fn invoice_released(env: &Env, invoice_id: u64, recipients: &Vec<Address>) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("released"),
            invoice_id,
        ),
        recipients.clone(),
    );
}

/// Emitted when an invoice is refunded after deadline.
/// Topics: (split, refunded, invoice_id)
/// Data: ()
pub fn invoice_refunded(env: &Env, invoice_id: u64) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("refunded"),
            invoice_id,
        ),
        (),
    );
}

/// Emitted when a release-condition preimage is verified.
/// Topics: (split, cond_ok, invoice_id)
/// Data: preimage_hash
pub fn condition_verified(env: &Env, invoice_id: u64, preimage_hash: &BytesN<32>) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("cond_ok"),
            invoice_id,
        ),
        preimage_hash.clone(),
    );
}

/// Emitted when an invoice expires.
/// Topics: (split, expired, invoice_id)
/// Data: (deadline, funded)
pub fn invoice_expired(env: &Env, invoice_id: u64, deadline: u64, funded: i128) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("expired"),
            invoice_id,
        ),
        (deadline, funded),
    );
}

/// Emitted when a recipient is added to an invoice whitelist.
/// Topics: (split, rcp_wl, invoice_id)
/// Data: address
pub fn recipient_whitelisted(env: &Env, invoice_id: u64, address: &Address) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("rcp_wl"),
            invoice_id,
        ),
        address.clone(),
    );
}

/// Emitted when a recipient is removed from an invoice whitelist.
/// Topics: (split, rcp_rl, invoice_id)
/// Data: address
pub fn recipient_removed_from_whitelist(env: &Env, invoice_id: u64, address: &Address) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("rcp_rl"),
            invoice_id,
        ),
        address.clone(),
    );
}

/// Emitted when rebate is accrued for a creator.
/// Topics: (split, rbt_acr, creator)
/// Data: (amount, tier_bps)
pub fn rebate_accrued(env: &Env, creator: &Address, amount: i128, tier_bps: u32) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("rbt_acr"), creator.clone()),
        (amount, tier_bps),
    );
}

/// Emitted once per payer when their refund is transferred.
/// Topics: (split, pay_ref, invoice_id)
/// Data: (payer, amount)
pub fn payer_refunded(env: &Env, invoice_id: u64, payer: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("pay_ref"), invoice_id),
        (payer.clone(), amount),
    );
}

/// Emitted when a recipient is added to a pending invoice.
/// Topics: (split, add_rec, invoice_id)
/// Data: (recipient, amount)
pub fn recipient_added(env: &Env, invoice_id: u64, recipient: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("add_rec"), invoice_id),
        (recipient.clone(), amount),
    );
}

/// Emitted when the creator adjusts recipient split amounts.
/// Topics: (split, adj_spl, invoice_id)
/// Data: creator
pub fn split_adjusted(env: &Env, invoice_id: u64, creator: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("adj_spl"), invoice_id),
        creator.clone(),
    );
}

/// Emitted when an invoice is archived to instance storage.
/// Topics: (split, archived, invoice_id)
/// Data: ()
pub fn invoice_archived(env: &Env, invoice_id: u64) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("archived"),
            invoice_id,
        ),
        (),
    );
}

/// Emitted when a delegate is assigned to an invoice.
/// Topics: (split, delegated, invoice_id)
/// Data: delegate
pub fn delegate_set(env: &Env, invoice_id: u64, delegate: &Address) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("delegated"),
            invoice_id,
        ),
        delegate.clone(),
    );
}

/// Emitted when a delegate is revoked from an invoice.
/// Topics: (split, revoked, invoice_id)
/// Data: ()
pub fn delegate_revoked(env: &Env, invoice_id: u64) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("revoked"), invoice_id),
        (),
    );
}

/// Emitted when an invoice is partially released.
/// Topics: (split, part_rel, invoice_id)
/// Data: recipients
pub fn invoice_partially_released(env: &Env, invoice_id: u64, recipients: &Vec<Address>) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("part_rel"),
            invoice_id,
        ),
        recipients.clone(),
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
/// Data: ()
pub fn invoice_cloned(env: &Env, source_id: u64, new_id: u64) {
    env.events()
        .publish((symbol_short!("cloned"), source_id, new_id), ());
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

/// Emitted when an invoice is resumed.
/// Topics: (split, resumed, invoice_id)
/// Data: creator
pub fn invoice_resumed(env: &Env, invoice_id: u64, creator: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("resumed"), invoice_id),
        creator.clone(),
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
pub fn rate_limit_hit(
    env: &Env,
    invoice_id: u64,
    payer: &Address,
    next_allowed_ledger: u32,
) {
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
/// `invoice_id` ordered by ledger, then replay `from → to` pairs.
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
    };
    let to_sym = match to_status {
        InvoiceStatus::Pending => symbol_short!("pending"),
        InvoiceStatus::Released => symbol_short!("released"),
        InvoiceStatus::Refunded => symbol_short!("refunded"),
        InvoiceStatus::Expired => symbol_short!("expired"),
        InvoiceStatus::Cancelled => symbol_short!("cancld"),
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

/// Issue #308: Emitted when a payer claims their per-payer refund.
/// Topics: (split, ref_clm, invoice_id)
/// Data: (payer, amount)
pub fn refund_claimed(env: &Env, invoice_id: u64, payer: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("ref_clm"), invoice_id),
        (payer.clone(), amount),
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
/// `milestone_bps` encodes the threshold in basis points:
///   - 2500 = 25%
///   - 5000 = 50%
///   - 7500 = 75%
///   - 10000 = 100%
///
/// Multiple events can be emitted in a single `pay()` call when a large payment
/// crosses several thresholds at once.
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

/// Issue #326: Emitted when a protocol fee is paid to treasury on release.
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
/// Data: score
pub fn rep_updated(env: &Env, address: &Address, score: &RepScore) {
    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("rep_upd"),
            address.clone(),
        ),
        score.clone(),
    );
}

/// Issue #437: Emitted when a delayed payout is scheduled on release.
/// Topics: (split, dlypay_s, invoice_id)
/// Data: (recipient, claimable_at_ledger)
pub fn delayed_payout_scheduled(env: &Env, invoice_id: u64, recipient: &Address, claimable_at: u32) {
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
        (symbol_short!("split"), symbol_short!("upg_frz"), symbol_short!("")),
        (checkpoint_hash.clone(), env.ledger().sequence()),
    );
}

/// Issue #435: Emitted when the contract is thawed (upgrade freeze removed).
/// Topics: (split, upg_thw, ())
/// Data: admin
pub fn contract_thawed(env: &Env, admin: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("upg_thw"), symbol_short!("")),
        admin.clone(),
    );
}

/// Issue #431: Emitted when a duplicate payment is detected and rejected.
/// Topics: (split, dup_pay, invoice_id)
/// Data: (payer, amount, fingerprint_hash)
pub fn duplicate_payment_rejected(env: &Env, invoice_id: u64, payer: &Address, amount: i128, fingerprint: &BytesN<32>) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("dup_pay"), invoice_id),
        (payer.clone(), amount, fingerprint.clone()),
    );
}

/// Issue #432: Emitted when a referrer receives a reward share.
/// Topics: (split, ref_rwd, invoice_id)
/// Data: (referrer, amount)
pub fn referrer_rewarded(env: &Env, invoice_id: u64, referrer: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("ref_rwd"), invoice_id),
        (referrer.clone(), amount),
    );
}

/// Issue #434: Emitted when a group member expires unfunded.
/// Topics: (split, grp_exp, invoice_id)
/// Data: group_id
pub fn group_member_expired(env: &Env, invoice_id: u64, group_id: u64) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("grp_exp"), invoice_id),
        group_id,
    );
}

/// Issue #434: Emitted when a group rollback is triggered.
/// Topics: (split, grp_roll, group_id)
/// Data: member_count
pub fn group_rollback_triggered(env: &Env, group_id: u64, member_count: u32) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("grp_roll"), group_id),
        member_count,
    );
}
