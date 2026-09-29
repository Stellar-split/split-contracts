//! Tests for the invoice lending marketplace (#856).

use crate::errors::Error;
use crate::test_helpers::{setup, Ctx};
use crate::types::{EscrowStatus, LoanStatus};
use soroban_sdk::{symbol_short, vec, Address};

const EXPIRY: u64 = 500;

/// Creator opens a 1 000 invoice and lists it: borrow 800, repay 900.
fn listed(ctx: &Ctx) -> (Address, u64) {
    let creator = ctx.user();
    let id = ctx.invoice(&creator, 1_000);
    ctx.client
        .list_invoice_for_loan(&creator, &id, &800, &900, &EXPIRY);
    (creator, id)
}

/// `listed` plus a lender who funds the loan.
fn funded(ctx: &Ctx) -> (Address, Address, u64) {
    let (creator, id) = listed(ctx);
    let lender = ctx.funded_user(800);
    ctx.client.fund_invoice_loan(&lender, &id);
    (creator, lender, id)
}

// ---------------------------------------------------------------------------
// Listing
// ---------------------------------------------------------------------------

#[test]
fn test_list_creates_open_listing() {
    let ctx = setup();
    let (creator, id) = listed(&ctx);
    assert!(ctx.has_event(symbol_short!("lending"), symbol_short!("listed"), id));

    let loan = ctx.client.get_loan_listing(&id).unwrap();
    assert_eq!(loan.borrower, creator);
    assert_eq!(loan.principal, 800);
    assert_eq!(loan.repayment, 900);
    assert_eq!(loan.status, LoanStatus::Open);
    assert_eq!(loan.lender, None);
    assert_eq!(ctx.client.get_open_loan_listings(), vec![&ctx.env, id]);
}

#[test]
fn test_list_requires_creator() {
    let ctx = setup();
    let creator = ctx.user();
    let id = ctx.invoice(&creator, 1_000);
    assert_eq!(
        ctx.client
            .try_list_invoice_for_loan(&ctx.user(), &id, &800, &900, &EXPIRY),
        Err(Ok(Error::Unauthorized))
    );
}

#[test]
fn test_list_rejects_bad_terms() {
    let ctx = setup();
    let creator = ctx.user();
    let id = ctx.invoice(&creator, 1_000);
    let cases: [(i128, i128, u64); 4] = [
        (0, 900, EXPIRY),     // zero principal
        (800, 700, EXPIRY),   // repayment below principal
        (800, 1_001, EXPIRY), // repayment above invoice total
        (800, 900, 0),        // expiry not in the future
    ];
    for (principal, repayment, expires_at) in cases {
        assert_eq!(
            ctx.client.try_list_invoice_for_loan(
                &creator,
                &id,
                &principal,
                &repayment,
                &expires_at
            ),
            Err(Ok(Error::InvalidLoanTerms))
        );
    }
}

#[test]
fn test_cannot_double_list() {
    let ctx = setup();
    let (creator, id) = listed(&ctx);
    assert_eq!(
        ctx.client
            .try_list_invoice_for_loan(&creator, &id, &500, &600, &EXPIRY),
        Err(Ok(Error::ActiveLoanExists))
    );
}

#[test]
fn test_loans_and_recipient_proposals_are_exclusive() {
    let ctx = setup();
    let (creator, id) = listed(&ctx);
    assert_eq!(
        ctx.client.try_propose_recipients(
            &creator,
            &id,
            &vec![&ctx.env, ctx.user()],
            &vec![&ctx.env, 10_000u32],
            &1u32,
            &EXPIRY,
        ),
        Err(Ok(Error::ActiveLoanExists))
    );

    let other = ctx.invoice(&creator, 1_000);
    ctx.client.propose_recipients(
        &creator,
        &other,
        &vec![&ctx.env, ctx.user()],
        &vec![&ctx.env, 10_000u32],
        &1u32,
        &EXPIRY,
    );
    assert_eq!(
        ctx.client
            .try_list_invoice_for_loan(&creator, &other, &800, &900, &EXPIRY),
        Err(Ok(Error::RecipientProposalExists))
    );
}

