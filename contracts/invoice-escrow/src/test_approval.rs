//! Tests for the recipient approval workflow (#855).

use crate::errors::Error;
use crate::test_helpers::{setup, Ctx};
use crate::types::{EscrowStatus, ProposalStatus};
use soroban_sdk::{symbol_short, vec, Address, BytesN, Vec};

const DEADLINE: u64 = 500;

/// Creator, invoice of 1 000, two recipients (60/40) with a 2-of-2 threshold.
fn proposed(ctx: &Ctx) -> (Address, u64, Address, Address) {
    let creator = ctx.user();
    let id = ctx.invoice(&creator, 1_000);
    let r1 = ctx.user();
    let r2 = ctx.user();
    let version = ctx.client.propose_recipients(
        &creator,
        &id,
        &vec![&ctx.env, r1.clone(), r2.clone()],
        &vec![&ctx.env, 6_000u32, 4_000u32],
        &2u32,
        &DEADLINE,
    );
    assert_eq!(version, 1);
    (creator, id, r1, r2)
}

fn reason(ctx: &Ctx) -> BytesN<32> {
    BytesN::from_array(&ctx.env, &[7u8; 32])
}

// ---------------------------------------------------------------------------
// Proposal validation
// ---------------------------------------------------------------------------

#[test]
fn test_propose_stores_proposal_and_emits_event() {
    let ctx = setup();
    let (_, id, r1, r2) = proposed(&ctx);
    assert!(ctx.has_event(symbol_short!("approval"), symbol_short!("proposed"), id));

    let p = ctx.client.get_recipient_proposal(&id).unwrap();
    assert_eq!(p.version, 1);
    assert_eq!(p.recipients, vec![&ctx.env, r1, r2]);
    assert_eq!(p.required_approvals, 2);
    assert_eq!(p.status, ProposalStatus::Pending);
    assert!(p.approvals.is_empty());
    assert!(!ctx.client.is_release_approved(&id));
}

#[test]
fn test_propose_by_non_creator_fails() {
    let ctx = setup();
    let creator = ctx.user();
    let id = ctx.invoice(&creator, 1_000);
    let r1 = ctx.user();
    let res = ctx.client.try_propose_recipients(
        &ctx.user(),
        &id,
        &vec![&ctx.env, r1],
        &vec![&ctx.env, 10_000u32],
        &1u32,
        &DEADLINE,
    );
    assert_eq!(res, Err(Ok(Error::Unauthorized)));
}

#[test]
fn test_propose_rejects_bad_shares() {
    let ctx = setup();
    let creator = ctx.user();
    let id = ctx.invoice(&creator, 1_000);
    let recipients = vec![&ctx.env, ctx.user(), ctx.user()];

    let under = ctx.client.try_propose_recipients(
        &creator,
        &id,
        &recipients,
        &vec![&ctx.env, 5_000u32, 4_000u32],
        &1u32,
        &DEADLINE,
    );
    assert_eq!(under, Err(Ok(Error::InvalidShares)));

    let zero = ctx.client.try_propose_recipients(
        &creator,
        &id,
        &recipients,
        &vec![&ctx.env, 10_000u32, 0u32],
        &1u32,
        &DEADLINE,
    );
    assert_eq!(zero, Err(Ok(Error::InvalidShares)));
}

#[test]
fn test_propose_rejects_bad_recipient_lists() {
    let ctx = setup();
    let creator = ctx.user();
    let id = ctx.invoice(&creator, 1_000);
    let r = ctx.user();

    let dup = ctx.client.try_propose_recipients(
        &creator,
        &id,
        &vec![&ctx.env, r.clone(), r.clone()],
        &vec![&ctx.env, 5_000u32, 5_000u32],
        &1u32,
        &DEADLINE,
    );
    assert_eq!(dup, Err(Ok(Error::InvalidRecipients)));

    let mismatch = ctx.client.try_propose_recipients(
        &creator,
        &id,
        &vec![&ctx.env, r.clone()],
        &vec![&ctx.env, 5_000u32, 5_000u32],
        &1u32,
        &DEADLINE,
    );
    assert_eq!(mismatch, Err(Ok(Error::InvalidRecipients)));

    let empty = ctx.client.try_propose_recipients(
        &creator,
        &id,
        &Vec::new(&ctx.env),
        &Vec::new(&ctx.env),
        &1u32,
        &DEADLINE,
    );
    assert_eq!(empty, Err(Ok(Error::InvalidRecipients)));
}

