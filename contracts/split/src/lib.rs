//! StellarSplit — on-chain invoice & payment splitting contract.
//!
//! Allows a creator to define an invoice with multiple recipients and amounts.
//! Payers contribute funds; once fully funded the contract auto-routes USDC to
//! each recipient. If the deadline passes unfunded, payers are refunded.
//!
//! ## Features added in this branch
//! * **#522** — Cross-invoice linkage: a child invoice's release is blocked
//!   until its parent is `Released` / `Finalised`.
//! * **#523** — Late-payment penalty: contributions after the deadline but
//!   within a grace window incur an extra penalty transferred to the treasury.
//! * **#524** — Batch creation: `batch_create_invoices` creates up to
//!   `MAX_BATCH_SIZE` invoices atomically.
//! * **#525** — Largest-remainder rounding: `_release` uses
//!   `calc::distribute_with_remainder` so every stroop is accounted for.

#![no_std]

mod calc;
mod events;
mod types;

#[cfg(test)]
mod test;

use soroban_sdk::{
    contract, contractimpl, panic_with_error, symbol_short, token, Address, Env, Symbol, Vec,
};
use types::{ContractError, Invoice, InvoiceParams, InvoiceStatus, Payment};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of invoices that can be created in a single batch (#524).
const MAX_BATCH_SIZE: u32 = 50;

/// Maximum parent-chain depth allowed before we stop recursing (#522).
const MAX_PARENT_DEPTH: u32 = 10;

/// Grace period (in seconds) after the deadline during which a contribution
/// is accepted but treated as "late" (#523).
const GRACE_WINDOW: u64 = 86_400; // 24 hours

// ---------------------------------------------------------------------------
// Storage helpers
// ---------------------------------------------------------------------------

/// Storage key for the auto-incrementing invoice counter.
fn counter_key() -> Symbol {
    symbol_short!("counter")
}

/// Composite storage key for an invoice: `(symbol, id)`.
fn invoice_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("inv"), id)
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

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct SplitContract;

#[contractimpl]
impl SplitContract {
    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// Create a new invoice.
    ///
    /// # Arguments
    /// * `creator`           – address that owns the invoice (must authorise)
    /// * `recipients`        – ordered list of recipient addresses
    /// * `amounts`           – amount owed to each recipient (parallel to `recipients`)
    /// * `token`             – USDC token contract address
    /// * `deadline`          – Unix timestamp; after this refunds become available
    /// * `parent_invoice_id` – optional ID of a parent invoice (#522)
    /// * `late_penalty_bps`  – penalty in basis points for late contributions (#523)
    ///
    /// # Returns
    /// The new invoice ID (monotonically increasing u64).
    pub fn create_invoice(
        env: Env,
        creator: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        token: Address,
        deadline: u64,
        parent_invoice_id: Option<u64>,
        late_penalty_bps: u32,
    ) -> u64 {
        creator.require_auth();

        assert!(
            recipients.len() == amounts.len(),
            "recipients and amounts length mismatch"
        );
        assert!(!recipients.is_empty(), "must have at least one recipient");
        assert!(
            deadline > env.ledger().timestamp(),
            "deadline must be in the future"
        );

        for amt in amounts.iter() {
            assert!(amt > 0, "amounts must be positive");
        }

        // #522 — validate parent reference before creating the invoice.
        if let Some(parent_id) = parent_invoice_id {
            Self::_validate_parent(&env, parent_id, 0);
        }

        Self::_create_invoice_inner(
            &env,
            creator,
            recipients,
            amounts,
            token,
            deadline,
            parent_invoice_id,
            late_penalty_bps,
        )
    }

