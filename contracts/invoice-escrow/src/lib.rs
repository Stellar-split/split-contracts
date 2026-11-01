//! Invoice Escrow Contract — StellarSplit
//!
//! Provides on-chain invoice escrow with:
//! - Multi-payer deposit aggregation
//! - Automatic fund release when fully funded
//! - Deadline-based refund fallback
//! - **Upgradable admin authority** via a two-step `transfer_admin` flow
//! - **Co-signer approval**: optional N-of-M sign-off gating release
//! - **Oracle condition guards**: optional price-feed condition, with a
//!   fallback oracle, that must hold before release
//! - **Scheduled release**: optional time + ledger-count delay before release
//! - **Refund grace period**: configurable delay after the deadline before
//!   refunds can be processed, with admin extend / waive overrides
//!
//! # Admin Transfer Flow
//!
//! The current admin calls [`InvoiceEscrowContract::transfer_admin`] to propose
//! a new admin address. The proposed address must then call
//! [`InvoiceEscrowContract::accept_admin`] to complete the handover. The current
//! admin can cancel the pending proposal at any time via
//! [`InvoiceEscrowContract::cancel_transfer`]. This prevents accidental or
//! malicious lock-out by ensuring the new admin actively confirms the transfer.

#![no_std]

mod errors;
mod oracle;
mod types;

#[cfg(test)]
mod test;

use errors::Error;
use soroban_sdk::{
    contract, contractimpl, symbol_short, token, Address, BytesN, Env, Symbol, Vec,
};
use oracle::PriceOracleClient;
use types::{
    ApprovalStatus, BlacklistEntry, CosignerApproval, EscrowInvoice, EscrowReleased, EscrowStatus,
    OracleCondition, OracleReading, OracleSource, OptionalRefundReason, PriceComparator, PriceData, RefundGrace,
    RefundGraceStatus, RefundReason, ReleaseSchedule, ScheduleStatus,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum allowed deadline horizon (seconds from now). Roughly 10 years.
const MAX_DEADLINE_HORIZON: u64 = 315_360_000;

// ---------------------------------------------------------------------------
// Storage key helpers
// ---------------------------------------------------------------------------

/// Instance storage: the current contract admin.
fn admin_key() -> Symbol {
    symbol_short!("admin")
}

/// Instance storage: the pending (proposed) admin awaiting acceptance.
fn pending_admin_key() -> Symbol {
    symbol_short!("pend_adm")
}

/// Instance storage: monotonic invoice counter.
fn counter_key() -> Symbol {
    symbol_short!("counter")
}

/// Instance storage: default refund grace period (seconds) applied to new invoices.
fn default_grace_key() -> Symbol {
    symbol_short!("grace_def")
}

/// Persistent storage: per-invoice record keyed by `(escrow, id)`.
fn invoice_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("escrow"), id)
}

/// Persistent storage: per-payer per-invoice deposit total.
fn deposit_key(id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("deposit"), id, payer.clone())
}

/// Persistent storage: blacklist entry for a payer.
fn blacklist_key(payer: &Address) -> (Symbol, Address) {
    (symbol_short!("blacklist"), payer.clone())
}

/// Persistent storage: co-signer approval record for an invoice.
fn approval_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("approval"), id)
}

/// Persistent storage: oracle release condition for an invoice.
fn oracle_cond_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("orc_cond"), id)
}

/// Persistent storage: most recent successful oracle evaluation for an invoice.
fn oracle_read_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("orc_read"), id)
}

/// Persistent storage: release schedule for an invoice.
fn schedule_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("schedule"), id)
}

/// Persistent storage: refund grace configuration for an invoice.
fn grace_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("grace"), id)
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Number of ledgers during which a blacklisted payer can submit an appeal.
/// ~30 minutes at 5s ledger close time.
const APPEAL_WINDOW_LEDGERS: u32 = 360;

/// Maximum number of co-signers on a single invoice. Bounds the linear
/// membership scans performed on every vote.
const MAX_COSIGNERS: u32 = 20;

/// Maximum ledger delay for a release schedule (~1 year at 5s ledgers).
const MAX_SCHEDULE_LEDGERS: u32 = 6_312_000;

/// Extra ledgers of TTL kept on a scheduled invoice beyond its unlock
/// ledger (~1 day), so the record is still live when it becomes due.
const SCHEDULE_TTL_BUFFER_LEDGERS: u32 = 17_280;

/// Maximum refund grace period, including admin extensions (30 days).
const MAX_REFUND_GRACE_SECS: u64 = 30 * 24 * 60 * 60;

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn require_admin(env: &Env) -> Address {
    let admin: Address = env
        .storage()
        .instance()
        .get(&admin_key())
        .expect("not initialized");
    admin.require_auth();
    admin
}

/// Validates that a deadline is in the future and not too far away.
fn validate_deadline(env: &Env, deadline: u64) -> Result<(), Error> {
    let now = env.ledger().timestamp();
    if deadline <= now {
        return Err(Error::DeadlinePassed);
    }
    if deadline > now.saturating_add(MAX_DEADLINE_HORIZON) {
        return Err(Error::DeadlineOutOfRange);
    }
    Ok(())
}

fn get_invoice(env: &Env, id: u64) -> Result<EscrowInvoice, Error> {
    env.storage()
        .persistent()
        .get(&invoice_key(id))
        .ok_or(Error::InvoiceNotFound)
}

fn save_invoice(env: &Env, id: u64, invoice: &EscrowInvoice) {
    env.storage().persistent().set(&invoice_key(id), invoice);
}

/// Returns `true` while the invoice can still accept configuration changes
/// and votes (i.e. it has not been released, refunded or cancelled).
fn is_open(invoice: &EscrowInvoice) -> bool {
    invoice.status == EscrowStatus::Pending || invoice.status == EscrowStatus::Active
}

fn get_approval(env: &Env, id: u64) -> Option<CosignerApproval> {
    env.storage().persistent().get(&approval_key(id))
}

fn save_approval(env: &Env, id: u64, approval: &CosignerApproval) {
    env.storage().persistent().set(&approval_key(id), approval);
}

/// Recompute `approval.status` from the current tallies.
fn recompute_approval_status(approval: &mut CosignerApproval) {
    let remaining = approval.signers.len() - approval.rejections.len();
    approval.status = if approval.approvals.len() >= approval.threshold {
        ApprovalStatus::Approved
    } else if remaining < approval.threshold {
        ApprovalStatus::Rejected
    } else {
        ApprovalStatus::Pending
    };
}

fn get_oracle_condition(env: &Env, id: u64) -> Option<OracleCondition> {
    env.storage().persistent().get(&oracle_cond_key(id))
}

/// Query a single oracle and validate the returned price.
///
/// Uses `try_lastprice` so that a panicking, missing or incompatible oracle
/// contract surfaces as [`Error::OracleUnavailable`] instead of aborting.
fn read_oracle(env: &Env, oracle: &Address, asset: &Address, max_staleness_secs: u64) -> Result<PriceData, Error> {
    let client = PriceOracleClient::new(env, oracle);
    let data = match client.try_lastprice(asset) {
        Ok(Ok(Some(data))) => data,
        _ => return Err(Error::OracleUnavailable),
    };

    let now = env.ledger().timestamp();
    if data.price <= 0 || data.timestamp > now {
        return Err(Error::OracleInvalidPrice);
    }
    if now - data.timestamp > max_staleness_secs {
        return Err(Error::OracleStalePrice);
    }
    Ok(data)
}

/// Evaluate an oracle condition, falling back to the secondary oracle when
/// the primary cannot supply a usable price.
///
/// When both oracles fail, the error from the last oracle tried is returned
/// (the fallback's if one is configured, otherwise the primary's).
fn evaluate_oracle_condition(env: &Env, id: u64, cond: &OracleCondition) -> Result<OracleReading, Error> {
    let (data, source) = match read_oracle(env, &cond.oracle, &cond.asset, cond.max_staleness_secs) {
        Ok(data) => (data, OracleSource::Primary),
        Err(primary_err) => {
            let fallback = cond.fallback_oracle.as_ref().ok_or(primary_err)?;
            emit_oracle_fallback(env, id, primary_err);
            let data = read_oracle(env, fallback, &cond.asset, cond.max_staleness_secs)?;
            (data, OracleSource::Fallback)
        }
    };

    let condition_met = match cond.comparator {
        PriceComparator::GreaterOrEqual => data.price >= cond.threshold_price,
        PriceComparator::LessOrEqual => data.price <= cond.threshold_price,
    };
    Ok(OracleReading {
        price: data.price,
        price_timestamp: data.timestamp,
        source,
        condition_met,
        checked_at: env.ledger().timestamp(),
    })
}

fn get_schedule(env: &Env, id: u64) -> Option<ReleaseSchedule> {
    env.storage().persistent().get(&schedule_key(id))
}