#[test]
fn test_propose_rejects_bad_threshold_and_deadline() {
    let ctx = setup();
    let creator = ctx.user();
    let id = ctx.invoice(&creator, 1_000);
    let recipients = vec![&ctx.env, ctx.user(), ctx.user()];
    let shares = vec![&ctx.env, 5_000u32, 5_000u32];

    for bad in [0u32, 3u32] {
        let res = ctx
            .client
            .try_propose_recipients(&creator, &id, &recipients, &shares, &bad, &DEADLINE);
        assert_eq!(res, Err(Ok(Error::InvalidThreshold)));
    }

    ctx.set_time(600);
    let late = ctx
        .client
        .try_propose_recipients(&creator, &id, &recipients, &shares, &1u32, &DEADLINE);
    assert_eq!(late, Err(Ok(Error::DeadlinePassed)));
}

// ---------------------------------------------------------------------------
// Release gating and payout
// ---------------------------------------------------------------------------

#[test]
fn test_full_funding_is_held_until_recipients_approve() {
    let ctx = setup();
    let (creator, id, r1, r2) = proposed(&ctx);
    let payer = ctx.funded_user(1_000);

    ctx.client.deposit(&payer, &id, &1_000);

    let inv = ctx.client.get_invoice(&id);
    assert_eq!(inv.status, EscrowStatus::Active);
    assert_eq!(inv.funded_amount, 1_000);
    assert_eq!(ctx.contract_balance(), 1_000);
    assert_eq!(
        ctx.client.try_release(&id),
        Err(Ok(Error::RecipientsNotApproved))
    );

    // First approval: still pending.
    let s1 = ctx.client.approve_recipients(&r1, &id, &1u32);
    assert_eq!(s1, ProposalStatus::Pending);
    assert!(ctx.has_event(symbol_short!("approval"), symbol_short!("vote_yes"), id));
    assert_eq!(ctx.contract_balance(), 1_000);

    // Second approval reaches the threshold and auto-releases.
    let s2 = ctx.client.approve_recipients(&r2, &id, &1u32);
    assert_eq!(s2, ProposalStatus::Approved);
    assert!(ctx.has_event(symbol_short!("approval"), symbol_short!("approved"), id));
    assert!(ctx.has_event(symbol_short!("approval"), symbol_short!("payout"), id));
    assert!(ctx.has_event(symbol_short!("escrow"), symbol_short!("released"), id));

    assert_eq!(ctx.client.get_invoice(&id).status, EscrowStatus::Released);
    assert_eq!(ctx.balance(&r1), 600);
    assert_eq!(ctx.balance(&r2), 400);
    assert_eq!(ctx.balance(&creator), 0);
    assert_eq!(ctx.contract_balance(), 0);
}

#[test]
fn test_approved_before_funding_splits_on_auto_release() {
    let ctx = setup();
    let (creator, id, r1, r2) = proposed(&ctx);
    ctx.client.approve_recipients(&r1, &id, &1u32);
    ctx.client.approve_recipients(&r2, &id, &1u32);
    assert!(ctx.client.is_release_approved(&id));

    let payer = ctx.funded_user(1_000);
    ctx.client.deposit(&payer, &id, &1_000);

    assert_eq!(ctx.client.get_invoice(&id).status, EscrowStatus::Released);
    assert_eq!(ctx.balance(&r1), 600);
    assert_eq!(ctx.balance(&r2), 400);
    assert_eq!(ctx.balance(&creator), 0);
}

