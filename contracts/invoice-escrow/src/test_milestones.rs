//! Tests for the milestone-triggered payment escrow (#854).

use crate::errors::Error;
use crate::test_helpers::{setup, Ctx};
use crate::types::{MilestoneEscrowStatus, MilestoneInput, MilestoneTrigger};
use soroban_sdk::{symbol_short, vec, Address, Vec};

fn m(amount: i128, trigger: MilestoneTrigger) -> MilestoneInput {
    MilestoneInput { amount, trigger }
}

/// Payer-funded escrow of 600 with three milestones:
/// 0 — 100 on payer approval, 1 — 200 at t=100, 2 — 300 on arbiter sign-off.
fn three_stage(ctx: &Ctx) -> (Address, Address, Address, u64) {
    let payer = ctx.funded_user(1_000);
    let payee = ctx.user();
    let arbiter = ctx.user();
    let id = ctx.client.create_milestone_escrow(
        &payer,
        &payee,
        &ctx.token,
        &vec![
            &ctx.env,
            m(100, MilestoneTrigger::PayerApproval),
            m(200, MilestoneTrigger::Timestamp(100)),
            m(300, MilestoneTrigger::Arbiter(arbiter.clone())),
        ],
    );
    (payer, payee, arbiter, id)
}

#[test]
fn test_create_prefunds_escrow() {
    let ctx = setup();
    let (payer, payee, _, id) = three_stage(&ctx);
    assert_eq!(id, 0);
    assert!(ctx.has_event(symbol_short!("milestone"), symbol_short!("created"), id));

    let e = ctx.client.get_milestone_escrow(&id);
    assert_eq!(e.payer, payer);
    assert_eq!(e.payee, payee);
    assert_eq!(e.total_amount, 600);
    assert_eq!(e.released_amount, 0);
    assert_eq!(e.milestones.len(), 3);
    assert_eq!(e.status, MilestoneEscrowStatus::Active);
    assert_eq!(ctx.balance(&payer), 400);
    assert_eq!(ctx.contract_balance(), 600);
}

#[test]
fn test_ids_increment() {
    let ctx = setup();
    let (_, _, _, first) = three_stage(&ctx);
    let (_, _, _, second) = three_stage(&ctx);
    assert_eq!(second, first + 1);
}

#[test]
fn test_create_validation() {
    let ctx = setup();
    let payer = ctx.funded_user(1_000);
    let payee = ctx.user();

    let empty: Vec<MilestoneInput> = Vec::new(&ctx.env);
    assert_eq!(
        ctx.client
            .try_create_milestone_escrow(&payer, &payee, &ctx.token, &empty),
        Err(Ok(Error::InvalidMilestones))
    );

    let zero = vec![&ctx.env, m(0, MilestoneTrigger::PayerApproval)];
    assert_eq!(
        ctx.client
            .try_create_milestone_escrow(&payer, &payee, &ctx.token, &zero),
        Err(Ok(Error::InvalidMilestones))
    );

    let mut too_many: Vec<MilestoneInput> = Vec::new(&ctx.env);
    for _ in 0..21 {
        too_many.push_back(m(1, MilestoneTrigger::PayerApproval));
    }
    assert_eq!(
        ctx.client
            .try_create_milestone_escrow(&payer, &payee, &ctx.token, &too_many),
        Err(Ok(Error::InvalidMilestones))
    );

    let ok = vec![&ctx.env, m(1, MilestoneTrigger::PayerApproval)];
    assert_eq!(
        ctx.client
            .try_create_milestone_escrow(&payer, &payer, &ctx.token, &ok),
        Err(Ok(Error::SelfDealing))
    );
}

#[test]
fn test_payer_approval_trigger() {
    let ctx = setup();
    let (payer, payee, _, id) = three_stage(&ctx);

    // Payee cannot self-approve a payer-approval milestone.
    assert_eq!(
        ctx.client.try_trigger_milestone(&payee, &id, &0u32),
        Err(Ok(Error::TriggerNotSatisfied))
    );

    let paid = ctx.client.trigger_milestone(&payer, &id, &0u32);
    assert_eq!(paid, 100);
    assert!(ctx.has_event(symbol_short!("milestone"), symbol_short!("released"), id));
    assert_eq!(ctx.balance(&payee), 100);
    assert_eq!(ctx.client.get_milestone_escrow(&id).released_amount, 100);

    assert_eq!(
        ctx.client.try_trigger_milestone(&payer, &id, &0u32),
        Err(Ok(Error::MilestoneAlreadyReleased))
    );
}

#[test]
fn test_timestamp_trigger() {
    let ctx = setup();
    let (_, payee, _, id) = three_stage(&ctx);
    let anyone = ctx.user();

    ctx.set_time(99);
    assert_eq!(
        ctx.client.try_trigger_milestone(&anyone, &id, &1u32),
        Err(Ok(Error::TriggerNotSatisfied))
    );

    ctx.set_time(100);
    assert_eq!(ctx.client.trigger_milestone(&anyone, &id, &1u32), 200);
    assert_eq!(ctx.balance(&payee), 200);
}

