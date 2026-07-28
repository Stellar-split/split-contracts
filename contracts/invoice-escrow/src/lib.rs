//! Invoice Escrow Contract — StellarSplit
//!
//! Provides on-chain invoice escrow with:
//! - Multi-payer deposit aggregation
//! - Automatic fund release when fully funded
//! - Deadline-based refund fallback
//! - **Upgradable admin authority** via a two-step `transfer_admin` flow
//!
//! # Admin Transfer Flow
//!
//! The current admin calls [`InvoiceEscrowContract::transfer_admin`] to propose
//! a new admin address. The proposed address must then call
//! [`InvoiceEscrowContract::accept_admin`] to complete the handover. The current
//! admin can cancel the pending proposal at any time via
//! [`InvoiceEscrowContract::cancel_transfer`]. This prevents accidental or
//! malicious lock-out by ensuring the new admin actively confirms the transfer.

#![no_std]

mod errors;
mod types;

#[cfg(test)]
mod test;

use errors::Error;
use soroban_sdk::{
    contract, contractimpl, symbol_short, token, Address, Env, Symbol, Vec,
};
use types::{EscrowInvoice, EscrowStatus};

// ---------------------------------------------------------------------------
// Storage key helpers
// ---------------------------------------------------------------------------

/// Instance storage: the current contract admin.
fn admin_key() -> Symbol {
    symbol_short!("admin")
}

/// Instance storage: the pending (proposed) admin awaiting acceptance.
fn pending_admin_key() -> Symbol {
    symbol_short!("pend_adm")
}

/// Instance storage: monotonic invoice counter.
fn counter_key() -> Symbol {
    symbol_short!("counter")
}

/// Persistent storage: per-invoice record keyed by `(escrow, id)`.
fn invoice_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("escrow"), id)
}

/// Persistent storage: per-payer per-invoice deposit total.
fn deposit_key(id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("deposit"), id, payer.clone())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn require_admin(env: &Env) -> Address {
    let admin: Address = env
        .storage()
        .instance()
        .get(&admin_key())
        .expect("not initialized");
    admin.require_auth();
    admin
}

fn get_invoice(env: &Env, id: u64) -> Result<EscrowInvoice, Error> {
    env.storage()
        .persistent()
        .get(&invoice_key(id))
        .ok_or(Error::InvoiceNotFound)
}

fn save_invoice(env: &Env, id: u64, invoice: &EscrowInvoice) {
    env.storage().persistent().set(&invoice_key(id), invoice);
}

// ---------------------------------------------------------------------------
// Event helpers
// ---------------------------------------------------------------------------

/// Topics: `(escrow, adm_prop)` — Data: `(current_admin, proposed_admin, ledger)`
fn emit_admin_transfer_proposed(env: &Env, current: &Address, proposed: &Address) {
    env.events().publish(
        (symbol_short!("escrow"), symbol_short!("adm_prop")),
        (current.clone(), proposed.clone(), env.ledger().sequence()),
    );
}

/// Topics: `(escrow, adm_acpt)` — Data: `(new_admin, ledger)`
fn emit_admin_transfer_accepted(env: &Env, new_admin: &Address) {
    env.events().publish(
        (symbol_short!("escrow"), symbol_short!("adm_acpt")),
        (new_admin.clone(), env.ledger().sequence()),
    );
}

/// Topics: `(escrow, adm_cncl)` — Data: `(admin, cancelled_pending, ledger)`
fn emit_admin_transfer_cancelled(env: &Env, admin: &Address, cancelled: &Address) {
    env.events().publish(
        (symbol_short!("escrow"), symbol_short!("adm_cncl")),
        (admin.clone(), cancelled.clone(), env.ledger().sequence()),
    );
}

/// Topics: `(escrow, created, id)` — Data: `(creator, total_amount, deadline)`
fn emit_invoice_created(env: &Env, id: u64, creator: &Address, total: i128, deadline: u64) {
    env.events().publish(
        (symbol_short!("escrow"), symbol_short!("created"), id),
        (creator.clone(), total, deadline),
    );
}

/// Topics: `(escrow, deposited, id)` — Data: `(payer, amount, funded_so_far)`
fn emit_deposited(env: &Env, id: u64, payer: &Address, amount: i128, funded: i128) {
    env.events().publish(
        (symbol_short!("escrow"), symbol_short!("deposited"), id),
        (payer.clone(), amount, funded),
    );
}