#[test]
fn test_rounding_remainder_goes_to_last_recipient() {
    let ctx = setup();
    let creator = ctx.user();
    let id = ctx.invoice(&creator, 100);
    let (a, b, c) = (ctx.user(), ctx.user(), ctx.user());
    ctx.client.propose_recipients(
        &creator,
        &id,
        &vec![&ctx.env, a.clone(), b.clone(), c.clone()],
        &vec![&ctx.env, 3_333u32, 3_333u32, 3_334u32],
        &1u32,
        &DEADLINE,
    );
    ctx.client.approve_recipients(&a, &id, &1u32);

    let payer = ctx.funded_user(100);
    ctx.client.deposit(&payer, &id, &100);

    assert_eq!(ctx.balance(&a), 33);
    assert_eq!(ctx.balance(&b), 33);
    assert_eq!(ctx.balance(&c), 34);
    assert_eq!(ctx.contract_balance(), 0);
}

#[test]
fn test_n_of_m_threshold() {
    let ctx = setup();
    let creator = ctx.user();
    let id = ctx.invoice(&creator, 900);
    let (a, b, c) = (ctx.user(), ctx.user(), ctx.user());
    ctx.client.propose_recipients(
        &creator,
        &id,
        &vec![&ctx.env, a.clone(), b.clone(), c.clone()],
        &vec![&ctx.env, 3_000u32, 3_000u32, 4_000u32],
        &2u32,
        &DEADLINE,
    );
    assert_eq!(
        ctx.client.approve_recipients(&a, &id, &1u32),
        ProposalStatus::Pending
    );
    assert_eq!(
        ctx.client.approve_recipients(&c, &id, &1u32),
        ProposalStatus::Approved
    );
    // `b` never voted but still receives its share.
    let payer = ctx.funded_user(900);
    ctx.client.deposit(&payer, &id, &900);
    assert_eq!(ctx.balance(&b), 270);
}

// ---------------------------------------------------------------------------
// Voting rules
// ---------------------------------------------------------------------------

#[test]
fn test_non_recipient_and_double_vote_rejected() {
    let ctx = setup();
    let (_, id, r1, _) = proposed(&ctx);

    assert_eq!(
        ctx.client.try_approve_recipients(&ctx.user(), &id, &1u32),
        Err(Ok(Error::NotARecipient))
    );

    ctx.client.approve_recipients(&r1, &id, &1u32);
    assert_eq!(
        ctx.client.try_approve_recipients(&r1, &id, &1u32),
        Err(Ok(Error::AlreadyVoted))
    );
    assert_eq!(
        ctx.client
            .try_reject_recipients(&r1, &id, &1u32, &reason(&ctx)),
        Err(Ok(Error::AlreadyVoted))
    );
}

#[test]
fn test_vote_without_proposal_fails() {
    let ctx = setup();
    let creator = ctx.user();
    let id = ctx.invoice(&creator, 1_000);
    assert_eq!(
        ctx.client.try_approve_recipients(&ctx.user(), &id, &1u32),
        Err(Ok(Error::ProposalNotFound))
    );
}

#[test]
fn test_revision_bumps_version_and_clears_votes() {
    let ctx = setup();
    let (creator, id, r1, r2) = proposed(&ctx);
    ctx.client.approve_recipients(&r1, &id, &1u32);

    let v2 = ctx.client.propose_recipients(
        &creator,
        &id,
        &vec![&ctx.env, r1.clone(), r2.clone()],
        &vec![&ctx.env, 5_000u32, 5_000u32],
        &2u32,
        &DEADLINE,
    );
    assert_eq!(v2, 2);
    let p = ctx.client.get_recipient_proposal(&id).unwrap();
    assert!(p.approvals.is_empty());
    assert_eq!(p.status, ProposalStatus::Pending);

    // Votes on the superseded version are refused.
    assert_eq!(
        ctx.client.try_approve_recipients(&r2, &id, &1u32),
        Err(Ok(Error::ProposalVersionMismatch))
    );
    ctx.client.approve_recipients(&r1, &id, &2u32);
    assert_eq!(
        ctx.client.approve_recipients(&r2, &id, &2u32),
        ProposalStatus::Approved
    );
}

