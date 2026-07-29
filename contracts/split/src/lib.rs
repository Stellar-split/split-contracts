//! StellarSplit â on-chain invoice & payment splitting contract.
//!
//! Allows a creator to define an invoice with multiple recipients and amounts.
//! Payers contribute funds; once fully funded the contract auto-routes USDC to
//! each recipient. If the deadline passes unfunded, payers are refunded.
//!
//! Additionally features audit logging, invoice archival, and a contributor leaderboard.

#![no_std]

mod events;
mod types;

#[cfg(test)]
mod test;

use soroban_sdk::{contract, contractimpl, symbol_short, token, Address, Env, Symbol, Vec};
use types::{Invoice, InvoiceStatus, Payment, TransferKind, TransferRecord};

// ---------------------------------------------------------------------------
// Storage helpers
// ---------------------------------------------------------------------------

/// Storage key for the auto-incrementing invoice counter.
fn counter_key() -> Symbol {
    symbol_short!("counter")
}

/// Composite storage key for an invoice: (symbol, id).
fn invoice_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("inv"), id)
}

/// Composite storage key for an audit log: (symbol, invoice_id).
fn audit_log_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("audit_log"), invoice_id)
}

/// Composite storage key for an archived invoice: (symbol, id).
fn archived_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("archived_inv"), id)
}

/// Composite storage key for the top contributors leaderboard.
fn top_contributors_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("top_contribs"), invoice_id)
}

/// Storage key for the max audit log entries configuration.
fn max_audit_log_entries_key() -> Symbol {
    symbol_short!("max_audit_entries")
}

/// Storage key for the max leaderboard size configuration.
fn max_leaderboard_size_key() -> Symbol {
    symbol_short!("max_leaderboard_size")
}

fn load_invoice(env: &Env, id: u64) -> Invoice {
    env.storage()
        .persistent()
        .get(&invoice_key(id))
        .expect("invoice not found")
}

fn save_invoice(env: &Env, id: u64, invoice: &Invoice) {
    env.storage()
        .persistent()
        .set(&invoice_key(id), invoice);
}

fn remove_invoice(env: &Env, id: u64) {
    env.storage().persistent().remove(&invoice_key(id));
}

fn load_audit_log(env: &Env, invoice_id: u64) -> Vec<TransferRecord> {
    env.storage()
        .persistent()
        .get(&audit_log_key(invoice_id))
        .unwrap_or(Vec::new(env))
}

fn append_audit_record(env: &Env, invoice_id: u64, record: &TransferRecord) {
    let mut log = load_audit_log(env, invoice_id);
    let max = get_max_audit_log_entries(env);
    if log.len() >= max as usize {
        return;
    }
    log.push_back(record.clone());
    env.storage().persistent().set(&audit_log_key(invoice_id), &log);
}

fn get_max_audit_log_entries(env: &Env) -> u32 {
    env.storage()
        .persistent()
        .get(&max_audit_log_entries_key())
        .unwrap_or(1_000u32)
}

fn get_max_leaderboard_size(env: &Env) -> u32 {
    env.storage()
        .persistent()
        .get(&max_leaderboard_size_key())
        .unwrap_or(10u32)
}

fn load_top_contributors(env: &Env, invoice_id: u64) -> Vec<(Address, i128)> {
    env.storage()
        .persistent()
        .get(&top_contributors_key(invoice_id))
        .unwrap_or(Vec::new(env))
}

fn save_top_contributors(env: &Env, invoice_id: u64, leaders: &Vec<(Address, i128)>) {
    env.storage()
        .persistent()
        .set(&top_contributors_key(invoice_id), leaders);
}