/// Persist a schedule and keep it (and its invoice) alive at least until
/// the unlock ledger plus a buffer, capped at the network's maximum TTL.
fn save_schedule(env: &Env, id: u64, schedule: &ReleaseSchedule) {
    let storage = env.storage().persistent();
    storage.set(&schedule_key(id), schedule);

    let ledgers_left = schedule
        .unlock_ledger
        .saturating_sub(env.ledger().sequence());
    let extend_to = ledgers_left
        .saturating_add(SCHEDULE_TTL_BUFFER_LEDGERS)
        .min(env.storage().max_ttl());
    storage.extend_ttl(&schedule_key(id), extend_to, extend_to);
    storage.extend_ttl(&invoice_key(id), extend_to, extend_to);
}

/// Check every release gate configured on the invoice. Returns `Ok(())`
/// when the invoice may be released now.
///
/// Gates are checked cheapest-first: release schedule, co-signer approval,
/// then the oracle condition (which requires a cross-contract call).
fn check_release_gates(env: &Env, id: u64) -> Result<(), Error> {
    if let Some(schedule) = get_schedule(env, id) {
        if schedule.status == ScheduleStatus::Scheduled {
            if env.ledger().timestamp() < schedule.release_at {
                return Err(Error::ReleaseNotYetDue);
            }
            if env.ledger().sequence() < schedule.unlock_ledger {
                return Err(Error::ScheduleLedgerNotReached);
            }
        }
    }
    if let Some(approval) = get_approval(env, id) {
        match approval.status {
            ApprovalStatus::Approved => {}
            ApprovalStatus::Pending => return Err(Error::ApprovalRequired),
            ApprovalStatus::Rejected => return Err(Error::ApprovalRejected),
        }
    }
    if let Some(cond) = get_oracle_condition(env, id) {
        let reading = evaluate_oracle_condition(env, id, &cond)?;
        env.storage().persistent().set(&oracle_read_key(id), &reading);
        if !reading.condition_met {
            return Err(Error::OracleConditionNotMet);
        }
    }
    Ok(())
}

/// Shared implementation of `approve_release` / `reject_release`.
fn cast_vote(env: &Env, signer: Address, invoice_id: u64, approve: bool) -> Result<ApprovalStatus, Error> {
    signer.require_auth();
    let invoice = get_invoice(env, invoice_id)?;
    if !is_open(&invoice) {
        return Err(Error::InvalidStatus);
    }
    let mut approval = get_approval(env, invoice_id).ok_or(Error::NoCosigners)?;
    if !approval.signers.contains(&signer) {
        return Err(Error::NotCosigner);
    }
    if approval.approvals.contains(&signer) || approval.rejections.contains(&signer) {
        return Err(Error::AlreadyVoted);
    }

    let action = if approve {
        approval.approvals.push_back(signer.clone());
        symbol_short!("approved")
    } else {
        approval.rejections.push_back(signer.clone());
        symbol_short!("rejected")
    };

    let old_status = approval.status.clone();
    recompute_approval_status(&mut approval);
    approval.updated_ledger = env.ledger().sequence();
    save_approval(env, invoice_id, &approval);

    emit_cosigner_vote(env, action, invoice_id, &signer, &approval);
    if old_status != approval.status {
        emit_approval_status_changed(env, invoice_id, &old_status, &approval.status);
    }
    Ok(approval.status)
}

fn get_grace(env: &Env, id: u64) -> Option<RefundGrace> {
    env.storage().persistent().get(&grace_key(id))
}

fn save_grace(env: &Env, id: u64, grace: &RefundGrace) {
    env.storage().persistent().set(&grace_key(id), grace);
}

/// Timestamp after which refunds are allowed: the deadline plus any
/// un-waived grace period.
fn refund_opens_at(invoice: &EscrowInvoice, grace: &Option<RefundGrace>) -> u64 {
    match grace {
        Some(g) if !g.waived => invoice
            .deadline
            .saturating_add(g.grace_secs)
            .saturating_add(g.extension_secs),
        _ => invoice.deadline,
    }
}

fn grace_status(env: &Env, invoice: &EscrowInvoice, grace: &Option<RefundGrace>) -> RefundGraceStatus {
    let g = match grace {
        Some(g) if g.grace_secs > 0 || g.extension_secs > 0 || g.waived => g,
        _ => return RefundGraceStatus::NoGrace,
    };
    let now = env.ledger().timestamp();
    if g.waived {
        RefundGraceStatus::Waived
    } else if now <= invoice.deadline {
        RefundGraceStatus::NotStarted
    } else if now <= refund_opens_at(invoice, grace) {
        RefundGraceStatus::Active
    } else {
        RefundGraceStatus::Elapsed
    }
}

/// Mark the invoice released and transfer `total_amount` to the creator.
fn execute_release(env: &Env, id: u64, invoice: &mut EscrowInvoice) -> (Address, i128) {
    let total = invoice.total_amount;
    let creator = invoice.creator.clone();
    invoice.status = EscrowStatus::Released;
    save_invoice(env, id, invoice);
    let token_client = token::Client::new(env, &invoice.token);
    token_client.transfer(&env.current_contract_address(), &creator, &total);
    emit_released(env, id, total);

    if let Some(mut schedule) = get_schedule(env, id) {
        if schedule.status == ScheduleStatus::Scheduled {
            schedule.status = ScheduleStatus::Executed;
            env.storage().persistent().set(&schedule_key(id), &schedule);
            emit_schedule_executed(env, id);
        }
    }
    (creator, total)
}

// ---------------------------------------------------------------------------
// Event helpers
// ---------------------------------------------------------------------------

/// Topics: `(escrow, adm_prop)` — Data: `(current_admin, proposed_admin, ledger)`
fn emit_admin_transfer_proposed(env: &Env, current: &Address, proposed: &Address) {
    env.events().publish(
        (symbol_short!("escrow"), symbol_short!("adm_prop")),
        (current.clone(), proposed.clone(), env.ledger().sequence()),
    );
}

/// Topics: `(escrow, adm_acpt)` — Data: `(new_admin, ledger)`
fn emit_admin_transfer_accepted(env: &Env, new_admin: &Address) {
    env.events().publish(
        (symbol_short!("escrow"), symbol_short!("adm_acpt")),
        (new_admin.clone(), env.ledger().sequence()),
    );
}

/// Topics: `(escrow, adm_cncl)` — Data: `(admin, cancelled_pending, ledger)`
fn emit_admin_transfer_cancelled(env: &Env, admin: &Address, cancelled: &Address) {
    env.events().publish(
        (symbol_short!("escrow"), symbol_short!("adm_cncl")),
        (admin.clone(), cancelled.clone(), env.ledger().sequence()),
    );
}

/// Topics: `(escrow, created, id)` — Data: `(creator, total_amount, deadline)`
fn emit_invoice_created(env: &Env, id: u64, creator: &Address, total: i128, deadline: u64) {
    env.events().publish(
        (symbol_short!("escrow"), symbol_short!("created"), id),
        (creator.clone(), total, deadline),
    );
}

/// Topics: `(escrow, deposited, id)` — Data: `(payer, amount, funded_so_far)`
fn emit_deposited(env: &Env, id: u64, payer: &Address, amount: i128, funded: i128) {
    env.events().publish(
        (symbol_short!("escrow"), symbol_short!("deposited"), id),
        (payer.clone(), amount, funded),
    );
}

/// Topics: `(escrow, released, id)` — Data: total_amount
fn emit_released(env: &Env, id: u64, total: i128) {
    env.events().publish(
        (symbol_short!("escrow"), symbol_short!("released"), id),
        total,
    );
}

/// Topics: `(escrow, refunded, id)` — Data: `(payer, amount, reason)`
///
/// The `reason` is included so off-chain indexers can attribute each refund
/// to a [`RefundReason`] without additional lookups.
fn emit_refunded(env: &Env, id: u64, payer: &Address, amount: i128, reason: &RefundReason) {
    env.events().publish(
        (symbol_short!("escrow"), symbol_short!("refunded"), id),
        (payer.clone(), amount, reason.clone()),
    );
}

/// Topics: `(escrow, cosig_set, id)` — Data: `(signers, threshold)`
fn emit_cosigners_set(env: &Env, id: u64, signers: &Vec<Address>, threshold: u32) {
    env.events().publish(
        (symbol_short!("escrow"), symbol_short!("cosig_set"), id),
        (signers.clone(), threshold),
    );
}

/// Topics: `(escrow, <action>, id)` — Data: `(signer, approvals, rejections, status)`
///
/// `action` is one of `approved`, `rejected` or `revoked`.
fn emit_cosigner_vote(env: &Env, action: Symbol, id: u64, signer: &Address, approval: &CosignerApproval) {
    env.events().publish(
        (symbol_short!("escrow"), action, id),
        (
            signer.clone(),
            approval.approvals.len(),
            approval.rejections.len(),
            approval.status.clone(),
        ),
    );
}