#[test]
fn test_rejection_that_breaks_threshold_rejects_proposal() {
    let ctx = setup();
    let (_, id, r1, r2) = proposed(&ctx);

    let status = ctx
        .client
        .reject_recipients(&r1, &id, &1u32, &reason(&ctx));
    assert_eq!(status, ProposalStatus::Rejected);
    assert!(ctx.has_event(symbol_short!("approval"), symbol_short!("vote_no"), id));
    assert!(ctx.has_event(symbol_short!("approval"), symbol_short!("rejected"), id));

    assert_eq!(
        ctx.client.try_approve_recipients(&r2, &id, &1u32),
        Err(Ok(Error::ProposalClosed))
    );
    assert!(!ctx.client.is_release_approved(&id));
}

#[test]
fn test_rejection_below_threshold_keeps_proposal_pending() {
    let ctx = setup();
    let creator = ctx.user();
    let id = ctx.invoice(&creator, 1_000);
    let (a, b, c) = (ctx.user(), ctx.user(), ctx.user());
    ctx.client.propose_recipients(
        &creator,
        &id,
        &vec![&ctx.env, a.clone(), b.clone(), c.clone()],
        &vec![&ctx.env, 3_000u32, 3_000u32, 4_000u32],
        &2u32,
        &DEADLINE,
    );
    let status = ctx.client.reject_recipients(&a, &id, &1u32, &reason(&ctx));
    assert_eq!(status, ProposalStatus::Pending);
}

#[test]
fn test_revoke_approval() {
    let ctx = setup();
    let (_, id, r1, r2) = proposed(&ctx);
    ctx.client.approve_recipients(&r1, &id, &1u32);
    ctx.client.revoke_recipient_approval(&r1, &id);
    assert!(ctx.has_event(symbol_short!("approval"), symbol_short!("revoked"), id));
    assert!(ctx
        .client
        .get_recipient_proposal(&id)
        .unwrap()
        .approvals
        .is_empty());

    // Revoking without an approval on record fails.
    assert_eq!(
        ctx.client.try_revoke_recipient_approval(&r2, &id),
        Err(Ok(Error::NotARecipient))
    );
}

#[test]
fn test_vote_after_deadline_fails() {
    let ctx = setup();
    let (_, id, r1, _) = proposed(&ctx);
    ctx.set_time(DEADLINE + 1);
    assert_eq!(
        ctx.client.try_approve_recipients(&r1, &id, &1u32),
        Err(Ok(Error::DeadlinePassed))
    );
}

#[test]
fn test_withdraw_proposal_restores_creator_payout() {
    let ctx = setup();
    let (creator, id, _, _) = proposed(&ctx);
    let payer = ctx.funded_user(1_000);
    ctx.client.deposit(&payer, &id, &1_000);
    assert_eq!(ctx.balance(&creator), 0);

    ctx.client.withdraw_recipient_proposal(&creator, &id);
    assert!(ctx.has_event(symbol_short!("approval"), symbol_short!("withdrawn"), id));
    assert!(ctx.client.get_recipient_proposal(&id).is_none());

    ctx.client.release(&id);
    assert_eq!(ctx.balance(&creator), 1_000);
}

#[test]
fn test_cannot_propose_after_release() {
    let ctx = setup();
    let creator = ctx.user();
    let id = ctx.invoice(&creator, 1_000);
    let payer = ctx.funded_user(1_000);
    ctx.client.deposit(&payer, &id, &1_000);

    let res = ctx.client.try_propose_recipients(
        &creator,
        &id,
        &vec![&ctx.env, ctx.user()],
        &vec![&ctx.env, 10_000u32],
        &1u32,
        &DEADLINE,
    );
    assert_eq!(res, Err(Ok(Error::InvalidStatus)));
}