#[test]
fn test_arbiter_trigger() {
    let ctx = setup();
    let (payer, payee, arbiter, id) = three_stage(&ctx);

    assert_eq!(
        ctx.client.try_trigger_milestone(&payer, &id, &2u32),
        Err(Ok(Error::TriggerNotSatisfied))
    );
    assert_eq!(ctx.client.trigger_milestone(&arbiter, &id, &2u32), 300);
    assert_eq!(ctx.balance(&payee), 300);
}

#[test]
fn test_out_of_range_index() {
    let ctx = setup();
    let (payer, _, _, id) = three_stage(&ctx);
    assert_eq!(
        ctx.client.try_trigger_milestone(&payer, &id, &3u32),
        Err(Ok(Error::MilestoneIndexOutOfRange))
    );
    assert_eq!(
        ctx.client.try_trigger_milestone(&payer, &99u64, &0u32),
        Err(Ok(Error::MilestoneEscrowNotFound))
    );
}

#[test]
fn test_all_milestones_complete_escrow() {
    let ctx = setup();
    let (payer, payee, arbiter, id) = three_stage(&ctx);
    ctx.set_time(100);

    ctx.client.trigger_milestone(&payer, &id, &0u32);
    ctx.client.trigger_milestone(&arbiter, &id, &2u32);
    ctx.client.trigger_milestone(&payee, &id, &1u32);
    assert!(ctx.has_event(symbol_short!("milestone"), symbol_short!("completed"), id));

    let e = ctx.client.get_milestone_escrow(&id);
    assert_eq!(e.status, MilestoneEscrowStatus::Completed);
    assert_eq!(e.released_amount, 600);
    assert_eq!(ctx.balance(&payee), 600);
    assert_eq!(ctx.contract_balance(), 0);

    assert_eq!(
        ctx.client.try_trigger_milestone(&payer, &id, &0u32),
        Err(Ok(Error::EscrowClosed))
    );
}

#[test]
fn test_trigger_due_releases_only_due_timed_milestones() {
    let ctx = setup();
    let payer = ctx.funded_user(1_000);
    let payee = ctx.user();
    let id = ctx.client.create_milestone_escrow(
        &payer,
        &payee,
        &ctx.token,
        &vec![
            &ctx.env,
            m(100, MilestoneTrigger::Timestamp(10)),
            m(100, MilestoneTrigger::PayerApproval),
            m(100, MilestoneTrigger::Timestamp(20)),
            m(100, MilestoneTrigger::Timestamp(30)),
        ],
    );

    ctx.set_time(25);
    assert_eq!(ctx.client.trigger_due_milestones(&id), 2);
    assert_eq!(ctx.balance(&payee), 200);

    // Nothing new is due until t=30.
    assert_eq!(ctx.client.trigger_due_milestones(&id), 0);

    ctx.set_time(30);
    assert_eq!(ctx.client.trigger_due_milestones(&id), 1);
    assert_eq!(ctx.balance(&payee), 300);
    assert_eq!(
        ctx.client.get_milestone_escrow(&id).status,
        MilestoneEscrowStatus::Active
    );
}

#[test]
fn test_mutual_cancel_refunds_remainder() {
    let ctx = setup();
    let (payer, payee, _, id) = three_stage(&ctx);
    ctx.client.trigger_milestone(&payer, &id, &0u32);

    let refunded = ctx.client.cancel_milestone_escrow(&id);
    assert_eq!(refunded, 500);
    assert!(ctx.has_event(symbol_short!("milestone"), symbol_short!("cancelled"), id));
    assert_eq!(ctx.balance(&payer), 900);
    assert_eq!(ctx.balance(&payee), 100);
    assert_eq!(
        ctx.client.get_milestone_escrow(&id).status,
        MilestoneEscrowStatus::Cancelled
    );
    assert_eq!(
        ctx.client.try_cancel_milestone_escrow(&id),
        Err(Ok(Error::EscrowClosed))
    );
}

#[test]
fn test_admin_resolves_dispute_for_payee() {
    let ctx = setup();
    let (payer, payee, _, id) = three_stage(&ctx);
    ctx.client.trigger_milestone(&payer, &id, &0u32);

    assert_eq!(ctx.client.resolve_milestone_dispute(&id, &true), 500);
    assert!(ctx.has_event(symbol_short!("milestone"), symbol_short!("resolved"), id));
    assert_eq!(ctx.balance(&payee), 600);
    let e = ctx.client.get_milestone_escrow(&id);
    assert_eq!(e.status, MilestoneEscrowStatus::Completed);
    assert_eq!(e.released_amount, 600);
}

#[test]
fn test_admin_resolves_dispute_for_payer() {
    let ctx = setup();
    let (payer, _, _, id) = three_stage(&ctx);
    assert_eq!(ctx.client.resolve_milestone_dispute(&id, &false), 600);
    assert_eq!(ctx.balance(&payer), 1_000);
    assert_eq!(
        ctx.client.get_milestone_escrow(&id).status,
        MilestoneEscrowStatus::Cancelled
    );
    assert_eq!(ctx.client.get_admin(), ctx.admin);
}