    /// Pay toward an invoice.
    ///
    /// Transfers `amount` of the invoice token from `payer` to this contract.
    /// Auto-releases funds if the invoice becomes fully funded.
    ///
    /// Contributions after the `deadline` but within [`GRACE_WINDOW`] are
    /// accepted and flagged as late; the `late_penalty_bps` fee is charged
    /// on top and transferred to `treasury` (#523).
    ///
    /// # Arguments
    /// * `payer`      – address making the payment (must authorise)
    /// * `invoice_id` – target invoice
    /// * `amount`     – amount to pay in stroops (before any penalty)
    /// * `treasury`   – address that receives late-payment penalties;
    ///                  ignored for on-time contributions
    pub fn pay(env: Env, payer: Address, invoice_id: u64, amount: i128, treasury: Address) {
        payer.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        let now = env.ledger().timestamp();

        // Reject anything past the grace window.
        assert!(
            now <= invoice.deadline + GRACE_WINDOW,
            "invoice deadline has passed"
        );

        assert!(amount > 0, "payment amount must be positive");

        let total: i128 = invoice.amounts.iter().sum();
        let remaining = total - invoice.funded;
        assert!(amount <= remaining, "payment exceeds remaining balance");

        // #523 — determine whether this contribution is late.
        let is_late = now > invoice.deadline;

        let token_client = token::Client::new(&env, &invoice.token);

        if is_late && invoice.late_penalty_bps > 0 {
            // penalty = ceil(amount * late_penalty_bps / 10_000)
            let penalty: i128 =
                (amount * invoice.late_penalty_bps as i128 + 9_999) / 10_000;

            // Transfer principal to the contract.
            token_client.transfer(&payer, &env.current_contract_address(), &amount);

            // Transfer penalty to treasury.
            token_client.transfer(&payer, &treasury, &penalty);

            events::late_payment_penalty_charged(&env, invoice_id, &payer, penalty);
        } else {
            // On-time (or zero-penalty late) — transfer principal only.
            token_client.transfer(&payer, &env.current_contract_address(), &amount);
        }

        invoice.payments.push_back(Payment {
            payer: payer.clone(),
            amount,
        });
        invoice.funded += amount;

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
    /// For child invoices (#522), the parent must be `Released` or `Finalised`
    /// before this call can succeed (enforced inside `_release`).
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

    /// Refund all payers if the deadline (+ grace window) has passed and the
    /// invoice is not fully funded.
    pub fn refund(env: Env, invoice_id: u64) {
        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(
            env.ledger().timestamp() > invoice.deadline + GRACE_WINDOW,
            "deadline has not passed"
        );

        let token_client = token::Client::new(&env, &invoice.token);

        for payment in invoice.payments.iter() {
            token_client.transfer(
                &env.current_contract_address(),
                &payment.payer,
                &payment.amount,
            );
        }

        invoice.status = InvoiceStatus::Refunded;
        save_invoice(&env, invoice_id, &invoice);
        events::invoice_refunded(&env, invoice_id);
    }

    /// Retrieve an invoice by ID.
    pub fn get_invoice(env: Env, invoice_id: u64) -> Invoice {
        load_invoice(&env, invoice_id)
    }

    /// Create multiple invoices atomically in a single transaction (#524).
    ///
    /// All invoices are created or none are (the transaction rolls back on any
    /// failure).  Returns the IDs of the newly created invoices, in order.
    ///
    /// # Arguments
    /// * `invoices` – up to `MAX_BATCH_SIZE` invoice parameter structs
    ///
    /// # Errors
    /// * `BatchTooLarge` if `invoices.len() > MAX_BATCH_SIZE`
    pub fn batch_create_invoices(env: Env, invoices: Vec<InvoiceParams>) -> Vec<u64> {
        if invoices.len() > MAX_BATCH_SIZE {
            panic_with_error!(&env, ContractError::BatchTooLarge);
        }

        assert!(!invoices.is_empty(), "batch must not be empty");

        let mut ids: Vec<u64> = Vec::new(&env);

        for params in invoices.iter() {
            params.creator.require_auth();

            assert!(
                params.recipients.len() == params.amounts.len(),
                "recipients and amounts length mismatch"
            );
            assert!(
                !params.recipients.is_empty(),
                "must have at least one recipient"
            );
            assert!(
                params.deadline > env.ledger().timestamp(),
                "deadline must be in the future"
            );
            for amt in params.amounts.iter() {
                assert!(amt > 0, "amounts must be positive");
            }

            // #522 — validate parent reference.
            if let Some(parent_id) = params.parent_invoice_id {
                Self::_validate_parent(&env, parent_id, 0);
            }

            let id = Self::_create_invoice_inner(
                &env,
                params.creator.clone(),
                params.recipients.clone(),
                params.amounts.clone(),
                params.token.clone(),
                params.deadline,
                params.parent_invoice_id,
                params.late_penalty_bps,
            );
            ids.push_back(id);
        }

        // #524 — emit one BatchInvoiceCreated event with all IDs.
        events::batch_invoice_created(&env, &ids);

        ids
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Core invoice creation logic, shared by `create_invoice` and
    /// `batch_create_invoices`.
    fn _create_invoice_inner(
        env: &Env,
        creator: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        token: Address,
        deadline: u64,
        parent_invoice_id: Option<u64>,
        late_penalty_bps: u32,
    ) -> u64 {
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
            deadline,
            funded: 0,
            status: InvoiceStatus::Pending,
            payments: Vec::new(env),
            parent_invoice_id,
            late_penalty_bps,
        };

        save_invoice(env, id, &invoice);
        events::invoice_created(env, id, &creator, total);

        id
    }

    /// Route funds to all recipients using the largest-remainder method (#525)
    /// and mark the invoice as `Released`.
    ///
    /// Also enforces the #522 parent-gate so both manual `release` calls and
    /// auto-release triggered from `pay` respect the parent constraint.
    fn _release(env: &Env, invoice_id: u64, invoice: &mut Invoice) {
        // #522 — block release if parent is not yet finalised.
        if let Some(parent_id) = invoice.parent_invoice_id {
            let parent = load_invoice(env, parent_id);
            let parent_ok = parent.status == InvoiceStatus::Released
                || parent.status == InvoiceStatus::Finalised;
            if !parent_ok {
                panic_with_error!(env, ContractError::ParentInvoiceNotFinalised);
            }
        }

        let token_client = token::Client::new(env, &invoice.token);

        // #525 — use amounts as ratios; denom = sum of amounts.
        let denom: i128 = invoice.amounts.iter().sum();
        let funded = invoice.funded;

        let shares = if denom == 0 {
            // Edge case: all amounts are zero — nothing to distribute.
            let mut z = Vec::new(env);
            for _ in 0..invoice.recipients.len() {
                z.push_back(0i128);
            }
            z
        } else {
            calc::distribute_with_remainder(env, funded, &invoice.amounts, denom)
        };

        for (i, recipient) in invoice.recipients.iter().enumerate() {
            let share = shares.get(i as u32).unwrap_or(0);
            if share > 0 {
                token_client.transfer(&env.current_contract_address(), &recipient, &share);
            }
        }

        // #522 — emit ChildInvoiceUnblocked if this is a child invoice.
        if let Some(parent_id) = invoice.parent_invoice_id {
            events::child_invoice_unblocked(env, invoice_id, parent_id);
        }

        invoice.status = InvoiceStatus::Released;
        save_invoice(env, invoice_id, invoice);
        events::invoice_released(env, invoice_id, &invoice.recipients);
    }

    /// #522 — Walk the parent chain and verify:
    /// 1. The chain depth does not exceed `MAX_PARENT_DEPTH`.
    /// 2. Each referenced invoice exists.
    ///
    /// `depth` starts at 0 for the direct parent.
    fn _validate_parent(env: &Env, parent_id: u64, depth: u32) {
        if depth >= MAX_PARENT_DEPTH {
            panic_with_error!(env, ContractError::ParentChainTooDeep);
        }

        // Load the parent — panics with "invoice not found" if it doesn't exist.
        let parent = load_invoice(env, parent_id);

        // Recurse if this parent also has a parent.
        if let Some(grandparent_id) = parent.parent_invoice_id {
            Self::_validate_parent(env, grandparent_id, depth + 1);
        }
    }
}