/// Topics: `(escrow, appr_stat, id)` — Data: `(old_status, new_status)`
fn emit_approval_status_changed(env: &Env, id: u64, old: &ApprovalStatus, new: &ApprovalStatus) {
    env.events().publish(
        (symbol_short!("escrow"), symbol_short!("appr_stat"), id),
        (old.clone(), new.clone()),
    );
}

/// Topics: `(escrow, orc_set, id)` — Data: `(oracle, fallback_oracle, asset, threshold_price)`
fn emit_oracle_condition_set(env: &Env, id: u64, cond: &OracleCondition) {
    env.events().publish(
        (symbol_short!("escrow"), symbol_short!("orc_set"), id),
        (
            cond.oracle.clone(),
            cond.fallback_oracle.clone(),
            cond.asset.clone(),
            cond.threshold_price,
        ),
    );
}

/// Topics: `(escrow, orc_clear, id)` — Data: `admin`
fn emit_oracle_condition_cleared(env: &Env, id: u64, admin: &Address) {
    env.events().publish(
        (symbol_short!("escrow"), symbol_short!("orc_clear"), id),
        admin.clone(),
    );
}

/// Topics: `(escrow, orc_fback, id)` — Data: primary oracle error code
fn emit_oracle_fallback(env: &Env, id: u64, primary_err: Error) {
    env.events().publish(
        (symbol_short!("escrow"), symbol_short!("orc_fback"), id),
        primary_err as u32,
    );
}

/// Topics: `(escrow, sched_set, id)` — Data: `(release_at, unlock_ledger)`
fn emit_schedule_set(env: &Env, id: u64, schedule: &ReleaseSchedule) {
    env.events().publish(
        (symbol_short!("escrow"), symbol_short!("sched_set"), id),
        (schedule.release_at, schedule.unlock_ledger),
    );
}

/// Topics: `(escrow, sched_cnl, id)` — Data: ledger sequence
fn emit_schedule_cancelled(env: &Env, id: u64) {
    env.events().publish(
        (symbol_short!("escrow"), symbol_short!("sched_cnl"), id),
        env.ledger().sequence(),
    );
}

/// Topics: `(escrow, sched_exe, id)` — Data: `(timestamp, ledger)`
fn emit_schedule_executed(env: &Env, id: u64) {
    env.events().publish(
        (symbol_short!("escrow"), symbol_short!("sched_exe"), id),
        (env.ledger().timestamp(), env.ledger().sequence()),
    );
}

/// Topics: `(escrow, grace_def)` — Data: `grace_secs`
fn emit_default_grace_set(env: &Env, grace_secs: u64) {
    env.events().publish(
        (symbol_short!("escrow"), symbol_short!("grace_def")),
        grace_secs,
    );
}

/// Topics: `(escrow, <action>, id)` — Data: `(grace_secs, extension_secs, waived)`
///
/// `action` is one of `grace_set`, `grace_ext` or `grace_wvd`.
fn emit_grace_changed(env: &Env, action: Symbol, id: u64, grace: &RefundGrace) {
    env.events().publish(
        (symbol_short!("escrow"), action, id),
        (grace.grace_secs, grace.extension_secs, grace.waived),
    );
}

/// Topics: `(blacklist, bl_add)` — Data: `(payer, reason_hash, banned_at)`
fn emit_payer_blacklisted(env: &Env, payer: &Address, reason_hash: &BytesN<32>, banned_at: u64) {
    env.events().publish(
        (symbol_short!("blacklist"), symbol_short!("bl_add")),
        (payer.clone(), reason_hash.clone(), banned_at),
    );
}

/// Topics: `(blacklist, appeal)` — Data: `(payer, appeal_hash)`
fn emit_appeal_submitted(env: &Env, payer: &Address, appeal_hash: &BytesN<32>) {
    env.events().publish(
        (symbol_short!("blacklist"), symbol_short!("appeal")),
        (payer.clone(), appeal_hash.clone()),
    );
}

