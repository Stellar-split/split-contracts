//! Advanced recipient approval workflow (issue #855).
//!
//! An invoice creator can route an escrow release to a set of recipients
//! instead of to themselves. The split only takes effect once the recipients
//! agree to it:
//!
//! - `propose_recipients` records recipients, basis-point shares, an N-of-M
//!   approval threshold and a voting deadline. Re-proposing bumps the version
//!   and clears every vote, so recipients always vote on the exact split.
//! - Recipients `approve_recipients` / `reject_recipients` a specific version,
//!   and may `revoke_recipient_approval` while the proposal is pending.
//! - Once `required_approvals` is reached the proposal is `Approved`. If the
//!   invoice is already fully funded, the release fires immediately.
//! - If so many recipients reject that the threshold can no longer be met, the
//!   proposal becomes `Rejected`; the creator must revise or withdraw it.
//!
//! While a proposal exists and is not `Approved`, the invoice cannot be
//! released (auto-release on full funding is deferred, and `release` returns
//! [`Error::RecipientsNotApproved`]).

use soroban_sdk::{symbol_short, token, Address, BytesN, Env, Symbol, Vec};

use crate::errors::Error;
use crate::types::{EscrowStatus, ProposalStatus, RecipientProposal};
use crate::{get_invoice, settle_release};

/// Maximum number of recipients in a single proposal.
pub(crate) const MAX_RECIPIENTS: u32 = 20;

const BPS_DENOMINATOR: u32 = 10_000;

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Persistent storage: recipient proposal keyed by invoice ID.
fn proposal_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("rcp_prop"), invoice_id)
}

pub(crate) fn get_proposal(env: &Env, invoice_id: u64) -> Option<RecipientProposal> {
    env.storage().persistent().get(&proposal_key(invoice_id))
}

