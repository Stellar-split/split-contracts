//! Invoice lending marketplace (issue #856).
//!
//! An invoice creator can borrow against an escrow invoice before it is paid:
//!
//! 1. `list_invoice_for_loan` — the creator (borrower) lists the invoice with a
//!    principal, a repayment amount (principal + interest, capped at the
//!    invoice total) and a listing expiry. Open listings are indexed so
//!    lenders can discover them via `get_open_loan_listings`.
//! 2. `fund_invoice_loan` — a lender advances the principal straight to the
//!    borrower.
//! 3. When the invoice is released, the lender is repaid first out of the
//!    escrowed funds and only the remainder goes to the creator. The borrower
//!    may also repay early with `repay_invoice_loan`.
//! 4. Lenders can sell a funded position with `transfer_loan_position`.
//! 5. If the invoice is refunded or cancelled while the loan is outstanding,
//!    anyone can call `mark_loan_defaulted` to record the default on-chain.
//!
//! An invoice with a recipient proposal cannot be listed (and vice versa), so
//! the release payout is never split between a lender and recipients.

use soroban_sdk::{symbol_short, token, Address, Env, Symbol, Vec};

use crate::errors::Error;
use crate::get_invoice;
use crate::types::{EscrowStatus, LoanListing, LoanStatus};

/// Maximum number of simultaneously open listings in the marketplace index.
pub(crate) const MAX_OPEN_LISTINGS: u32 = 100;

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Persistent storage: loan listing keyed by invoice ID.
fn loan_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("loan"), invoice_id)
}

/// Instance storage: invoice IDs with an `Open` listing.
fn index_key() -> Symbol {
    symbol_short!("loan_idx")
}

pub(crate) fn get_loan(env: &Env, invoice_id: u64) -> Option<LoanListing> {
    env.storage().persistent().get(&loan_key(invoice_id))
}

fn save_loan(env: &Env, loan: &LoanListing) {
    env.storage().persistent().set(&loan_key(loan.invoice_id), loan);
}

fn get_index(env: &Env) -> Vec<u64> {
    env.storage()
        .instance()
        .get(&index_key())
        .unwrap_or_else(|| Vec::new(env))
}

fn remove_from_index(env: &Env, invoice_id: u64) {
    let mut index = get_index(env);
    if let Some(i) = index.first_index_of(invoice_id) {
        index.remove(i);
        env.storage().instance().set(&index_key(), &index);
    }
}

/// `true` when the invoice has an `Open` or `Funded` loan.
pub(crate) fn has_active_loan(env: &Env, invoice_id: u64) -> bool {
    matches!(
        get_loan(env, invoice_id).map(|l| l.status),
        Some(LoanStatus::Open) | Some(LoanStatus::Funded)
    )
}

fn is_open_invoice(status: &EscrowStatus) -> bool {
    *status == EscrowStatus::Pending || *status == EscrowStatus::Active
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Topics: `(lending, listed, id)` — Data: `(borrower, principal, repayment, expires_at)`
fn emit_listed(env: &Env, l: &LoanListing) {
    env.events().publish(
        (symbol_short!("lending"), symbol_short!("listed"), l.invoice_id),
        (l.borrower.clone(), l.principal, l.repayment, l.expires_at),
    );
}

/// Topics: `(lending, cancelled, id)` — Data: `borrower`
fn emit_cancelled(env: &Env, l: &LoanListing) {
    env.events().publish(
        (symbol_short!("lending"), symbol_short!("cancelled"), l.invoice_id),
        l.borrower.clone(),
    );
}

/// Topics: `(lending, funded, id)` — Data: `(lender, borrower, principal)`
fn emit_funded(env: &Env, l: &LoanListing, lender: &Address) {
    env.events().publish(
        (symbol_short!("lending"), symbol_short!("funded"), l.invoice_id),
        (lender.clone(), l.borrower.clone(), l.principal),
    );
}

/// Topics: `(lending, repaid, id)` — Data: `(lender, amount)`
fn emit_repaid(env: &Env, invoice_id: u64, lender: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("lending"), symbol_short!("repaid"), invoice_id),
        (lender.clone(), amount),
    );
}

