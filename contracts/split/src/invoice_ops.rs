//! Invoice operations added in issues #821, #822, #823 and #825:
//! smart categorization, payment priority levels, event archival and
//! payment confirmation delays.

use soroban_sdk::{contractimpl, panic_with_error, symbol_short, Address, Env, Symbol, Vec};

use crate::error::ContractError;
use crate::events;
use crate::types::AuditEntry;
use crate::{
    audit_log_key, get_audit_log, load_invoice, require_admin, SplitContract, SplitContractArgs,
    SplitContractClient,
};

/// Default upper bound (exclusive) of the "small" category.
const DEFAULT_SMALL_THRESHOLD: i128 = 1_000;
/// Default lower bound (inclusive) of the "large" category.
const DEFAULT_LARGE_THRESHOLD: i128 = 100_000;

/// Priority levels (issue #822).
pub const PRIORITY_LOW: u32 = 0;
pub const PRIORITY_NORMAL: u32 = 1;
pub const PRIORITY_HIGH: u32 = 2;

fn cat_thresholds_key() -> Symbol {
    symbol_short!("cat_thr")
}

fn recipient_category_key(recipient: &Address) -> (Symbol, Address) {
    (symbol_short!("rcpt_cat"), recipient.clone())
}

fn invoice_category_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("inv_cat"), invoice_id)
}

fn priority_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("pay_prio"), invoice_id)
}

fn cold_events_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("evt_cold"), invoice_id)
}

fn confirm_blocks_key() -> Symbol {
    symbol_short!("conf_blk")
}

fn pending_payment_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("conf_pay"), invoice_id, payer.clone())
}

#[contractimpl]
impl SplitContract {
    // -----------------------------------------------------------------------
    // Issue #821: Smart invoice categorization
    // -----------------------------------------------------------------------

    /// Admin: configure amount thresholds used by `categorize_invoice`.
    /// Totals `< small` are "small", `>= large` are "large", else "medium".
    pub fn set_category_thresholds(env: Env, small: i128, large: i128) {
        require_admin(&env);
        assert!(small > 0 && small < large, "invalid thresholds");
        env.storage()
            .instance()
            .set(&cat_thresholds_key(), &(small, large));
    }

    /// Admin: tag a recipient with a category. Invoices paying this recipient
    /// are categorized by recipient before falling back to amount.
    pub fn set_recipient_category(env: Env, recipient: Address, category: Symbol) {
        require_admin(&env);
        env.storage()
            .persistent()
            .set(&recipient_category_key(&recipient), &category);
    }

    /// Auto-tag an invoice by recipient (first tagged recipient wins) or by
    /// total amount. Stores and returns the category.
    pub fn categorize_invoice(env: Env, invoice_id: u64) -> Symbol {
        let invoice = load_invoice(&env, invoice_id);
        let mut category: Option<Symbol> = None;
        for r in invoice.recipients.iter() {
            if let Some(c) = env
                .storage()
                .persistent()
                .get::<_, Symbol>(&recipient_category_key(&r))
            {
                category = Some(c);
                break;
            }
        }
        let category = category.unwrap_or_else(|| {
            let (small, large): (i128, i128) = env
                .storage()
                .instance()
                .get(&cat_thresholds_key())
                .unwrap_or((DEFAULT_SMALL_THRESHOLD, DEFAULT_LARGE_THRESHOLD));
            let total: i128 = invoice.amounts.iter().sum();
            if total < small {
                symbol_short!("small")
            } else if total >= large {
                symbol_short!("large")
            } else {
                symbol_short!("medium")
            }
        });
        env.storage()
            .persistent()
            .set(&invoice_category_key(invoice_id), &category);
        events::invoice_categorized(&env, invoice_id, &category);
        category
    }

    /// Return the stored category for an invoice, if it has been categorized.
    pub fn get_invoice_category(env: Env, invoice_id: u64) -> Option<Symbol> {
        env.storage()
            .persistent()
            .get(&invoice_category_key(invoice_id))
    }

    // -----------------------------------------------------------------------
    // Issue #822: Payment priority levels
    // -----------------------------------------------------------------------

    /// Creator: set an invoice's payment priority (0 = low, 1 = normal,
    /// 2 = high).
    pub fn set_payment_priority(env: Env, invoice_id: u64, level: u32) {
        let invoice = load_invoice(&env, invoice_id);
        invoice.creator.require_auth();
        if level > PRIORITY_HIGH {
            panic_with_error!(env, ContractError::InvalidPriority);
        }
        let old = Self::get_payment_priority(env.clone(), invoice_id);
        env.storage()
            .persistent()
            .set(&priority_key(invoice_id), &level);
        events::payment_priority_set(&env, invoice_id, old, level);
    }