#[test]
fn test_cancel_listing() {
    let ctx = setup();
    let (creator, id) = listed(&ctx);

    assert_eq!(
        ctx.client.try_cancel_loan_listing(&ctx.user(), &id),
        Err(Ok(Error::Unauthorized))
    );
    ctx.client.cancel_loan_listing(&creator, &id);
    assert!(ctx.has_event(symbol_short!("lending"), symbol_short!("cancelled"), id));
    assert_eq!(
        ctx.client.get_loan_listing(&id).unwrap().status,
        LoanStatus::Cancelled
    );
    assert!(ctx.client.get_open_loan_listings().is_empty());

    let lender = ctx.funded_user(800);
    assert_eq!(
        ctx.client.try_fund_invoice_loan(&lender, &id),
        Err(Ok(Error::LoanNotOpen))
    );

    // The invoice can be relisted with new terms.
    ctx.client
        .list_invoice_for_loan(&creator, &id, &500, &550, &EXPIRY);
    assert_eq!(ctx.client.get_loan_listing(&id).unwrap().principal, 500);
}

#[test]
fn test_open_listings_hide_expired() {
    let ctx = setup();
    let (_, id) = listed(&ctx);
    assert_eq!(ctx.client.get_open_loan_listings().len(), 1);
    ctx.set_time(EXPIRY + 1);
    assert!(ctx.client.get_open_loan_listings().is_empty());

    let lender = ctx.funded_user(800);
    assert_eq!(
        ctx.client.try_fund_invoice_loan(&lender, &id),
        Err(Ok(Error::DeadlinePassed))
    );
}

// ---------------------------------------------------------------------------
// Funding and repayment
// ---------------------------------------------------------------------------

#[test]
fn test_fund_advances_principal_to_borrower() {
    let ctx = setup();
    let (creator, lender, id) = funded(&ctx);
    assert!(ctx.has_event(symbol_short!("lending"), symbol_short!("funded"), id));

    assert_eq!(ctx.balance(&creator), 800);
    assert_eq!(ctx.balance(&lender), 0);
    let loan = ctx.client.get_loan_listing(&id).unwrap();
    assert_eq!(loan.status, LoanStatus::Funded);
    assert_eq!(loan.lender, Some(lender.clone()));
    assert!(ctx.client.get_open_loan_listings().is_empty());

    let second = ctx.funded_user(800);
    assert_eq!(
        ctx.client.try_fund_invoice_loan(&second, &id),
        Err(Ok(Error::LoanNotOpen))
    );
}

#[test]
fn test_borrower_cannot_self_fund() {
    let ctx = setup();
    let (creator, id) = listed(&ctx);
    ctx.mint(&creator, 800);
    assert_eq!(
        ctx.client.try_fund_invoice_loan(&creator, &id),
        Err(Ok(Error::SelfDealing))
    );
}

#[test]
fn test_release_repays_lender_first() {
    let ctx = setup();
    let (creator, lender, id) = funded(&ctx);
    let payer = ctx.funded_user(1_000);
    ctx.client.deposit(&payer, &id, &1_000);

    assert_eq!(ctx.client.get_invoice(&id).status, EscrowStatus::Released);
    assert!(ctx.has_event(symbol_short!("lending"), symbol_short!("repaid"), id));
    assert_eq!(ctx.balance(&lender), 900);
    // 800 principal up front + 100 remainder at release.
    assert_eq!(ctx.balance(&creator), 900);
    assert_eq!(ctx.contract_balance(), 0);
    assert_eq!(
        ctx.client.get_loan_listing(&id).unwrap().status,
        LoanStatus::Repaid
    );
}