/// Topics: `(lending, transfer, id)` — Data: `(from_lender, to_lender)`
fn emit_transfer(env: &Env, invoice_id: u64, from: &Address, to: &Address) {
    env.events().publish(
        (symbol_short!("lending"), symbol_short!("transfer"), invoice_id),
        (from.clone(), to.clone()),
    );
}

/// Topics: `(lending, defaulted, id)` — Data: `(lender, repayment_owed)`
fn emit_defaulted(env: &Env, invoice_id: u64, lender: &Address, owed: i128) {
    env.events().publish(
        (symbol_short!("lending"), symbol_short!("defaulted"), invoice_id),
        (lender.clone(), owed),
    );
}

// ---------------------------------------------------------------------------
// Release hook
// ---------------------------------------------------------------------------

/// Called when an invoice's escrow is released. Repays a funded lender out of
/// `total` (returning what is left for the creator) and closes a listing that
/// was never funded.
pub(crate) fn settle_on_release(env: &Env, invoice_id: u64, token: &Address, total: i128) -> i128 {
    let mut loan = match get_loan(env, invoice_id) {
        Some(l) => l,
        None => return total,
    };
    match loan.status {
        LoanStatus::Funded => {
            let lender = loan.lender.clone().expect("funded loan has a lender");
            let payout = loan.repayment.min(total);
            loan.status = LoanStatus::Repaid;
            save_loan(env, &loan);
            token::Client::new(env, token).transfer(
                &env.current_contract_address(),
                &lender,
                &payout,
            );
            emit_repaid(env, invoice_id, &lender, payout);
            total - payout
        }
        LoanStatus::Open => {
            loan.status = LoanStatus::Cancelled;
            save_loan(env, &loan);
            remove_from_index(env, invoice_id);
            emit_cancelled(env, &loan);
            total
        }
        _ => total,
    }
}

// ---------------------------------------------------------------------------
// Entry points (wrapped by the contract impl in lib.rs)
// ---------------------------------------------------------------------------

pub(crate) fn list(
    env: &Env,
    borrower: Address,
    invoice_id: u64,
    principal: i128,
    repayment: i128,
    expires_at: u64,
) -> Result<(), Error> {
    let invoice = get_invoice(env, invoice_id)?;
    if invoice.creator != borrower {
        return Err(Error::Unauthorized);
    }
    borrower.require_auth();
    if !is_open_invoice(&invoice.status) {
        return Err(Error::InvalidStatus);
    }
    if has_active_loan(env, invoice_id) {
        return Err(Error::ActiveLoanExists);
    }
    if crate::approval::get_proposal(env, invoice_id).is_some() {
        return Err(Error::RecipientProposalExists);
    }
    let now = env.ledger().timestamp();
    if principal <= 0
        || repayment < principal
        || repayment > invoice.total_amount
        || expires_at <= now
    {
        return Err(Error::InvalidLoanTerms);
    }

    // Drop listings that can no longer be funded before checking capacity.
    let mut index = open_listings(env);
    if index.len() >= MAX_OPEN_LISTINGS {
        return Err(Error::CapacityReached);
    }

    let loan = LoanListing {
        invoice_id,
        borrower,
        token: invoice.token,
        principal,
        repayment,
        expires_at,
        lender: None,
        status: LoanStatus::Open,
        funded_at: 0,
    };
    save_loan(env, &loan);
    index.push_back(invoice_id);
    env.storage().instance().set(&index_key(), &index);
    emit_listed(env, &loan);
    Ok(())
}

pub(crate) fn cancel(env: &Env, borrower: Address, invoice_id: u64) -> Result<(), Error> {
    let mut loan = get_loan(env, invoice_id).ok_or(Error::LoanNotFound)?;
    if loan.borrower != borrower {
        return Err(Error::Unauthorized);
    }
    borrower.require_auth();
    if loan.status != LoanStatus::Open {
        return Err(Error::LoanNotOpen);
    }
    loan.status = LoanStatus::Cancelled;
    save_loan(env, &loan);
    remove_from_index(env, invoice_id);
    emit_cancelled(env, &loan);
    Ok(())
}