    /// Return an invoice's payment priority (defaults to normal).
    pub fn get_payment_priority(env: Env, invoice_id: u64) -> u32 {
        env.storage()
            .persistent()
            .get(&priority_key(invoice_id))
            .unwrap_or(PRIORITY_NORMAL)
    }

    /// Order invoice IDs for processing: highest priority first, preserving
    /// input order within the same priority level.
    pub fn sort_by_priority(env: Env, invoice_ids: Vec<u64>) -> Vec<u64> {
        let mut out = Vec::new(&env);
        let mut level = PRIORITY_HIGH as i64;
        while level >= PRIORITY_LOW as i64 {
            for id in invoice_ids.iter() {
                if Self::get_payment_priority(env.clone(), id) as i64 == level {
                    out.push_back(id);
                }
            }
            level -= 1;
        }
        out
    }

    // -----------------------------------------------------------------------
    // Issue #823: Contract event archival
    // -----------------------------------------------------------------------

    /// Admin: move audit events older than `before` into cold storage.
    /// Returns the number of events archived.
    pub fn archive_events(env: Env, invoice_id: u64, before: u64) -> u32 {
        require_admin(&env);
        let log = get_audit_log(&env, invoice_id);
        let mut hot: Vec<AuditEntry> = Vec::new(&env);
        let mut cold: Vec<AuditEntry> = Self::get_archived_events(env.clone(), invoice_id);
        let mut count: u32 = 0;
        for e in log.iter() {
            if e.timestamp < before {
                cold.push_back(e);
                count += 1;
            } else {
                hot.push_back(e);
            }
        }
        if count > 0 {
            let key = audit_log_key(invoice_id);
            if env.storage().persistent().has(&key) {
                env.storage().persistent().set(&key, &hot);
            } else {
                env.storage().instance().set(&key, &hot);
            }
            env.storage()
                .persistent()
                .set(&cold_events_key(invoice_id), &cold);
        }
        events::events_archived(&env, invoice_id, count, before);
        count
    }

    /// Return the audit events that were moved to cold storage.
    pub fn get_archived_events(env: Env, invoice_id: u64) -> Vec<AuditEntry> {
        env.storage()
            .persistent()
            .get(&cold_events_key(invoice_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    // -----------------------------------------------------------------------
    // Issue #825: Payment confirmation delays
    // -----------------------------------------------------------------------

    /// Admin: set how many ledgers a queued payment must wait before it can
    /// be credited.
    pub fn set_confirmation_blocks(env: Env, blocks: u32) {
        require_admin(&env);
        env.storage().instance().set(&confirm_blocks_key(), &blocks);
    }

    /// Return the configured confirmation delay in ledgers (default 0).
    pub fn get_confirmation_blocks(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&confirm_blocks_key())
            .unwrap_or(0)
    }

    /// Queue a payment; it can be credited via `confirm_payment` once the
    /// confirmation delay has elapsed. Returns the ledger it confirms at.
    pub fn queue_payment(
        env: Env,
        payer: Address,
        invoice_id: u64,
        amount: i128,
        nonce: u64,
    ) -> u32 {
        payer.require_auth();
        assert!(amount > 0, "amount must be positive");
        load_invoice(&env, invoice_id);
        let confirm_at = env
            .ledger()
            .sequence()
            .saturating_add(Self::get_confirmation_blocks(env.clone()));
        env.storage().persistent().set(
            &pending_payment_key(invoice_id, &payer),
            &(amount, nonce, confirm_at),
        );
        events::payment_queued(&env, invoice_id, &payer, amount, confirm_at);
        confirm_at
    }

    /// Credit a queued payment once `confirm_at` ledger has been reached.
    pub fn confirm_payment(env: Env, payer: Address, invoice_id: u64) {
        let key = pending_payment_key(invoice_id, &payer);
        let (amount, nonce, confirm_at): (i128, u64, u32) = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic_with_error!(env, ContractError::NoPendingPayment));
        if env.ledger().sequence() < confirm_at {
            panic_with_error!(env, ContractError::ConfirmationPending);
        }
        env.storage().persistent().remove(&key);
        Self::pay(
            env.clone(),
            payer.clone(),
            invoice_id,
            amount,
            nonce,
            false,
            false,
            None,
        );
        events::payment_confirmed(&env, invoice_id, &payer, amount);
    }

    /// Return a pending payment as `(amount, nonce, confirm_at_ledger)`.
    pub fn get_pending_payment(
        env: Env,
        payer: Address,
        invoice_id: u64,
    ) -> Option<(i128, u64, u32)> {
        env.storage()
            .persistent()
            .get(&pending_payment_key(invoice_id, &payer))
    }
}
