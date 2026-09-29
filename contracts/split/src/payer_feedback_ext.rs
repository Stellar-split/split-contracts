//! Issue #819: creator feedback system.
//!
//! Any address that has paid an invoice may leave one feedback note for the
//! invoice creator. Notes are capped in length and count per invoice.

use super::*;
use soroban_sdk::{contractimpl, contracttype, symbol_short, Address, Env, String, Symbol, Vec};

pub const MAX_FEEDBACK_LEN: u32 = 280;
pub const MAX_FEEDBACK_PER_INVOICE: u32 = 50;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PayerFeedback {
    pub payer: Address,
    pub message: String,
    pub timestamp: u64,
}

fn feedback_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("pay_fb"), invoice_id)
}

#[contractimpl]
impl SplitContract {
    /// Leave a note for the invoice creator. Only payers of the invoice, once each.
    pub fn leave_feedback(env: Env, payer: Address, invoice_id: u64, message: String) {
        require_not_paused(&env);
        payer.require_auth();
        let len = message.len();
        assert!(
            len > 0 && len <= MAX_FEEDBACK_LEN,
            "invalid feedback length"
        );
        let invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.payments.iter().any(|p| p.payer == payer),
            "only payers can leave feedback"
        );
        let mut list = Self::get_feedback(env.clone(), invoice_id);
        assert!(
            list.len() < MAX_FEEDBACK_PER_INVOICE,
            "feedback limit reached"
        );
        assert!(
            !list.iter().any(|f| f.payer == payer),
            "feedback already left"
        );
        list.push_back(PayerFeedback {
            payer: payer.clone(),
            message,
            timestamp: env.ledger().timestamp(),
        });
        env.storage()
            .persistent()
            .set(&feedback_key(invoice_id), &list);
        env.events().publish(
            (
                symbol_short!("split"),
                symbol_short!("feedback"),
                invoice_id,
            ),
            (invoice.creator, payer),
        );
    }

    pub fn get_feedback(env: Env, invoice_id: u64) -> Vec<PayerFeedback> {
        env.storage()
            .persistent()
            .get(&feedback_key(invoice_id))
            .unwrap_or_else(|| Vec::new(&env))
    }
}

#[cfg(test)]
mod tests {
    use crate::ext_test_util::{fixture, mint, new_invoice, pay};
    use soroban_sdk::{testutils::Address as _, Address, String};

    #[test]
    fn payer_can_leave_feedback() {
        let f = fixture();
        let (id, _, _) = new_invoice(&f, 100);
        let p = Address::generate(&f.env);
        mint(&f.env, &f.token, &p, 100);
        pay(&f, &p, id, 10);
        f.c.leave_feedback(&p, &id, &String::from_str(&f.env, "great work"));
        let list = f.c.get_feedback(&id);
        assert_eq!(list.len(), 1);
        assert_eq!(list.get(0).unwrap().payer, p);
    }

    #[test]
    #[should_panic(expected = "only payers can leave feedback")]
    fn non_payer_rejected() {
        let f = fixture();
        let (id, _, _) = new_invoice(&f, 100);
        let p = Address::generate(&f.env);
        f.c.leave_feedback(&p, &id, &String::from_str(&f.env, "hi"));
    }

    #[test]
    #[should_panic(expected = "feedback already left")]
    fn duplicate_feedback_rejected() {
        let f = fixture();
        let (id, _, _) = new_invoice(&f, 100);
        let p = Address::generate(&f.env);
        mint(&f.env, &f.token, &p, 100);
        pay(&f, &p, id, 10);
        f.c.leave_feedback(&p, &id, &String::from_str(&f.env, "a"));
        f.c.leave_feedback(&p, &id, &String::from_str(&f.env, "b"));
    }
}
