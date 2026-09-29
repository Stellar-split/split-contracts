//! Issue #765: read helper for the existing per-invoice pause / auto-resume
//! mechanism (`pause_invoice`, `resume_invoice`, lazy auto-resume in `_pay`).

use super::*;
use soroban_sdk::{contractimpl, Env};

#[contractimpl]
impl SplitContract {
    /// True when the invoice is paused and any configured `auto_resume_at`
    /// timestamp has not yet elapsed. (Named `is_invoice_paused` because
    /// `is_paused(env)` already reports the contract-wide flag.)
    pub fn is_invoice_paused(env: Env, invoice_id: u64) -> bool {
        let invoice = load_invoice(&env, invoice_id);
        if !invoice.frozen {
            return false;
        }
        match invoice.auto_resume_at {
            Some(at) => env.ledger().timestamp() < at,
            None => true,
        }
    }
}