pub(crate) fn fund(env: &Env, lender: Address, invoice_id: u64) -> Result<(), Error> {
    lender.require_auth();
    let mut loan = get_loan(env, invoice_id).ok_or(Error::LoanNotFound)?;
    if loan.status != LoanStatus::Open {
        return Err(Error::LoanNotOpen);
    }
    let now = env.ledger().timestamp();
    if now > loan.expires_at {
        return Err(Error::DeadlinePassed);
    }
    if lender == loan.borrower {
        return Err(Error::SelfDealing);
    }
    let invoice = get_invoice(env, invoice_id)?;
    if !is_open_invoice(&invoice.status) {
        return Err(Error::InvalidStatus);
    }

    loan.lender = Some(lender.clone());
    loan.status = LoanStatus::Funded;
    loan.funded_at = now;
    save_loan(env, &loan);
    remove_from_index(env, invoice_id);

    token::Client::new(env, &loan.token).transfer(&lender, &loan.borrower, &loan.principal);
    emit_funded(env, &loan, &lender);
    Ok(())
}

pub(crate) fn repay(env: &Env, borrower: Address, invoice_id: u64) -> Result<(), Error> {
    let mut loan = get_loan(env, invoice_id).ok_or(Error::LoanNotFound)?;
    if loan.borrower != borrower {
        return Err(Error::Unauthorized);
    }
    borrower.require_auth();
    if loan.status != LoanStatus::Funded {
        return Err(Error::LoanNotFunded);
    }
    let lender = loan.lender.clone().expect("funded loan has a lender");
    loan.status = LoanStatus::Repaid;
    save_loan(env, &loan);

    token::Client::new(env, &loan.token).transfer(&borrower, &lender, &loan.repayment);
    emit_repaid(env, invoice_id, &lender, loan.repayment);
    Ok(())
}

pub(crate) fn transfer_position(
    env: &Env,
    lender: Address,
    invoice_id: u64,
    new_lender: Address,
) -> Result<(), Error> {
    let mut loan = get_loan(env, invoice_id).ok_or(Error::LoanNotFound)?;
    if loan.status != LoanStatus::Funded {
        return Err(Error::LoanNotFunded);
    }
    if loan.lender.as_ref() != Some(&lender) {
        return Err(Error::Unauthorized);
    }
    lender.require_auth();
    if new_lender == loan.borrower {
        return Err(Error::SelfDealing);
    }
    loan.lender = Some(new_lender.clone());
    save_loan(env, &loan);
    emit_transfer(env, invoice_id, &lender, &new_lender);
    Ok(())
}

pub(crate) fn mark_defaulted(env: &Env, invoice_id: u64) -> Result<(), Error> {
    let mut loan = get_loan(env, invoice_id).ok_or(Error::LoanNotFound)?;
    if loan.status != LoanStatus::Funded {
        return Err(Error::LoanNotFunded);
    }
    let invoice = get_invoice(env, invoice_id)?;
    if invoice.status != EscrowStatus::Refunded && invoice.status != EscrowStatus::Cancelled {
        return Err(Error::InvalidStatus);
    }
    let lender = loan.lender.clone().expect("funded loan has a lender");
    loan.status = LoanStatus::Defaulted;
    save_loan(env, &loan);
    emit_defaulted(env, invoice_id, &lender, loan.repayment);
    Ok(())
}

/// Invoice IDs whose listing is open and not yet expired.
pub(crate) fn open_listings(env: &Env) -> Vec<u64> {
    let now = env.ledger().timestamp();
    let mut out: Vec<u64> = Vec::new(env);
    for id in get_index(env).iter() {
        if let Some(l) = get_loan(env, id) {
            if l.status == LoanStatus::Open && l.expires_at >= now {
                out.push_back(id);
            }
        }
    }
    out
}