#[test]
fn test_early_repayment() {
    let ctx = setup();
    let (creator, lender, id) = funded(&ctx);
    ctx.mint(&creator, 100);

    assert_eq!(
        ctx.client.try_repay_invoice_loan(&ctx.user(), &id),
        Err(Ok(Error::Unauthorized))
    );
    ctx.client.repay_invoice_loan(&creator, &id);
    assert!(ctx.has_event(symbol_short!("lending"), symbol_short!("repaid"), id));
    assert_eq!(ctx.balance(&lender), 900);
    assert_eq!(ctx.balance(&creator), 0);

    // Release now pays the creator in full.
    let payer = ctx.funded_user(1_000);
    ctx.client.deposit(&payer, &id, &1_000);
    assert_eq!(ctx.balance(&creator), 1_000);
    assert_eq!(ctx.balance(&lender), 900);

    assert_eq!(
        ctx.client.try_repay_invoice_loan(&creator, &id),
        Err(Ok(Error::LoanNotFunded))
    );
}

#[test]
fn test_transfer_position_redirects_repayment() {
    let ctx = setup();
    let (creator, lender, id) = funded(&ctx);
    let buyer = ctx.user();

    assert_eq!(
        ctx.client.try_transfer_loan_position(&buyer, &id, &buyer),
        Err(Ok(Error::Unauthorized))
    );
    assert_eq!(
        ctx.client.try_transfer_loan_position(&lender, &id, &creator),
        Err(Ok(Error::SelfDealing))
    );

    ctx.client.transfer_loan_position(&lender, &id, &buyer);
    assert!(ctx.has_event(symbol_short!("lending"), symbol_short!("transfer"), id));
    assert_eq!(ctx.client.get_loan_listing(&id).unwrap().lender, Some(buyer.clone()));

    let payer = ctx.funded_user(1_000);
    ctx.client.deposit(&payer, &id, &1_000);
    assert_eq!(ctx.balance(&buyer), 900);
    assert_eq!(ctx.balance(&lender), 0);
}

#[test]
fn test_release_closes_unfunded_listing() {
    let ctx = setup();
    let (creator, id) = listed(&ctx);
    let payer = ctx.funded_user(1_000);
    ctx.client.deposit(&payer, &id, &1_000);

    assert_eq!(ctx.balance(&creator), 1_000);
    assert_eq!(
        ctx.client.get_loan_listing(&id).unwrap().status,
        LoanStatus::Cancelled
    );
    assert!(ctx.client.get_open_loan_listings().is_empty());
}

// ---------------------------------------------------------------------------
// Default
// ---------------------------------------------------------------------------

#[test]
fn test_mark_defaulted_after_refund() {
    let ctx = setup();
    let (_, lender, id) = funded(&ctx);
    let payer = ctx.funded_user(1_000);
    ctx.client.deposit(&payer, &id, &300);

    assert_eq!(
        ctx.client.try_mark_loan_defaulted(&id),
        Err(Ok(Error::InvalidStatus))
    );

    ctx.set_time(1_001);
    ctx.client.refund(&id, &vec![&ctx.env, payer.clone()]);
    ctx.client.mark_loan_defaulted(&id);
    assert!(ctx.has_event(symbol_short!("lending"), symbol_short!("defaulted"), id));

    let loan = ctx.client.get_loan_listing(&id).unwrap();
    assert_eq!(loan.status, LoanStatus::Defaulted);
    assert_eq!(loan.lender, Some(lender));
    assert_eq!(ctx.balance(&payer), 1_000);

    assert_eq!(
        ctx.client.try_mark_loan_defaulted(&id),
        Err(Ok(Error::LoanNotFunded))
    );
}

#[test]
fn test_unknown_loan() {
    let ctx = setup();
    let creator = ctx.user();
    let id = ctx.invoice(&creator, 1_000);
    assert!(ctx.client.get_loan_listing(&id).is_none());
    assert_eq!(
        ctx.client.try_fund_invoice_loan(&ctx.user(), &id),
        Err(Ok(Error::LoanNotFound))
    );
}