fn save_proposal(env: &Env, invoice_id: u64, proposal: &RecipientProposal) {
    env.storage()
        .persistent()
        .set(&proposal_key(invoice_id), proposal);
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Topics: `(approval, proposed, id)` — Data: `(version, recipient_count, required, deadline)`
fn emit_proposed(env: &Env, invoice_id: u64, p: &RecipientProposal) {
    env.events().publish(
        (symbol_short!("approval"), symbol_short!("proposed"), invoice_id),
        (
            p.version,
            p.recipients.len(),
            p.required_approvals,
            p.approval_deadline,
        ),
    );
}

/// Topics: `(approval, vote_yes, id)` — Data: `(recipient, version, approval_count)`
fn emit_vote_yes(env: &Env, invoice_id: u64, recipient: &Address, version: u32, count: u32) {
    env.events().publish(
        (symbol_short!("approval"), symbol_short!("vote_yes"), invoice_id),
        (recipient.clone(), version, count),
    );
}

/// Topics: `(approval, vote_no, id)` — Data: `(recipient, version, reason_hash)`
fn emit_vote_no(
    env: &Env,
    invoice_id: u64,
    recipient: &Address,
    version: u32,
    reason_hash: &BytesN<32>,
) {
    env.events().publish(
        (symbol_short!("approval"), symbol_short!("vote_no"), invoice_id),
        (recipient.clone(), version, reason_hash.clone()),
    );
}

/// Topics: `(approval, revoked, id)` — Data: `(recipient, version)`
fn emit_revoked(env: &Env, invoice_id: u64, recipient: &Address, version: u32) {
    env.events().publish(
        (symbol_short!("approval"), symbol_short!("revoked"), invoice_id),
        (recipient.clone(), version),
    );
}

/// Topics: `(approval, approved, id)` — Data: `version`
fn emit_approved(env: &Env, invoice_id: u64, version: u32) {
    env.events().publish(
        (symbol_short!("approval"), symbol_short!("approved"), invoice_id),
        version,
    );
}

/// Topics: `(approval, rejected, id)` — Data: `version`
fn emit_rejected(env: &Env, invoice_id: u64, version: u32) {
    env.events().publish(
        (symbol_short!("approval"), symbol_short!("rejected"), invoice_id),
        version,
    );
}

/// Topics: `(approval, withdrawn, id)` — Data: `version`
fn emit_withdrawn(env: &Env, invoice_id: u64, version: u32) {
    env.events().publish(
        (symbol_short!("approval"), symbol_short!("withdrawn"), invoice_id),
        version,
    );
}

/// Topics: `(approval, payout, id)` — Data: `(recipient, amount)`
fn emit_payout(env: &Env, invoice_id: u64, recipient: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("approval"), symbol_short!("payout"), invoice_id),
        (recipient.clone(), amount),
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_open(status: &EscrowStatus) -> bool {
    *status == EscrowStatus::Pending || *status == EscrowStatus::Active
}

/// Load a pending proposal and validate the common vote preconditions.
fn load_for_vote(
    env: &Env,
    recipient: &Address,
    invoice_id: u64,
    version: u32,
) -> Result<RecipientProposal, Error> {
    let proposal = get_proposal(env, invoice_id).ok_or(Error::ProposalNotFound)?;
    if proposal.version != version {
        return Err(Error::ProposalVersionMismatch);
    }
    if proposal.status != ProposalStatus::Pending {
        return Err(Error::ProposalClosed);
    }
    if env.ledger().timestamp() > proposal.approval_deadline {
        return Err(Error::DeadlinePassed);
    }
    if !proposal.recipients.contains(recipient) {
        return Err(Error::NotARecipient);
    }
    Ok(proposal)
}

/// `true` when the invoice may be released: either no recipient proposal
/// exists, or the proposal has been approved.
pub(crate) fn release_allowed(env: &Env, invoice_id: u64) -> bool {
    match get_proposal(env, invoice_id) {
        None => true,
        Some(p) => p.status == ProposalStatus::Approved,
    }
}

/// Pay `amount` of `token` to the approved recipients according to their
/// shares. The last recipient receives any rounding remainder.
///
/// Returns `false` (and transfers nothing) when there is no approved
/// proposal, in which case the caller pays the creator instead.
pub(crate) fn distribute(env: &Env, invoice_id: u64, token: &Address, amount: i128) -> bool {
    let proposal = match get_proposal(env, invoice_id) {
        Some(p) if p.status == ProposalStatus::Approved => p,
        _ => return false,
    };
    let token_client = token::Client::new(env, token);
    let contract = env.current_contract_address();
    let last = proposal.recipients.len() - 1;
    let mut paid: i128 = 0;
    for (i, recipient) in proposal.recipients.iter().enumerate() {
        let share = if i as u32 == last {
            amount - paid
        } else {
            let bps = proposal.shares_bps.get_unchecked(i as u32) as i128;
            amount
                .checked_mul(bps)
                .expect("recipient share overflow")
                / BPS_DENOMINATOR as i128
        };
        paid += share;
        if share > 0 {
            token_client.transfer(&contract, &recipient, &share);
        }
        emit_payout(env, invoice_id, &recipient, share);
    }
    true
}

// ---------------------------------------------------------------------------
// Entry points (wrapped by the contract impl in lib.rs)
// ---------------------------------------------------------------------------

pub(crate) fn propose(
    env: &Env,
    creator: Address,
    invoice_id: u64,
    recipients: Vec<Address>,
    shares_bps: Vec<u32>,
    required_approvals: u32,
    approval_deadline: u64,
) -> Result<u32, Error> {
    let invoice = get_invoice(env, invoice_id)?;
    if invoice.creator != creator {
        return Err(Error::Unauthorized);
    }
    creator.require_auth();
    if !is_open(&invoice.status) {
        return Err(Error::InvalidStatus);
    }
    if crate::lending::has_active_loan(env, invoice_id) {
        return Err(Error::ActiveLoanExists);
    }

    let n = recipients.len();
    if n == 0 || n > MAX_RECIPIENTS || n != shares_bps.len() {
        return Err(Error::InvalidRecipients);
    }
    for i in 0..n {
        let a = recipients.get_unchecked(i);
        for j in (i + 1)..n {
            if a == recipients.get_unchecked(j) {
                return Err(Error::InvalidRecipients);
            }
        }
    }
    let mut sum: u32 = 0;
    for bps in shares_bps.iter() {
        if bps == 0 {
            return Err(Error::InvalidShares);
        }
        sum = sum.checked_add(bps).ok_or(Error::InvalidShares)?;
    }
    if sum != BPS_DENOMINATOR {
        return Err(Error::InvalidShares);
    }
    if required_approvals == 0 || required_approvals > n {
        return Err(Error::InvalidThreshold);
    }
    if approval_deadline <= env.ledger().timestamp() {
        return Err(Error::DeadlinePassed);
    }

    let version = get_proposal(env, invoice_id).map_or(1, |p| p.version.saturating_add(1));
    let proposal = RecipientProposal {
        version,
        recipients,
        shares_bps,
        required_approvals,
        approvals: Vec::new(env),
        rejections: Vec::new(env),
        approval_deadline,
        status: ProposalStatus::Pending,
    };
    save_proposal(env, invoice_id, &proposal);
    emit_proposed(env, invoice_id, &proposal);
    Ok(version)
}

pub(crate) fn approve(
    env: &Env,
    recipient: Address,
    invoice_id: u64,
    version: u32,
) -> Result<ProposalStatus, Error> {
    recipient.require_auth();
    let mut proposal = load_for_vote(env, &recipient, invoice_id, version)?;
    if proposal.approvals.contains(&recipient) || proposal.rejections.contains(&recipient) {
        return Err(Error::AlreadyVoted);
    }

    proposal.approvals.push_back(recipient.clone());
    emit_vote_yes(
        env,
        invoice_id,
        &recipient,
        version,
        proposal.approvals.len(),
    );

    if proposal.approvals.len() >= proposal.required_approvals {
        proposal.status = ProposalStatus::Approved;
    }
    save_proposal(env, invoice_id, &proposal);

    if proposal.status == ProposalStatus::Approved {
        emit_approved(env, invoice_id, version);
        // Auto-release a fully funded invoice that was waiting on approval.
        let mut invoice = get_invoice(env, invoice_id)?;
        if invoice.status == EscrowStatus::Active
            && invoice.funded_amount >= invoice.total_amount
        {
            settle_release(env, invoice_id, &mut invoice);
        }
    }
    Ok(proposal.status)
}

pub(crate) fn reject(
    env: &Env,
    recipient: Address,
    invoice_id: u64,
    version: u32,
    reason_hash: BytesN<32>,
) -> Result<ProposalStatus, Error> {
    recipient.require_auth();
    let mut proposal = load_for_vote(env, &recipient, invoice_id, version)?;
    if proposal.approvals.contains(&recipient) || proposal.rejections.contains(&recipient) {
        return Err(Error::AlreadyVoted);
    }

    proposal.rejections.push_back(recipient.clone());
    emit_vote_no(env, invoice_id, &recipient, version, &reason_hash);

    let still_possible = proposal.recipients.len() - proposal.rejections.len();
    if still_possible < proposal.required_approvals {
        proposal.status = ProposalStatus::Rejected;
        emit_rejected(env, invoice_id, version);
    }
    save_proposal(env, invoice_id, &proposal);
    Ok(proposal.status)
}

pub(crate) fn revoke(env: &Env, recipient: Address, invoice_id: u64) -> Result<(), Error> {
    recipient.require_auth();
    let mut proposal = get_proposal(env, invoice_id).ok_or(Error::ProposalNotFound)?;
    if proposal.status != ProposalStatus::Pending {
        return Err(Error::ProposalClosed);
    }
    let idx = proposal
        .approvals
        .first_index_of(&recipient)
        .ok_or(Error::NotARecipient)?;
    proposal.approvals.remove(idx);
    save_proposal(env, invoice_id, &proposal);
    emit_revoked(env, invoice_id, &recipient, proposal.version);
    Ok(())
}

pub(crate) fn withdraw(env: &Env, creator: Address, invoice_id: u64) -> Result<(), Error> {
    let invoice = get_invoice(env, invoice_id)?;
    if invoice.creator != creator {
        return Err(Error::Unauthorized);
    }
    creator.require_auth();
    if !is_open(&invoice.status) {
        return Err(Error::InvalidStatus);
    }
    let proposal = get_proposal(env, invoice_id).ok_or(Error::ProposalNotFound)?;
    env.storage().persistent().remove(&proposal_key(invoice_id));
    emit_withdrawn(env, invoice_id, proposal.version);
    Ok(())
}