/// Topics: `(blacklist, bl_fin)` — Data: `(payer, upheld)`
fn emit_blacklist_finalised(env: &Env, payer: &Address, upheld: bool) {
    env.events().publish(
        (symbol_short!("blacklist"), symbol_short!("bl_fin")),
        (payer.clone(), upheld),
    );
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct InvoiceEscrowContract;

#[contractimpl]
impl InvoiceEscrowContract {
    // -----------------------------------------------------------------------
    // Lifecycle
    // -----------------------------------------------------------------------

    /// Initialize the contract. May only be called once.
    ///
    /// # Arguments
    /// * `admin` — Initial contract administrator.
    ///
    /// # Errors
    /// * [`Error::AlreadyInitialized`] if the contract has already been set up.
    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&admin_key()) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&admin_key(), &admin);
        env.storage().instance().set(&counter_key(), &0u64);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Admin authority transfer — two-step handover
    // -----------------------------------------------------------------------

    /// **Step 1 of 2**: Propose a new admin address.
    ///
    /// The current admin authenticates and records `new_admin` as the pending
    /// replacement. No authority is transferred yet — the `new_admin` must call
    /// [`Self::accept_admin`] to complete the handover.
    ///
    /// Calling `transfer_admin` a second time with a different address replaces
    /// any existing pending proposal (useful to correct a typo without having
    /// to cancel first).
    ///
    /// # Arguments
    /// * `new_admin` — The address being proposed as the next admin.
    ///
    /// # Errors
    /// * [`Error::NotAdmin`] if the caller is not the current admin
    ///   (enforced via `require_auth` on the stored admin address).
    /// * [`Error::NotInitialized`] if the contract has not been initialized.
    pub fn transfer_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        if !env.storage().instance().has(&admin_key()) {
            return Err(Error::NotInitialized);
        }
        let current_admin = require_admin(&env);
        env.storage()
            .instance()
            .set(&pending_admin_key(), &new_admin);
        emit_admin_transfer_proposed(&env, &current_admin, &new_admin);
        Ok(())
    }

    /// **Step 2 of 2**: Accept the pending admin proposal.
    ///
    /// The `new_admin` address that was proposed via [`Self::transfer_admin`]
    /// authenticates here, completing the handover. The pending proposal is
    /// cleared and the caller becomes the new admin.
    ///
    /// # Errors
    /// * [`Error::NoPendingAdmin`] if there is no outstanding proposal.
    /// * [`Error::Unauthorized`] if the caller is not the proposed admin
    ///   (enforced via `require_auth` on the pending admin address).
    pub fn accept_admin(env: Env) -> Result<(), Error> {
        let pending: Address = env
            .storage()
            .instance()
            .get(&pending_admin_key())
            .ok_or(Error::NoPendingAdmin)?;
        pending.require_auth();
        env.storage().instance().set(&admin_key(), &pending);
        env.storage().instance().remove(&pending_admin_key());
        emit_admin_transfer_accepted(&env, &pending);
        Ok(())
    }

    /// Cancel a pending admin transfer proposal.
    ///
    /// Only the current admin can cancel. The pending proposal is cleared
    /// without changing the current admin.
    ///
    /// # Errors
    /// * [`Error::NotAdmin`] if the caller is not the current admin.
    /// * [`Error::NoPendingAdmin`] if there is no outstanding proposal to cancel.
    pub fn cancel_transfer(env: Env) -> Result<(), Error> {
        if !env.storage().instance().has(&admin_key()) {
            return Err(Error::NotInitialized);
        }
        let admin = require_admin(&env);
        let pending: Address = env
            .storage()
            .instance()
            .get(&pending_admin_key())
            .ok_or(Error::NoPendingAdmin)?;
        env.storage().instance().remove(&pending_admin_key());
        emit_admin_transfer_cancelled(&env, &admin, &pending);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Admin read-only queries
    // -----------------------------------------------------------------------

    /// Return the current admin address.
    ///
    /// # Errors
    /// * [`Error::NotInitialized`] if the contract has not been initialized.
    pub fn get_admin(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&admin_key())
            .ok_or(Error::NotInitialized)
    }

    /// Return the pending (proposed) admin address, if any.
    ///
    /// Returns `None` when no transfer is in progress.
    pub fn get_pending_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&pending_admin_key())
    }

    // -----------------------------------------------------------------------
    // Escrow — invoice management
    // -----------------------------------------------------------------------

    /// Create a new escrow invoice.
    ///
    /// # Arguments
    /// * `creator`      — Address that creates and owns this invoice.
    /// * `token`        — Token contract (e.g. USDC).
    /// * `total_amount` — Total amount needed to fully fund the invoice.
    /// * `deadline`     — Unix timestamp after which refunds are available.
    ///
    /// Returns the new invoice ID.
    ///
    /// # Errors
    /// * [`Error::NotInitialized`]   — Contract not initialized.
    /// * [`Error::InvalidTotalAmount`] — `total_amount` ≤ 0.
    /// * [`Error::DeadlinePassed`]   — `deadline` is not in the future.
    /// * [`Error::DeadlineOutOfRange`] — `deadline` exceeds the maximum
    ///   horizon of 365 days from the current ledger timestamp.
    pub fn create_invoice(
        env: Env,
        creator: Address,
        token: Address,
        total_amount: i128,
        deadline: u64,
    ) -> Result<u64, Error> {
        if !env.storage().instance().has(&admin_key()) {
            return Err(Error::NotInitialized);
        }
        if total_amount <= 0 {
            return Err(Error::InvalidTotalAmount);
        }
        validate_deadline(&env, deadline)?;
        creator.require_auth();

        let id: u64 = env
            .storage()
            .instance()
            .get(&counter_key())
            .unwrap_or(0u64);
        let next_id = id.checked_add(1).expect("invoice counter overflow");
        env.storage().instance().set(&counter_key(), &next_id);

        let invoice = EscrowInvoice {
            creator: creator.clone(),
            token,
            total_amount,
            funded_amount: 0,
            deadline,
            status: EscrowStatus::Pending,
            refund_reason: OptionalRefundReason::None,
        };
        save_invoice(&env, id, &invoice);

        // Snapshot the current default grace period so later changes to the
        // default never alter the terms of an existing invoice.
        let default_grace: u64 = env
            .storage()
            .instance()
            .get(&default_grace_key())
            .unwrap_or(0);
        if default_grace > 0 {
            save_grace(
                &env,
                id,
                &RefundGrace {
                    grace_secs: default_grace,
                    extension_secs: 0,
                    waived: false,
                },
            );
        }

        emit_invoice_created(&env, id, &creator, total_amount, deadline);
        Ok(id)
    }

    /// Deposit funds toward an invoice.
    ///
    /// Transfers `amount` of the invoice token from `payer` to this contract.
    /// If the deposit completes funding the invoice, the funds are immediately
    /// released to the creator (auto-release on full funding).
    ///
    /// Blacklisted payers (finalised + upheld) are blocked from depositing.
    ///
    /// All preconditions are validated before any storage write or token
    /// transfer, so a failed deposit leaves the invoice and payer balances
    /// untouched.
    ///
    /// # Errors
    /// * [`Error::InvoiceNotFound`]    — Unknown invoice ID.
    /// * [`Error::InvalidStatus`]      — Invoice is not `Pending` or `Active`.
    /// * [`Error::DeadlinePassed`]     — Deposit window has closed.
    /// * [`Error::InvalidAmount`]      — `amount` ≤ 0.
    /// * [`Error::OverFunded`]         — Deposit would exceed `total_amount`.
    /// * [`Error::PayerBlacklisted`]   — Payer is finalised-blacklisted.
    pub fn deposit(env: Env, payer: Address, invoice_id: u64, amount: i128) -> Result<(), Error> {
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        payer.require_auth();

        // Block finalised-blacklisted payers.
        if let Some(entry) = env
            .storage()
            .persistent()
            .get::<_, BlacklistEntry>(&blacklist_key(&payer))
        {
            if entry.finalised && entry.upheld {
                return Err(Error::PayerBlacklisted);
            }
        }

        let mut invoice = get_invoice(&env, invoice_id)?;

        // --- Validate all preconditions before any mutation. ---
        if invoice.status != EscrowStatus::Pending && invoice.status != EscrowStatus::Active {
            return Err(Error::InvalidStatus);
        }
        if env.ledger().timestamp() > invoice.deadline {
            return Err(Error::DeadlinePassed);
        }
        let new_funded = invoice
            .funded_amount
            .checked_add(amount)
            .expect("funded_amount overflow");
        if new_funded > invoice.total_amount {
            return Err(Error::OverFunded);
        }

        // --- Mutations begin here. ---
        // Pull funds from payer into the contract.
        let token_client = token::Client::new(&env, &invoice.token);
        token_client.transfer(&payer, &env.current_contract_address(), &amount);

        // Record per-payer contribution.
        let prev: i128 = env
            .storage()
            .persistent()
            .get(&deposit_key(invoice_id, &payer))
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&deposit_key(invoice_id, &payer), &(prev + amount));

        invoice.funded_amount = new_funded;
        invoice.status = EscrowStatus::Active;

        emit_deposited(&env, invoice_id, &payer, amount, new_funded);

        // Auto-release on full funding, unless a release gate (e.g. pending
        // co-signer approval) still blocks it. A gated invoice stays `Active`
        // and fully funded until `release` is called once the gate clears.
        if new_funded >= invoice.total_amount && check_release_gates(&env, invoice_id).is_ok() {
            execute_release(&env, invoice_id, &mut invoice);
        } else {
            save_invoice(&env, invoice_id, &invoice);
        }

        Ok(())
    }

    /// Release funds to the creator once the invoice is fully funded.
    ///
    /// Anyone may call this after the invoice reaches `total_amount`. Useful
    /// if auto-release did not fire (e.g. due to separate partial deposits,
    /// or because a release gate was still closed when funding completed).
    ///
    /// All preconditions are validated before any storage write or token
    /// transfer, so a failed release leaves state consistent.
    ///
    /// # Errors
    /// * [`Error::InvoiceNotFound`]    — Unknown invoice ID.
    /// * [`Error::InvalidStatus`]      — Invoice was already released/refunded/cancelled.
    /// * [`Error::InsufficientFunding`] — Not fully funded yet.
    /// * [`Error::ApprovalRequired`]   — Co-signer threshold not yet reached.
    /// * [`Error::ApprovalRejected`]   — Co-signers rejected the release.
    /// * [`Error::ReleaseNotYetDue`]  — Scheduled release time not reached.
    /// * [`Error::ScheduleLedgerNotReached`] — Scheduled ledger delay not elapsed.
    /// * [`Error::OracleConditionNotMet`] — Oracle price fails the condition.
    /// * [`Error::OracleUnavailable`] / [`Error::OracleStalePrice`] /
    ///   [`Error::OracleInvalidPrice`] — No usable oracle price.
    pub fn release(env: Env, invoice_id: u64) -> Result<(), Error> {
        let mut invoice = get_invoice(&env, invoice_id)?;

        // --- Validate all preconditions before any mutation. ---
        if invoice.status == EscrowStatus::Released
            || invoice.status == EscrowStatus::Refunded
            || invoice.status == EscrowStatus::Cancelled
        {
            return Err(Error::InvalidStatus);
        }
        if invoice.funded_amount < invoice.total_amount {
            return Err(Error::InsufficientFunding);
        }
        check_release_gates(&env, invoice_id)?;

        let (creator, total) = execute_release(&env, invoice_id, &mut invoice);

        // Emit structured EscrowReleased event for off-chain indexers.
        env.events().publish(
            (symbol_short!("escrow"), symbol_short!("released")),
            EscrowReleased {
                invoice_id,
                recipient: creator,
                amount: total,
            },
        );

        Ok(())
    }

    /// Refund all depositors after the deadline passes without full funding.
    ///
    /// Iterates over `payers` and returns each payer's recorded deposit.
    /// Callable by anyone once the deadline — and any refund grace period
    /// that follows it — has passed and the invoice is not already released.
    ///
    /// The `reason` is persisted on the invoice and emitted with each refund
    /// event so off-chain indexers can attribute the refund.
    ///
    /// All preconditions are validated before any storage write or token
    /// transfer, so a failed refund leaves state consistent.
    ///
    /// # Arguments
    /// * `invoice_id` — Target invoice.
    /// * `payers`     — List of payer addresses to refund in this call.
    /// * `reason`     — Why the refund is being issued.
    ///
    /// # Errors
    /// * [`Error::InvoiceNotFound`]    — Unknown invoice ID.
    /// * [`Error::InvalidStatus`]      — Invoice already released or refunded.
    /// * [`Error::DeadlineNotPassed`]  — Deadline has not expired yet.
    /// * [`Error::RefundGraceActive`]  — Deadline passed but the grace period
    ///   is still running (and has not been waived).
    pub fn refund(env: Env, invoice_id: u64, payers: Vec<Address>, reason: RefundReason) -> Result<(), Error> {
        let mut invoice = get_invoice(&env, invoice_id)?;

        // --- Validate all preconditions before any mutation. ---
        if invoice.status == EscrowStatus::Released
            || invoice.status == EscrowStatus::Refunded
            || invoice.status == EscrowStatus::Cancelled
        {
            return Err(Error::InvalidStatus);
        }
        let now = env.ledger().timestamp();
        if now <= invoice.deadline {
            return Err(Error::DeadlineNotPassed);
        }
        if now <= refund_opens_at(&invoice, &get_grace(&env, invoice_id)) {
            return Err(Error::RefundGraceActive);
        }

        // --- Mutations begin here. ---
        let token_client = token::Client::new(&env, &invoice.token);
        for payer in payers.iter() {
            let amount: i128 = env
                .storage()
                .persistent()
                .get(&deposit_key(invoice_id, &payer))
                .unwrap_or(0);
            if amount > 0 {
                env.storage()
                    .persistent()
                    .remove(&deposit_key(invoice_id, &payer));
                token_client.transfer(&env.current_contract_address(), &payer, &amount);
                emit_refunded(&env, invoice_id, &payer, amount, &reason);
            }
        }

        invoice.status = EscrowStatus::Refunded;
        invoice.refund_reason = OptionalRefundReason::Some(reason);
        save_invoice(&env, invoice_id, &invoice);
        Ok(())
    }

    /// Cancel an invoice. Only the admin may cancel.
    ///
    /// Transitions the invoice to `Cancelled`. If funds are held they must be
    /// refunded separately via [`Self::refund`] (or admin-driven recovery) since
    /// this call does not transfer tokens, avoiding unbounded iteration over
    /// unknown payers.
    ///
    /// All preconditions are validated before any storage write, so a failed
    /// cancellation leaves state consistent.
    ///
    /// # Errors
    /// * [`Error::NotAdmin`]      — Caller is not the admin.
    /// * [`Error::InvoiceNotFound`] — Unknown invoice ID.
    /// * [`Error::InvalidStatus`]   — Invoice not in a cancellable state.
    pub fn cancel_invoice(env: Env, invoice_id: u64) -> Result<(), Error> {
        if !env.storage().instance().has(&admin_key()) {
            return Err(Error::NotInitialized);
        }
        require_admin(&env);
        let mut invoice = get_invoice(&env, invoice_id)?;

        // --- Validate all preconditions before any mutation. ---
        if invoice.status == EscrowStatus::Released
            || invoice.status == EscrowStatus::Refunded
            || invoice.status == EscrowStatus::Cancelled
        {
            return Err(Error::InvalidStatus);
        }

        // --- Mutations begin here. ---
        invoice.status = EscrowStatus::Cancelled;
        save_invoice(&env, invoice_id, &invoice);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Co-signer approval
    // -----------------------------------------------------------------------

    /// Require N-of-M co-signer approval before the invoice can be released.
    ///
    /// Only the invoice creator may configure co-signers, and only while the
    /// invoice is open and no co-signer has voted yet. Calling again before
    /// any vote replaces the previous configuration.
    ///
    /// # Arguments
    /// * `invoice_id` — Target invoice.
    /// * `signers`    — Distinct co-signer addresses (1..=`MAX_COSIGNERS`).
    /// * `threshold`  — Approvals required, in `1..=signers.len()`.
    ///
    /// # Events
    /// Emits `(escrow, cosig_set, id)` with `(signers, threshold)`.
    ///
    /// # Errors
    /// * [`Error::InvoiceNotFound`]       — Unknown invoice ID.
    /// * [`Error::InvalidStatus`]         — Invoice is no longer open.
    /// * [`Error::InvalidCosignerConfig`] — Bad signer list or threshold.
    /// * [`Error::CosignersLocked`]       — A co-signer has already voted.
    pub fn set_cosigners(
        env: Env,
        invoice_id: u64,
        signers: Vec<Address>,
        threshold: u32,
    ) -> Result<(), Error> {
        let invoice = get_invoice(&env, invoice_id)?;
        invoice.creator.require_auth();
        if !is_open(&invoice) {
            return Err(Error::InvalidStatus);
        }

        let count = signers.len();
        if count == 0 || count > MAX_COSIGNERS || threshold == 0 || threshold > count {
            return Err(Error::InvalidCosignerConfig);
        }
        for i in 0..count {
            let signer = signers.get_unchecked(i);
            for j in (i + 1)..count {
                if signers.get_unchecked(j) == signer {
                    return Err(Error::InvalidCosignerConfig);
                }
            }
        }

        if let Some(existing) = get_approval(&env, invoice_id) {
            if !existing.approvals.is_empty() || !existing.rejections.is_empty() {
                return Err(Error::CosignersLocked);
            }
        }

        let approval = CosignerApproval {
            signers: signers.clone(),
            threshold,
            approvals: Vec::new(&env),
            rejections: Vec::new(&env),
            status: ApprovalStatus::Pending,
            updated_ledger: env.ledger().sequence(),
        };
        save_approval(&env, invoice_id, &approval);
        emit_cosigners_set(&env, invoice_id, &signers, threshold);
        Ok(())
    }

    /// Approve the release of an invoice as one of its co-signers.
    ///
    /// The signer authenticates via `require_auth` and must be in the
    /// configured signer list. Each co-signer may vote once; duplicate votes
    /// are rejected, so concurrent or replayed approvals cannot inflate the
    /// tally. When the threshold is reached the status becomes `Approved`.
    ///
    /// Approving does not release funds by itself — call [`Self::release`]
    /// once the invoice is fully funded and approved.
    ///
    /// # Errors
    /// * [`Error::InvoiceNotFound`] — Unknown invoice ID.
    /// * [`Error::InvalidStatus`]   — Invoice is no longer open.
    /// * [`Error::NoCosigners`]     — No co-signers configured.
    /// * [`Error::NotCosigner`]     — `signer` is not a configured co-signer.
    /// * [`Error::AlreadyVoted`]    — `signer` already approved or rejected.
    pub fn approve_release(env: Env, signer: Address, invoice_id: u64) -> Result<ApprovalStatus, Error> {
        cast_vote(&env, signer, invoice_id, true)
    }

    /// Reject the release of an invoice as one of its co-signers.
    ///
    /// Once enough co-signers reject that the threshold can no longer be
    /// reached, the status becomes `Rejected` and release is blocked.
    ///
    /// # Errors
    /// Same as [`Self::approve_release`].
    pub fn reject_release(env: Env, signer: Address, invoice_id: u64) -> Result<ApprovalStatus, Error> {
        cast_vote(&env, signer, invoice_id, false)
    }

    /// Withdraw a previously cast approval or rejection.
    ///
    /// Allowed only while the invoice is open (not yet released). The status
    /// is recomputed, so revoking an approval can move `Approved` back to
    /// `Pending`, and revoking a rejection can move `Rejected` back to
    /// `Pending`.
    ///
    /// # Errors
    /// * [`Error::InvoiceNotFound`] — Unknown invoice ID.
    /// * [`Error::InvalidStatus`]   — Invoice is no longer open.
    /// * [`Error::NoCosigners`]     — No co-signers configured.
    /// * [`Error::NotCosigner`]     — `signer` is not a configured co-signer.
    /// * [`Error::NotVoted`]        — `signer` has no vote to revoke.
    pub fn revoke_vote(env: Env, signer: Address, invoice_id: u64) -> Result<ApprovalStatus, Error> {
        signer.require_auth();
        let invoice = get_invoice(&env, invoice_id)?;
        if !is_open(&invoice) {
            return Err(Error::InvalidStatus);
        }
        let mut approval = get_approval(&env, invoice_id).ok_or(Error::NoCosigners)?;
        if !approval.signers.contains(&signer) {
            return Err(Error::NotCosigner);
        }

        if let Some(idx) = approval.approvals.first_index_of(&signer) {
            approval.approvals.remove(idx);
        } else if let Some(idx) = approval.rejections.first_index_of(&signer) {
            approval.rejections.remove(idx);
        } else {
            return Err(Error::NotVoted);
        }

        let old_status = approval.status.clone();
        recompute_approval_status(&mut approval);
        approval.updated_ledger = env.ledger().sequence();
        save_approval(&env, invoice_id, &approval);

        emit_cosigner_vote(&env, symbol_short!("revoked"), invoice_id, &signer, &approval);
        if old_status != approval.status {
            emit_approval_status_changed(&env, invoice_id, &old_status, &approval.status);
        }
        Ok(approval.status)
    }

    // -----------------------------------------------------------------------
    // Oracle condition guards
    // -----------------------------------------------------------------------

    /// Attach (or replace) a price condition that must hold before release.
    ///
    /// Only the creator may set the condition, and only while the invoice is
    /// still `Pending` (no deposits yet), so payers always fund against a
    /// known condition.
    ///
    /// # Events
    /// Emits `(escrow, orc_set, id)`.
    ///
    /// # Errors
    /// * [`Error::InvoiceNotFound`]     — Unknown invoice ID.
    /// * [`Error::InvalidStatus`]       — Invoice already has deposits or is closed.
    /// * [`Error::InvalidOracleConfig`] — Non-positive threshold, zero
    ///   staleness window, or fallback equal to the primary oracle.
    pub fn set_oracle_condition(env: Env, invoice_id: u64, condition: OracleCondition) -> Result<(), Error> {
        let invoice = get_invoice(&env, invoice_id)?;
        invoice.creator.require_auth();
        if invoice.status != EscrowStatus::Pending {
            return Err(Error::InvalidStatus);
        }
        if condition.threshold_price <= 0
            || condition.max_staleness_secs == 0
            || condition.fallback_oracle.as_ref() == Some(&condition.oracle)
        {
            return Err(Error::InvalidOracleConfig);
        }

        env.storage()
            .persistent()
            .set(&oracle_cond_key(invoice_id), &condition);
        emit_oracle_condition_set(&env, invoice_id, &condition);
        Ok(())
    }

    /// Admin override: remove an invoice's oracle condition.
    ///
    /// Intended as the last-resort fallback when both the primary and
    /// fallback oracles are permanently unavailable, so funds are not locked
    /// forever. Any stored reading is cleared as well.
    ///
    /// # Events
    /// Emits `(escrow, orc_clear, id)` with the admin address.
    ///
    /// # Errors
    /// * [`Error::NotInitialized`]    — Contract not initialized.
    /// * [`Error::InvoiceNotFound`]   — Unknown invoice ID.
    /// * [`Error::InvalidStatus`]     — Invoice is no longer open.
    /// * [`Error::NoOracleCondition`] — No condition to clear.
    pub fn clear_oracle_condition(env: Env, invoice_id: u64) -> Result<(), Error> {
        if !env.storage().instance().has(&admin_key()) {
            return Err(Error::NotInitialized);
        }
        let admin = require_admin(&env);
        let invoice = get_invoice(&env, invoice_id)?;
        if !is_open(&invoice) {
            return Err(Error::InvalidStatus);
        }
        if get_oracle_condition(&env, invoice_id).is_none() {
            return Err(Error::NoOracleCondition);
        }

        env.storage().persistent().remove(&oracle_cond_key(invoice_id));
        env.storage().persistent().remove(&oracle_read_key(invoice_id));
        emit_oracle_condition_cleared(&env, invoice_id, &admin);
        Ok(())
    }

    /// Evaluate the invoice's oracle condition now without changing state.
    ///
    /// Returns the reading (including whether the condition holds) or the
    /// oracle error that would block release.
    ///
    /// # Errors
    /// * [`Error::InvoiceNotFound`]   — Unknown invoice ID.
    /// * [`Error::NoOracleCondition`] — No condition configured.
    /// * [`Error::OracleUnavailable`] / [`Error::OracleStalePrice`] /
    ///   [`Error::OracleInvalidPrice`] — No usable oracle price.
    pub fn check_oracle_condition(env: Env, invoice_id: u64) -> Result<OracleReading, Error> {
        get_invoice(&env, invoice_id)?;
        let cond = get_oracle_condition(&env, invoice_id).ok_or(Error::NoOracleCondition)?;
        evaluate_oracle_condition(&env, invoice_id, &cond)
    }

    // -----------------------------------------------------------------------
    // Scheduled release
    // -----------------------------------------------------------------------

    /// Delay the release of an invoice until a specific time and ledger.
    ///
    /// Release (including auto-release on full funding) is blocked until the
    /// ledger timestamp is `>= release_at` **and** the ledger sequence is
    /// `>= current_sequence + min_ledgers`. Only the creator may schedule,
    /// and only while the invoice is open. Calling again replaces any
    /// existing schedule.
    ///
    /// The schedule and invoice TTLs are extended so both entries remain
    /// live until the schedule is due.
    ///
    /// # Arguments
    /// * `invoice_id`  — Target invoice.
    /// * `release_at`  — Unix timestamp; must be strictly in the future.
    /// * `min_ledgers` — Minimum ledgers to wait (≤ `MAX_SCHEDULE_LEDGERS`).
    ///
    /// # Events
    /// Emits `(escrow, sched_set, id)` with `(release_at, unlock_ledger)`.
    ///
    /// # Errors
    /// * [`Error::InvoiceNotFound`] — Unknown invoice ID.
    /// * [`Error::InvalidStatus`]   — Invoice is no longer open.
    /// * [`Error::InvalidSchedule`] — `release_at` not in the future or
    ///   `min_ledgers` too large.
    pub fn schedule_release(
        env: Env,
        invoice_id: u64,
        release_at: u64,
        min_ledgers: u32,
    ) -> Result<ReleaseSchedule, Error> {
        let invoice = get_invoice(&env, invoice_id)?;
        invoice.creator.require_auth();
        if !is_open(&invoice) {
            return Err(Error::InvalidStatus);
        }

        let now = env.ledger().timestamp();
        let seq = env.ledger().sequence();
        if release_at <= now || min_ledgers > MAX_SCHEDULE_LEDGERS {
            return Err(Error::InvalidSchedule);
        }
        let unlock_ledger = seq.checked_add(min_ledgers).ok_or(Error::InvalidSchedule)?;

        let schedule = ReleaseSchedule {
            release_at,
            unlock_ledger,
            scheduled_at: now,
            scheduled_ledger: seq,
            status: ScheduleStatus::Scheduled,
        };
        save_schedule(&env, invoice_id, &schedule);
        emit_schedule_set(&env, invoice_id, &schedule);
        Ok(schedule)
    }

    /// Cancel an active release schedule so it no longer gates release.
    ///
    /// Only the creator may cancel, and only while the invoice is open.
    ///
    /// # Events
    /// Emits `(escrow, sched_cnl, id)`.
    ///
    /// # Errors
    /// * [`Error::InvoiceNotFound`]  — Unknown invoice ID.
    /// * [`Error::InvalidStatus`]    — Invoice is no longer open.
    /// * [`Error::ScheduleNotFound`] — No active schedule.
    pub fn cancel_release_schedule(env: Env, invoice_id: u64) -> Result<(), Error> {
        let invoice = get_invoice(&env, invoice_id)?;
        invoice.creator.require_auth();
        if !is_open(&invoice) {
            return Err(Error::InvalidStatus);
        }
        let mut schedule = get_schedule(&env, invoice_id)
            .filter(|s| s.status == ScheduleStatus::Scheduled)
            .ok_or(Error::ScheduleNotFound)?;

        schedule.status = ScheduleStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&schedule_key(invoice_id), &schedule);
        emit_schedule_cancelled(&env, invoice_id);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Refund grace period
    // -----------------------------------------------------------------------

    /// Set the default refund grace period applied to invoices created from
    /// now on. Existing invoices keep the grace period they were created with.
    ///
    /// # Events
    /// Emits `(escrow, grace_def)` with `grace_secs`.
    ///
    /// # Errors
    /// * [`Error::NotInitialized`]     — Contract not initialized.
    /// * [`Error::InvalidGracePeriod`] — `grace_secs > MAX_REFUND_GRACE_SECS`.
    pub fn set_default_refund_grace(env: Env, grace_secs: u64) -> Result<(), Error> {
        if !env.storage().instance().has(&admin_key()) {
            return Err(Error::NotInitialized);
        }
        require_admin(&env);
        if grace_secs > MAX_REFUND_GRACE_SECS {
            return Err(Error::InvalidGracePeriod);
        }
        env.storage().instance().set(&default_grace_key(), &grace_secs);
        emit_default_grace_set(&env, grace_secs);
        Ok(())
    }

    /// Return the default refund grace period (seconds) for new invoices.
    pub fn get_default_refund_grace(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&default_grace_key())
            .unwrap_or(0)
    }

    /// Set an invoice's own refund grace period, overriding the default it
    /// was created with.
    ///
    /// Only the creator may do this, and only while the invoice is still
    /// `Pending` (no deposits yet), so payers always fund against known
    /// refund terms. Any admin extension is reset.
    ///
    /// # Events
    /// Emits `(escrow, grace_set, id)`.
    ///
    /// # Errors
    /// * [`Error::InvoiceNotFound`]    — Unknown invoice ID.
    /// * [`Error::InvalidStatus`]      — Invoice already has deposits or is closed.
    /// * [`Error::InvalidGracePeriod`] — `grace_secs > MAX_REFUND_GRACE_SECS`.
    pub fn set_refund_grace(env: Env, invoice_id: u64, grace_secs: u64) -> Result<(), Error> {
        let invoice = get_invoice(&env, invoice_id)?;
        invoice.creator.require_auth();
        if invoice.status != EscrowStatus::Pending {
            return Err(Error::InvalidStatus);
        }
        if grace_secs > MAX_REFUND_GRACE_SECS {
            return Err(Error::InvalidGracePeriod);
        }

        let grace = RefundGrace {
            grace_secs,
            extension_secs: 0,
            waived: false,
        };
        save_grace(&env, invoice_id, &grace);
        emit_grace_changed(&env, symbol_short!("grace_set"), invoice_id, &grace);
        Ok(())
    }

    /// Admin override: extend an invoice's refund grace period.
    ///
    /// Allowed while the invoice is open and before the grace period has
    /// elapsed or been waived. The total grace (base + extensions) may not
    /// exceed `MAX_REFUND_GRACE_SECS`.
    ///
    /// # Events
    /// Emits `(escrow, grace_ext, id)`.
    ///
    /// # Errors
    /// * [`Error::NotInitialized`]     — Contract not initialized.
    /// * [`Error::InvoiceNotFound`]    — Unknown invoice ID.
    /// * [`Error::InvalidStatus`]      — Invoice is no longer open.
    /// * [`Error::InvalidGracePeriod`] — Zero extension or total too long.
    /// * [`Error::RefundGraceElapsed`] — Grace already elapsed or waived.
    pub fn extend_refund_grace(env: Env, invoice_id: u64, extra_secs: u64) -> Result<(), Error> {
        if !env.storage().instance().has(&admin_key()) {
            return Err(Error::NotInitialized);
        }
        require_admin(&env);
        let invoice = get_invoice(&env, invoice_id)?;
        if !is_open(&invoice) {
            return Err(Error::InvalidStatus);
        }

        let current = get_grace(&env, invoice_id);
        match grace_status(&env, &invoice, &current) {
            RefundGraceStatus::Elapsed | RefundGraceStatus::Waived => {
                return Err(Error::RefundGraceElapsed)
            }
            RefundGraceStatus::NoGrace if env.ledger().timestamp() > invoice.deadline => {
                // No grace and deadline passed: refunds are already open.
                return Err(Error::RefundGraceElapsed);
            }
            _ => {}
        }

        let mut grace = current.unwrap_or(RefundGrace {
            grace_secs: 0,
            extension_secs: 0,
            waived: false,
        });
        let total = grace
            .grace_secs
            .checked_add(grace.extension_secs)
            .and_then(|t| t.checked_add(extra_secs))
            .ok_or(Error::InvalidGracePeriod)?;
        if extra_secs == 0 || total > MAX_REFUND_GRACE_SECS {
            return Err(Error::InvalidGracePeriod);
        }

        grace.extension_secs += extra_secs;
        save_grace(&env, invoice_id, &grace);
        emit_grace_changed(&env, symbol_short!("grace_ext"), invoice_id, &grace);
        Ok(())
    }

    /// Admin override: waive an invoice's refund grace period so refunds can
    /// be processed as soon as the deadline has passed.
    ///
    /// # Events
    /// Emits `(escrow, grace_wvd, id)`.
    ///
    /// # Errors
    /// * [`Error::NotInitialized`]     — Contract not initialized.
    /// * [`Error::InvoiceNotFound`]    — Unknown invoice ID.
    /// * [`Error::InvalidStatus`]      — Invoice is no longer open.
    /// * [`Error::RefundGraceElapsed`] — No grace to waive (none configured,
    ///   already elapsed, or already waived).
    pub fn waive_refund_grace(env: Env, invoice_id: u64) -> Result<(), Error> {
        if !env.storage().instance().has(&admin_key()) {
            return Err(Error::NotInitialized);
        }
        require_admin(&env);
        let invoice = get_invoice(&env, invoice_id)?;
        if !is_open(&invoice) {
            return Err(Error::InvalidStatus);
        }

        let current = get_grace(&env, invoice_id);
        match grace_status(&env, &invoice, &current) {
            RefundGraceStatus::NotStarted | RefundGraceStatus::Active => {}
            _ => return Err(Error::RefundGraceElapsed),
        }

        // `grace_status` only reports NotStarted/Active when a record exists.
        let mut grace = current.unwrap();
        grace.waived = true;
        save_grace(&env, invoice_id, &grace);
        emit_grace_changed(&env, symbol_short!("grace_wvd"), invoice_id, &grace);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Payer blacklist
    // -----------------------------------------------------------------------

    /// Blacklist a payer, preventing them from depositing into any invoice.
    ///
    /// Only the contract admin can blacklist a payer. The blacklist entry
    /// starts with `finalised: false`, giving the payer a window to submit
    /// an appeal via [`Self::submit_appeal`].
    ///
    /// # Arguments
    /// * `admin` — Contract admin address (must authenticate).
    /// * `payer` — Address to blacklist.
    /// * `reason_hash` — 32-byte hash describing the reason for the blacklist.
    ///
    /// # Events
    /// Emits `("blacklist", "bl_add")` with `(payer, reason_hash, banned_at)`.
    ///
    /// # Errors
    /// * [`Error::NotAdmin`] if caller is not the admin.
    /// * [`Error::NotInitialized`] if contract is not initialized.
    pub fn blacklist_payer(
        env: Env,
        admin: Address,
        payer: Address,
        reason_hash: BytesN<32>,
    ) -> Result<(), Error> {
        if !env.storage().instance().has(&admin_key()) {
            return Err(Error::NotInitialized);
        }
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&admin_key())
            .unwrap();
        if admin != stored_admin {
            return Err(Error::NotAdmin);
        }
        admin.require_auth();

        let now = env.ledger().timestamp();
        let entry = BlacklistEntry {
            banned_at: now,
            appeal_hash: None,
            finalised: false,
            upheld: false,
            reason_hash: reason_hash.clone(),
        };
        env.storage()
            .persistent()
            .set(&blacklist_key(&payer), &entry);
        emit_payer_blacklisted(&env, &payer, &reason_hash, now);
        Ok(())
    }

    /// Submit an appeal against a blacklist entry.
    ///
    /// Only the blacklisted payer may call this, and only within
    /// `APPEAL_WINDOW_LEDGERS` ledgers of being blacklisted. The appeal
    /// hash should be a commitment to the appeal justification (revealed
    /// off-chain if needed).
    ///
    /// # Arguments
    /// * `payer` — The blacklisted payer (must authenticate).
    /// * `invoice_id` — An invoice ID for context (logged in event).
    /// * `appeal_hash` — 32-byte hash of the appeal justification.
    ///
    /// # Events
    /// Emits `("blacklist", "appeal")` with `(payer, appeal_hash)`.
    ///
    /// # Errors
    /// * [`Error::NotBlacklisted`] if payer has no blacklist entry.
    /// * [`Error::AppealWindowExpired`] if the appeal window has passed.
    /// * [`Error::AlreadyFinalised`] if the blacklist is already finalised.
    /// * [`Error::NotBlacklistedPayer`] if caller is not the blacklisted payer.
    pub fn submit_appeal(
        env: Env,
        payer: Address,
        invoice_id: u64,
        appeal_hash: BytesN<32>,
    ) -> Result<(), Error> {
        payer.require_auth();

        let mut entry: BlacklistEntry = env
            .storage()
            .persistent()
            .get(&blacklist_key(&payer))
            .ok_or(Error::NotBlacklisted)?;

        if entry.finalised {
            return Err(Error::AlreadyFinalised);
        }

        // Check appeal window: must be within APPEAL_WINDOW_LEDGERS of ban.
        // Uses rough timestamp conversion (~5s per ledger). For production,
        // store `ban_ledger` in BlacklistEntry for precise ledger-based check.
        let now = env.ledger().timestamp();
        let appeal_deadline = entry
            .banned_at
            .saturating_add(APPEAL_WINDOW_LEDGERS as u64 * 5);
        if now > appeal_deadline {
            return Err(Error::AppealWindowExpired);
        }

        entry.appeal_hash = Some(appeal_hash.clone());
        env.storage()
            .persistent()
            .set(&blacklist_key(&payer), &entry);
        emit_appeal_submitted(&env, &payer, &appeal_hash);

        // Log invoice context in event
        env.events().publish(
            (symbol_short!("appeal"), invoice_id),
            payer,
        );
        Ok(())
    }

    /// Finalise a blacklist entry — either uphold the ban or reinstate the payer.
    ///
    /// Only the admin may call this. If `uphold` is `true`, the blacklist
    /// entry is finalised and the payer remains blocked. If `uphold` is
    /// `false`, the blacklist entry is removed entirely and the payer is
    /// reinstated.
    ///
    /// # Arguments
    /// * `admin` — Contract admin (must authenticate).
    /// * `payer` — The blacklisted payer to finalise.
    /// * `uphold` — `true` to keep the ban, `false` to reinstate.
    ///
    /// # Events
    /// Emits `("blacklist", "bl_fin")` with `(payer, upheld)`.
    ///
    /// # Errors
    /// * [`Error::NotAdmin`] if caller is not the admin.
    /// * [`Error::NotBlacklisted`] if payer has no blacklist entry.
    /// * [`Error::AlreadyFinalised`] if already finalised.
    pub fn finalise_blacklist(
        env: Env,
        admin: Address,
        payer: Address,
        uphold: bool,
    ) -> Result<(), Error> {
        if !env.storage().instance().has(&admin_key()) {
            return Err(Error::NotInitialized);
        }
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&admin_key())
            .unwrap();
        if admin != stored_admin {
            return Err(Error::NotAdmin);
        }
        admin.require_auth();

        let mut entry: BlacklistEntry = env
            .storage()
            .persistent()
            .get(&blacklist_key(&payer))
            .ok_or(Error::NotBlacklisted)?;

        if entry.finalised {
            return Err(Error::AlreadyFinalised);
        }

        if uphold {
            entry.finalised = true;
            entry.upheld = true;
            env.storage()
                .persistent()
                .set(&blacklist_key(&payer), &entry);
        } else {
            // Reinstate: remove the blacklist entry.
            env.storage()
                .persistent()
                .remove(&blacklist_key(&payer));
        }

        emit_blacklist_finalised(&env, &payer, uphold);
        Ok(())
    }

    /// Pay an invoice — delegates to [`Self::deposit`] which checks the
    /// payer blacklist before processing.
    ///
    /// If the payer is blacklisted (`finalised` and `upheld`), the
    /// call returns [`Error::PayerBlacklisted`].
    ///
    /// # Arguments
    /// * `payer` — Address making the payment (must authenticate).
    /// * `invoice_id` — Target invoice ID.
    /// * `amount` — Amount to pay toward the invoice.
    ///
    /// # Errors
    /// * [`Error::PayerBlacklisted`] — Payer is on the blacklist and
    ///   the entry is finalised with `upheld: true`.
    /// * See [`Self::deposit`] for all other errors.
    pub fn pay_invoice(
        env: Env,
        payer: Address,
        invoice_id: u64,
        amount: i128,
    ) -> Result<(), Error> {
        Self::deposit(env, payer, invoice_id, amount)
    }

    // -----------------------------------------------------------------------
    // Read-only queries
    // -----------------------------------------------------------------------

    /// Return the full invoice record.
    ///
    /// # Errors
    /// * [`Error::InvoiceNotFound`] — Unknown invoice ID.
    pub fn get_invoice(env: Env, invoice_id: u64) -> Result<EscrowInvoice, Error> {
        get_invoice(&env, invoice_id)
    }

    /// Return the current locked balance held in escrow for a given invoice.
    ///
    /// This is a read-only view function — it performs no state mutations.
    /// Returns `0` when no escrow entry exists for `invoice_id` (i.e. the
    /// invoice has never been created or has already been released/refunded).
    ///
    /// # Arguments
    /// * `invoice_id` — The ID of the invoice to query.
    ///
    /// # Returns
    /// The `funded_amount` currently locked in this escrow invoice, or `0`
    /// if the invoice does not exist.
    pub fn get_escrow_balance(env: Env, invoice_id: u64) -> i128 {
        env.storage()
            .persistent()
            .get::<_, EscrowInvoice>(&invoice_key(invoice_id))
            .map(|inv| inv.funded_amount)
            .unwrap_or(0)
    }

    /// Return the amount a specific payer has deposited toward an invoice.
    pub fn get_deposit(env: Env, invoice_id: u64, payer: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&deposit_key(invoice_id, &payer))
            .unwrap_or(0)
    }

    /// Return the blacklist entry for a payer, if any.
    pub fn get_blacklist_entry(env: Env, payer: Address) -> Option<BlacklistEntry> {
        env.storage()
            .persistent()
            .get(&blacklist_key(&payer))
    }

    /// Return the full co-signer approval record for an invoice, if any.
    pub fn get_approval(env: Env, invoice_id: u64) -> Option<CosignerApproval> {
        get_approval(&env, invoice_id)
    }

    /// Return the aggregate co-signer approval status, or `None` when the
    /// invoice has no co-signer requirement.
    pub fn get_approval_status(env: Env, invoice_id: u64) -> Option<ApprovalStatus> {
        get_approval(&env, invoice_id).map(|a| a.status)
    }

    /// Return `true` if `signer` has approved the invoice's release.
    pub fn has_approved(env: Env, invoice_id: u64, signer: Address) -> bool {
        get_approval(&env, invoice_id).map_or(false, |a| a.approvals.contains(&signer))
    }

    /// Return how many more approvals are needed to reach the threshold
    /// (`0` when already approved or when no co-signers are configured).
    pub fn approvals_remaining(env: Env, invoice_id: u64) -> u32 {
        get_approval(&env, invoice_id).map_or(0, |a| a.threshold.saturating_sub(a.approvals.len()))
    }

    /// Return the oracle condition attached to an invoice, if any.
    pub fn get_oracle_condition(env: Env, invoice_id: u64) -> Option<OracleCondition> {
        get_oracle_condition(&env, invoice_id)
    }

    /// Return the last oracle reading recorded during a release attempt
    /// (e.g. a deposit that completed funding), if any.
    pub fn get_last_oracle_reading(env: Env, invoice_id: u64) -> Option<OracleReading> {
        env.storage().persistent().get(&oracle_read_key(invoice_id))
    }

    /// Return the release schedule for an invoice, if any.
    pub fn get_release_schedule(env: Env, invoice_id: u64) -> Option<ReleaseSchedule> {
        get_schedule(&env, invoice_id)
    }

    /// Return `true` when the invoice has no active schedule or its schedule
    /// is due (both the time and ledger bounds have passed).
    pub fn is_release_due(env: Env, invoice_id: u64) -> bool {
        get_schedule(&env, invoice_id).map_or(true, |s| {
            s.status != ScheduleStatus::Scheduled
                || (env.ledger().timestamp() >= s.release_at
                    && env.ledger().sequence() >= s.unlock_ledger)
        })
    }

    /// Seconds remaining until the scheduled release time (`0` when due or
    /// when there is no active schedule).
    pub fn seconds_until_release(env: Env, invoice_id: u64) -> u64 {
        get_schedule(&env, invoice_id)
            .filter(|s| s.status == ScheduleStatus::Scheduled)
            .map_or(0, |s| s.release_at.saturating_sub(env.ledger().timestamp()))
    }

    /// Ledgers remaining until the scheduled unlock ledger (`0` when reached
    /// or when there is no active schedule).
    pub fn ledgers_until_release(env: Env, invoice_id: u64) -> u32 {
        get_schedule(&env, invoice_id)
            .filter(|s| s.status == ScheduleStatus::Scheduled)
            .map_or(0, |s| s.unlock_ledger.saturating_sub(env.ledger().sequence()))
    }

    /// Return the refund grace configuration for an invoice, if any.
    pub fn get_refund_grace(env: Env, invoice_id: u64) -> Option<RefundGrace> {
        get_grace(&env, invoice_id)
    }

    /// Return where the invoice is in its refund grace timeline.
    ///
    /// # Errors
    /// * [`Error::InvoiceNotFound`] — Unknown invoice ID.
    pub fn get_refund_grace_status(env: Env, invoice_id: u64) -> Result<RefundGraceStatus, Error> {
        let invoice = get_invoice(&env, invoice_id)?;
        Ok(grace_status(&env, &invoice, &get_grace(&env, invoice_id)))
    }

    /// Return the timestamp after which refunds may be processed (deadline
    /// plus any un-waived grace period).
    ///
    /// # Errors
    /// * [`Error::InvoiceNotFound`] — Unknown invoice ID.
    pub fn refund_available_after(env: Env, invoice_id: u64) -> Result<u64, Error> {
        let invoice = get_invoice(&env, invoice_id)?;
        Ok(refund_opens_at(&invoice, &get_grace(&env, invoice_id)))
    }

    /// Seconds left on the refund grace timer.
    ///
    /// Before the deadline this is the full grace window; after the deadline
    /// it counts down to `0`. Always `0` when there is no grace period or it
    /// has been waived.
    ///
    /// # Errors
    /// * [`Error::InvoiceNotFound`] — Unknown invoice ID.
    pub fn refund_grace_remaining(env: Env, invoice_id: u64) -> Result<u64, Error> {
        let invoice = get_invoice(&env, invoice_id)?;
        let opens_at = refund_opens_at(&invoice, &get_grace(&env, invoice_id));
        let from = env.ledger().timestamp().max(invoice.deadline);
        Ok(opens_at.saturating_sub(from))
    }

    /// Return `true` if the payer is blacklisted with a finalised and upheld entry.
    pub fn is_payer_blacklisted(env: Env, payer: Address) -> bool {
        env.storage()
            .persistent()
            .get::<_, BlacklistEntry>(&blacklist_key(&payer))
            .map_or(false, |entry| entry.finalised && entry.upheld)
    }
}