fn update_leaderboard(env: &Env, invoice_id: u64, payer: &Address, amount: i128) {
    let max = get_max_leaderboard_size(env);
    let mut leaders = load_top_contributors(env, invoice_id);

    let payer_cli = payer.clone();
    let mut found_index = None;
    for i in 0..leaders.len() {
        let (ref addr, _) = leaders.get(i).unwrap();
        if addr == &payer_cli {
            found_index = Some(i);
            break;
        }
    }

    let existing_amount = if let Some(idx) = found_index {
        let (_, amt) = leaders.get(idx).unwrap();
        Some(amt)
    } else {
        None
    };

    let new_amount = existing_amount.unwrap_or(0i128) + amount;

    if let Some(idx) = found_index {
        leaders.set(idx, (payer_cli.clone(), new_amount));
    } else {
        leaders.push_back((payer_cli.clone(), amount));
    }

    if found_index.is_some() {
        let idx = if let Some(i) = found_index {
            i
        } else {
            leaders.len() - 1
        };
        let (_, new_amt) = leaders.get(idx).unwrap();
        let mut j = idx;
        while j > 0 {
            let (_, prev_amt) = leaders.get(j - 1).unwrap();
            if new_amt > prev_amt {
                let curr = leaders.get(idx).unwrap();
                leaders.set(j, curr);
                j -= 1;
            } else {
                break;
            }
        }
        leaders.set(j, (payer_cli.clone(), new_amount));
    } else {
        let new_len = leaders.len();
        if new_len > 1 {
            let mut j = new_len - 1;
            while j > 0 {
                let (_, curr_amt) = leaders.get(j).unwrap();
                let (_, prev_amt) = leaders.get(j - 1).unwrap();
                if curr_amt > prev_amt {
                    let curr = leaders.get(j).unwrap();
                    leaders.set(j, curr);
                    j -= 1;
                } else {
                    break;
                }
            }
            leaders.set(j, (payer_cli, new_amount));
        }
    }

    while leaders.len() > max as usize {
        leaders.pop_back();
    }

    save_top_contributors(env, invoice_id, &leaders);
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct SplitContract;

#[contractimpl]
impl SplitContract {
    /// Create a new invoice.
    ///
    /// # Arguments
    /// * `creator`       - address that owns the invoice (must authorise)
    /// * `recipients`    - ordered list of recipient addresses
    /// * `amounts`       - amount owed to each recipient (parallel to `recipients`)
    /// * `token`         - USDC token contract address
    /// * `deadline_ledger` - ledger sequence after which unfunded invoices can be refunded
    ///
    /// # Returns
    /// The new invoice ID (monotonically increasing u64).
    pub fn create_invoice(
        env: Env,
        creator: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        token: Address,
        deadline_ledger: u32,
    ) -> u64 {
        creator.require_auth();

        assert!(
            recipients.len() == amounts.len(),
            "recipients and amounts length mismatch"
        );
        assert!(!recipients.is_empty(), "must have at least one recipient");
        assert!(
            deadline_ledger > env.ledger().sequence(),
            "deadline must be in the future"
        );

        for amt in amounts.iter() {
            assert!(amt > 0, "amounts must be positive");
        }

        // Increment and persist the invoice counter.
        let id: u64 = env
            .storage()
            .persistent()
            .get(&counter_key())
            .unwrap_or(0u64)
            + 1;
        env.storage().persistent().set(&counter_key(), &id);

        let total: i128 = amounts.iter().sum();

        let invoice = Invoice {
            creator: creator.clone(),
            recipients: recipients.clone(),
            amounts,
            token,
            deadline_ledger,
            funded: 0,
            status: InvoiceStatus::Pending,
            payments: Vec::new(&env),
        };

        save_invoice(&env, id, &invoice);
        events::invoice_created(&env, id, &creator, total);

        id
    }

    /// Pay toward an invoice.
    ///
    /// Transfers `amount` of the invoice token from `payer` to this contract.
    /// Auto-releases funds if the invoice becomes fully funded.
    ///
    /// # Arguments
    /// * `payer`      - address making the payment (must authorise)
    /// * `invoice_id` - target invoice
    /// * `amount`     - amount to pay in stroops
    pub fn pay(env: Env, payer: Address, invoice_id: u64, amount: i128) {
        payer.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(
            env.ledger().sequence() <= invoice.deadline_ledger,
            "invoice deadline has passed"
        );
        assert!(amount > 0, "payment amount must be positive");

        let total: i128 = invoice.amounts.iter().sum();
        let remaining = total - invoice.funded;
        assert!(amount <= remaining, "payment exceeds remaining balance");

        // Transfer tokens from payer to this contract.
        let token_client = token::Client::new(&env, &invoice.token);
        token_client.transfer(&payer, &env.current_contract_address(), &amount);

        append_audit_record(
            &env,
            invoice_id,
            &TransferRecord {
                from: payer.clone(),
                to: env.current_contract_address(),
                amount,
                kind: TransferKind::Contribution,
                ledger: env.ledger().sequence(),
            },
        );

        invoice.payments.push_back(Payment {
            payer: payer.clone(),
            amount,
        });
        invoice.funded += amount;

        update_leaderboard(&env, invoice_id, &payer, amount);

        events::payment_received(&env, invoice_id, &payer, amount);

        // Auto-release if fully funded.
        if invoice.funded >= total {
            Self::_release(&env, invoice_id, &mut invoice);
        } else {
            save_invoice(&env, invoice_id, &invoice);
        }
    }

    /// Release funds to all recipients once the invoice is fully funded.
    ///
    /// Can be called by anyone; validates full funding internally.
    pub fn release(env: Env, invoice_id: u64) {
        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        let total: i128 = invoice.amounts.iter().sum();
        assert!(invoice.funded >= total, "invoice not fully funded");

        Self::_release(&env, invoice_id, &mut invoice);
    }

    /// Refund all payers if the deadline has passed and the invoice is not fully funded.
    ///
    /// Can be called by anyone after the deadline.
    pub fn refund(env: Env, invoice_id: u64) {
        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(
            env.ledger().sequence() > invoice.deadline_ledger,
            "deadline has not passed"
        );

        let token_client = token::Client::new(&env, &invoice.token);

        for payment in invoice.payments.iter() {
            token_client.transfer(
                &env.current_contract_address(),
                &payment.payer,
                &payment.amount,
            );

            append_audit_record(
                &env,
                invoice_id,
                &TransferRecord {
                    from: env.current_contract_address(),
                    to: payment.payer.clone(),
                    amount: payment.amount,
                    kind: TransferKind::Refund,
                    ledger: env.ledger().sequence(),
                },
            );
        }

        invoice.status = InvoiceStatus::Refunded;
        archive_invoice(&env, invoice_id, &invoice);
        events::invoice_refunded(&env, invoice_id);
    }

    /// Retrieve an invoice by ID.
    pub fn get_invoice(env: Env, invoice_id: u64) -> Invoice {
        load_invoice(&env, invoice_id)
    }

    /// Retrieve the audit log for an invoice.
    pub fn get_audit_log(env: Env, invoice_id: u64) -> Vec<TransferRecord> {
        load_audit_log(&env, invoice_id)
    }

    /// Retrieve an archived invoice by ID.
    pub fn get_archived_invoice(env: Env, invoice_id: u64) -> Invoice {
        env.storage()
            .persistent()
            .get(&archived_key(invoice_id))
            .expect("archived invoice not found")
    }

    /// Retrieve the leaderboard for an invoice.
    ///
    /// Returns up to `n` top contributors sorted by cumulative paid amount descending.
    pub fn get_top_contributors(env: Env, invoice_id: u64, n: u32) -> Vec<(Address, i128)> {
        let leaders = load_top_contributors(&env, invoice_id);
        let mut result = Vec::new(&env);
        let count = if n as usize > leaders.len() {
            leaders.len()
        } else {
            n as usize
        };
        for i in 0..count {
            let entry = leaders.get(i).unwrap();
            result.push_back(entry);
        }
        result
    }

    /// Set the maximum number of audit log entries per invoice.
    /// Only callable by the contract creator (admin).
    pub fn set_max_audit_log_entries(env: Env, admin: Address, max: u32) {
        admin.require_auth();
        env.storage()
            .persistent()
            .set(&max_audit_log_entries_key(), &max);
    }

    /// Set the maximum number of leaderboard entries per invoice.
    /// Only callable by the contract creator (admin).
    pub fn set_max_leaderboard_size(env: Env, admin: Address, max: u32) {
        admin.require_auth();
        env.storage()
            .persistent()
            .set(&max_leaderboard_size_key(), &max);
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Route funds to all recipients and mark the invoice as released.
    fn _release(env: &Env, invoice_id: u64, invoice: &mut Invoice) {
        let token_client = token::Client::new(env, &invoice.token);

        for (recipient, amount) in invoice.recipients.iter().zip(invoice.amounts.iter()) {
            token_client.transfer(&env.current_contract_address(), &recipient, &amount);

            append_audit_record(
                env,
                invoice_id,
                &TransferRecord {
                    from: env.current_contract_address(),
                    to: recipient.clone(),
                    amount: *amount,
                    kind: TransferKind::Payout,
                    ledger: env.ledger().sequence(),
                },
            );
        }

        invoice.status = InvoiceStatus::Released;
        archive_invoice(env, invoice_id, invoice);
        events::invoice_released(
            env,
            invoice_id,
            &invoice.recipients,
        );
    }
}

/// Move a finalised invoice from hot storage to cold archival storage.
fn archive_invoice(env: &Env, invoice_id: u64, invoice: &Invoice) {
    env.storage()
        .persistent()
        .set(&archived_key(invoice_id), invoice);
    remove_invoice(env, invoice_id);
}