/// Topics: `(escrow, released, id)` — Data: total_amount
fn emit_released(env: &Env, id: u64, total: i128) {
    env.events().publish(
        (symbol_short!("escrow"), symbol_short!("released"), id),
        total,
    );
}

/// Topics: `(escrow, refunded, id)` — Data: (payer, amount)
fn emit_refunded(env: &Env, id: u64, payer: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("escrow"), symbol_short!("refunded"), id),
        (payer.clone(), amount),
    );
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct InvoiceEscrowContract;

#[contractimpl]
impl InvoiceEscrowContract {
    // -----------------------------------------------------------------------
    // Lifecycle
    // -----------------------------------------------------------------------

    /// Initialize the contract. May only be called once.
    ///
    /// # Arguments
    /// * `admin` — Initial contract administrator.
    ///
    /// # Errors
    /// * [`Error::AlreadyInitialized`] if the contract has already been set up.
    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&admin_key()) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&admin_key(), &admin);
        env.storage().instance().set(&counter_key(), &0u64);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Admin authority transfer — two-step handover
    // -----------------------------------------------------------------------

    /// **Step 1 of 2**: Propose a new admin address.
    ///
    /// The current admin authenticates and records `new_admin` as the pending
    /// replacement. No authority is transferred yet — the `new_admin` must call
    /// [`Self::accept_admin`] to complete the handover.
    ///
    /// Calling `transfer_admin` a second time with a different address replaces
    /// any existing pending proposal (useful to correct a typo without having
    /// to cancel first).
    ///
    /// # Arguments
    /// * `new_admin` — The address being proposed as the next admin.
    ///
    /// # Errors
    /// * [`Error::NotAdmin`] if the caller is not the current admin
    ///   (enforced via `require_auth` on the stored admin address).
    /// * [`Error::NotInitialized`] if the contract has not been initialized.
    pub fn transfer_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        if !env.storage().instance().has(&admin_key()) {
            return Err(Error::NotInitialized);
        }
        let current_admin = require_admin(&env);
        env.storage()
            .instance()
            .set(&pending_admin_key(), &new_admin);
        emit_admin_transfer_proposed(&env, &current_admin, &new_admin);
        Ok(())
    }

    /// **Step 2 of 2**: Accept the pending admin proposal.
    ///
    /// The `new_admin` address that was proposed via [`Self::transfer_admin`]
    /// authenticates here, completing the handover. The pending proposal is
    /// cleared and the caller becomes the new admin.
    ///
    /// # Errors
    /// * [`Error::NoPendingAdmin`] if there is no outstanding proposal.
    /// * [`Error::Unauthorized`] if the caller is not the proposed admin
    ///   (enforced via `require_auth` on the pending admin address).
    pub fn accept_admin(env: Env) -> Result<(), Error> {
        let pending: Address = env
            .storage()
            .instance()
            .get(&pending_admin_key())
            .ok_or(Error::NoPendingAdmin)?;
        pending.require_auth();
        env.storage().instance().set(&admin_key(), &pending);
        env.storage().instance().remove(&pending_admin_key());
        emit_admin_transfer_accepted(&env, &pending);
        Ok(())
    }

    /// Cancel a pending admin transfer proposal.
    ///
    /// Only the current admin can cancel. The pending proposal is cleared
    /// without changing the current admin.
    ///
    /// # Errors
    /// * [`Error::NotAdmin`] if the caller is not the current admin.
    /// * [`Error::NoPendingAdmin`] if there is no outstanding proposal to cancel.
    pub fn cancel_transfer(env: Env) -> Result<(), Error> {
        if !env.storage().instance().has(&admin_key()) {
            return Err(Error::NotInitialized);
        }
        let admin = require_admin(&env);
        let pending: Address = env
            .storage()
            .instance()
            .get(&pending_admin_key())
            .ok_or(Error::NoPendingAdmin)?;
        env.storage().instance().remove(&pending_admin_key());
        emit_admin_transfer_cancelled(&env, &admin, &pending);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Admin read-only queries
    // -----------------------------------------------------------------------

    /// Return the current admin address.
    ///
    /// # Errors
    /// * [`Error::NotInitialized`] if the contract has not been initialized.
    pub fn get_admin(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&admin_key())
            .ok_or(Error::NotInitialized)
    }

    /// Return the pending (proposed) admin address, if any.
    ///
    /// Returns `None` when no transfer is in progress.
    pub fn get_pending_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&pending_admin_key())
    }

    // -----------------------------------------------------------------------
    // Escrow — invoice management
    // -----------------------------------------------------------------------

    /// Create a new escrow invoice.
    ///
    /// # Arguments
    /// * `creator`      — Address that creates and owns this invoice.
    /// * `token`        — Token contract (e.g. USDC).
    /// * `total_amount` — Total amount needed to fully fund the invoice.
    /// * `deadline`     — Unix timestamp after which refunds are available.
    ///
    /// Returns the new invoice ID.
    ///
    /// # Errors
    /// * [`Error::NotInitialized`]   — Contract not initialized.
    /// * [`Error::InvalidTotalAmount`] — `total_amount` ≤ 0.
    pub fn create_invoice(
        env: Env,
        creator: Address,
        token: Address,
        total_amount: i128,
        deadline: u64,
    ) -> Result<u64, Error> {
        if !env.storage().instance().has(&admin_key()) {
            return Err(Error::NotInitialized);
        }
        if total_amount <= 0 {
            return Err(Error::InvalidTotalAmount);
        }
        creator.require_auth();

        let id: u64 = env
            .storage()
            .instance()
            .get(&counter_key())
            .unwrap_or(0u64);
        let next_id = id.checked_add(1).expect("invoice counter overflow");
        env.storage().instance().set(&counter_key(), &next_id);

        let invoice = EscrowInvoice {
            creator: creator.clone(),
            token,
            total_amount,
            funded_amount: 0,
            deadline,
            status: EscrowStatus::Pending,
        };
        save_invoice(&env, id, &invoice);
        emit_invoice_created(&env, id, &creator, total_amount, deadline);
        Ok(id)
    }

    /// Deposit funds toward an invoice.
    ///
    /// Transfers `amount` of the invoice token from `payer` to this contract.
    /// If the deposit completes funding the invoice, the funds are immediately
    /// released to the creator (auto-release on full funding).
    ///
    /// # Errors
    /// * [`Error::InvoiceNotFound`]    — Unknown invoice ID.
    /// * [`Error::InvalidStatus`]      — Invoice is not `Pending` or `Active`.
    /// * [`Error::DeadlinePassed`]     — Deposit window has closed.
    /// * [`Error::InvalidAmount`]      — `amount` ≤ 0.
    /// * [`Error::OverFunded`]         — Deposit would exceed `total_amount`.
    pub fn deposit(env: Env, payer: Address, invoice_id: u64, amount: i128) -> Result<(), Error> {
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        payer.require_auth();

        let mut invoice = get_invoice(&env, invoice_id)?;

        if invoice.status != EscrowStatus::Pending && invoice.status != EscrowStatus::Active {
            return Err(Error::InvalidStatus);
        }
        if env.ledger().timestamp() > invoice.deadline {
            return Err(Error::DeadlinePassed);
        }
        let new_funded = invoice
            .funded_amount
            .checked_add(amount)
            .expect("funded_amount overflow");
        if new_funded > invoice.total_amount {
            return Err(Error::OverFunded);
        }

        // Pull funds from payer into the contract.
        let token_client = token::Client::new(&env, &invoice.token);
        token_client.transfer(&payer, &env.current_contract_address(), &amount);

        // Record per-payer contribution.
        let prev: i128 = env
            .storage()
            .persistent()
            .get(&deposit_key(invoice_id, &payer))
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&deposit_key(invoice_id, &payer), &(prev + amount));

        invoice.funded_amount = new_funded;
        invoice.status = EscrowStatus::Active;

        emit_deposited(&env, invoice_id, &payer, amount, new_funded);

        // Auto-release on full funding.
        if new_funded >= invoice.total_amount {
            let total = invoice.total_amount;
            let creator = invoice.creator.clone();
            invoice.status = EscrowStatus::Released;
            save_invoice(&env, invoice_id, &invoice);
            token_client.transfer(&env.current_contract_address(), &creator, &total);
            emit_released(&env, invoice_id, total);
        } else {
            save_invoice(&env, invoice_id, &invoice);
        }

        Ok(())
    }

    /// Release funds to the creator once the invoice is fully funded.
    ///
    /// Anyone may call this after the invoice reaches `total_amount`. Useful
    /// if auto-release did not fire (e.g. due to separate partial deposits).
    ///
    /// # Errors
    /// * [`Error::InvoiceNotFound`]    — Unknown invoice ID.
    /// * [`Error::InvalidStatus`]      — Invoice was already released/refunded/cancelled.
    /// * [`Error::InsufficientFunding`] — Not fully funded yet.
    pub fn release(env: Env, invoice_id: u64) -> Result<(), Error> {
        let mut invoice = get_invoice(&env, invoice_id)?;

        if invoice.status == EscrowStatus::Released
            || invoice.status == EscrowStatus::Refunded
            || invoice.status == EscrowStatus::Cancelled
        {
            return Err(Error::InvalidStatus);
        }
        if invoice.funded_amount < invoice.total_amount {
            return Err(Error::InsufficientFunding);
        }

        let total = invoice.total_amount;
        let creator = invoice.creator.clone();
        invoice.status = EscrowStatus::Released;
        save_invoice(&env, invoice_id, &invoice);

        let token_client = token::Client::new(&env, &invoice.token);
        token_client.transfer(&env.current_contract_address(), &creator, &total);
        emit_released(&env, invoice_id, total);
        Ok(())
    }

    /// Refund all depositors after the deadline passes without full funding.
    ///
    /// Iterates over `payers` and returns each payer's recorded deposit.
    /// Callable by anyone once the deadline has passed and the invoice is not
    /// already released.
    ///
    /// # Arguments
    /// * `invoice_id` — Target invoice.
    /// * `payers`     — List of payer addresses to refund in this call.
    ///
    /// # Errors
    /// * [`Error::InvoiceNotFound`]    — Unknown invoice ID.
    /// * [`Error::InvalidStatus`]      — Invoice already released or refunded.
    /// * [`Error::DeadlineNotPassed`]  — Deadline has not expired yet.
    pub fn refund(env: Env, invoice_id: u64, payers: Vec<Address>) -> Result<(), Error> {
        let mut invoice = get_invoice(&env, invoice_id)?;

        if invoice.status == EscrowStatus::Released
            || invoice.status == EscrowStatus::Refunded
            || invoice.status == EscrowStatus::Cancelled
        {
            return Err(Error::InvalidStatus);
        }
        if env.ledger().timestamp() <= invoice.deadline {
            return Err(Error::DeadlineNotPassed);
        }

        let token_client = token::Client::new(&env, &invoice.token);
        for payer in payers.iter() {
            let amount: i128 = env
                .storage()
                .persistent()
                .get(&deposit_key(invoice_id, &payer))
                .unwrap_or(0);
            if amount > 0 {
                env.storage()
                    .persistent()
                    .remove(&deposit_key(invoice_id, &payer));
                token_client.transfer(&env.current_contract_address(), &payer, &amount);
                emit_refunded(&env, invoice_id, &payer, amount);
            }
        }

        invoice.status = EscrowStatus::Refunded;
        save_invoice(&env, invoice_id, &invoice);
        Ok(())
    }

    /// Cancel an invoice. Only the admin may cancel.
    ///
    /// Transitions the invoice to `Cancelled`. If funds are held they must be
    /// refunded separately via [`Self::refund`] (or admin-driven recovery) since
    /// this call does not transfer tokens, avoiding unbounded iteration over
    /// unknown payers.
    ///
    /// # Errors
    /// * [`Error::NotAdmin`]      — Caller is not the admin.
    /// * [`Error::InvoiceNotFound`] — Unknown invoice ID.
    /// * [`Error::InvalidStatus`]   — Invoice not in a cancellable state.
    pub fn cancel_invoice(env: Env, invoice_id: u64) -> Result<(), Error> {
        if !env.storage().instance().has(&admin_key()) {
            return Err(Error::NotInitialized);
        }
        require_admin(&env);
        let mut invoice = get_invoice(&env, invoice_id)?;
        if invoice.status == EscrowStatus::Released
            || invoice.status == EscrowStatus::Refunded
            || invoice.status == EscrowStatus::Cancelled
        {
            return Err(Error::InvalidStatus);
        }
        invoice.status = EscrowStatus::Cancelled;
        save_invoice(&env, invoice_id, &invoice);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Read-only queries
    // -----------------------------------------------------------------------

    /// Return the full invoice record.
    ///
    /// # Errors
    /// * [`Error::InvoiceNotFound`] — Unknown invoice ID.
    pub fn get_invoice(env: Env, invoice_id: u64) -> Result<EscrowInvoice, Error> {
        get_invoice(&env, invoice_id)
    }

    /// Return the amount a specific payer has deposited toward an invoice.
    pub fn get_deposit(env: Env, invoice_id: u64, payer: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&deposit_key(invoice_id, &payer))
            .unwrap_or(0)
    }
}
