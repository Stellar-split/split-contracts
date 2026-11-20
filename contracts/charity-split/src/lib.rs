//! StellarSplit Charity Split — invoices that donate a share of every payout.
//!
//! The admin maintains a registry of vetted charities. A creator opens an
//! invoice with a recipient split and picks an active charity plus a
//! donation share in basis points. Payers fund the invoice in the invoice
//! token; once fully funded it releases automatically: each recipient gets
//! their gross amount minus the donation share, and the charity receives
//! the sum of the deductions (including rounding dust). Payers can also
//! send optional extra donations straight to the charity.
//!
//! Creators may cancel an open invoice, after which payers reclaim their
//! contributions. Every state change emits an event under the `charity`
//! topic, and per-charity / per-donor totals are tracked on-chain.

#![no_std]

mod errors;
mod types;

pub use errors::Error;
pub use types::{Charity, CharityInvoice, CharityInvoiceStatus, DonationSplit};

use soroban_sdk::{contract, contractimpl, symbol_short, token, Address, Env, Symbol, Vec};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const BPS_DENOMINATOR: i128 = 10_000;
/// Smallest donation share an invoice may configure (0.01%).
pub const MIN_DONATION_BPS: u32 = 1;
/// Largest donation share an invoice may configure (50%).
pub const MAX_DONATION_BPS: u32 = 5_000;
pub const MAX_RECIPIENTS: u32 = 20;

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

fn admin_key() -> Symbol {
    symbol_short!("admin")
}

fn counter_key() -> Symbol {
    symbol_short!("inv_ctr")
}

fn charity_key(charity: &Address) -> (Symbol, Address) {
    (symbol_short!("charity"), charity.clone())
}

fn invoice_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("invoice"), id)
}

fn contribution_key(id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("contrib"), id, payer.clone())
}

fn charity_total_key(charity: &Address, token: &Address) -> (Symbol, Address, Address) {
    (symbol_short!("chr_tot"), charity.clone(), token.clone())
}

fn donor_total_key(donor: &Address, token: &Address) -> (Symbol, Address, Address) {
    (symbol_short!("dnr_tot"), donor.clone(), token.clone())
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

fn emit_charity_registered(env: &Env, charity: &Address, name: &Symbol) {
    env.events().publish(
        (symbol_short!("charity"), symbol_short!("reg"), charity.clone()),
        name.clone(),
    );
}

fn emit_charity_status(env: &Env, charity: &Address, active: bool) {
    env.events().publish(
        (symbol_short!("charity"), symbol_short!("status"), charity.clone()),
        active,
    );
}

fn emit_invoice_created(env: &Env, inv: &CharityInvoice) {
    env.events().publish(
        (symbol_short!("charity"), symbol_short!("created"), inv.id),
        (inv.creator.clone(), inv.total, inv.charity.clone(), inv.donation_bps),
    );
}

fn emit_paid(env: &Env, id: u64, payer: &Address, amount: i128, funded: i128) {
    env.events().publish(
        (symbol_short!("charity"), symbol_short!("paid"), id),
        (payer.clone(), amount, funded),
    );
}

fn emit_released(env: &Env, id: u64, total: i128, donation: i128) {
    env.events().publish(
        (symbol_short!("charity"), symbol_short!("released"), id),
        (total, donation),
    );
}

fn emit_donated(env: &Env, id: u64, charity: &Address, donor: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("charity"), symbol_short!("donated"), id),
        (charity.clone(), donor.clone(), amount),
    );
}

fn emit_cancelled(env: &Env, id: u64, funded: i128) {
    env.events().publish(
        (symbol_short!("charity"), symbol_short!("cancelled"), id),
        funded,
    );
}

fn emit_refunded(env: &Env, id: u64, payer: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("charity"), symbol_short!("refunded"), id),
        (payer.clone(), amount),
    );
}

/// Issue #1066: invoice automatically expired after its deadline passed.
fn emit_expired(env: &Env, id: u64, funded: i128) {
    env.events().publish(
        (symbol_short!("charity"), symbol_short!("expired"), id),
        funded,
    );
}

/// Issue #1066: if `inv` is still `Open` and its deadline (when set) has
/// passed, flip it to `Expired` and persist the change. Returns `true` when
/// the invoice is (now) expired.
fn enforce_deadline(env: &Env, inv: &mut CharityInvoice) -> bool {
    if inv.status == CharityInvoiceStatus::Open
        && inv.deadline != 0
        && env.ledger().timestamp() > inv.deadline
    {
        inv.status = CharityInvoiceStatus::Expired;
        save_invoice(env, inv);
        emit_expired(env, inv.id, inv.funded);
        true
    } else {
        inv.status == CharityInvoiceStatus::Expired
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn require_admin(env: &Env, admin: &Address) -> Result<(), Error> {
    let stored: Address = env
        .storage()
        .instance()
        .get(&admin_key())
        .ok_or(Error::NotInitialized)?;
    if stored != *admin {
        return Err(Error::NotAdmin);
    }
    admin.require_auth();
    Ok(())
}

fn load_charity(env: &Env, charity: &Address) -> Result<Charity, Error> {
    env.storage()
        .persistent()
        .get(&charity_key(charity))
        .ok_or(Error::CharityNotFound)
}

fn load_invoice(env: &Env, id: u64) -> Result<CharityInvoice, Error> {
    env.storage()
        .persistent()
        .get(&invoice_key(id))
        .ok_or(Error::InvoiceNotFound)
}

fn save_invoice(env: &Env, inv: &CharityInvoice) {
    env.storage().persistent().set(&invoice_key(inv.id), inv);
}

fn add_to_total<K>(env: &Env, key: &K, amount: i128) -> Result<(), Error>
where
    K: soroban_sdk::IntoVal<Env, soroban_sdk::Val>,
{
    let current: i128 = env.storage().persistent().get(key).unwrap_or(0);
    let next = current.checked_add(amount).ok_or(Error::Overflow)?;
    env.storage().persistent().set(key, &next);
    Ok(())
}

/// Deducts `donation_bps` from each recipient amount (rounding down) and
/// assigns the remainder of the total to the charity.
fn compute_split(env: &Env, inv: &CharityInvoice) -> Result<DonationSplit, Error> {
    let mut payouts = Vec::new(env);
    let mut paid_out: i128 = 0;
    for amount in inv.amounts.iter() {
        let cut = amount
            .checked_mul(inv.donation_bps as i128)
            .ok_or(Error::Overflow)?
            / BPS_DENOMINATOR;
        let net = amount - cut;
        paid_out = paid_out.checked_add(net).ok_or(Error::Overflow)?;
        payouts.push_back(net);
    }
    Ok(DonationSplit {
        payouts,
        donation: inv.total - paid_out,
    })
}

fn release(env: &Env, inv: &mut CharityInvoice) -> Result<(), Error> {
    let split = compute_split(env, inv)?;
    let client = token::Client::new(env, &inv.token);
    let this = env.current_contract_address();

    for (recipient, payout) in inv.recipients.iter().zip(split.payouts.iter()) {
        if payout > 0 {
            client.transfer(&this, &recipient, &payout);
        }
    }
    if split.donation > 0 {
        client.transfer(&this, &inv.charity, &split.donation);
        add_to_total(env, &charity_total_key(&inv.charity, &inv.token), split.donation)?;
    }

    let mut charity = load_charity(env, &inv.charity)?;
    charity.donation_count = charity.donation_count.saturating_add(1);
    env.storage().persistent().set(&charity_key(&inv.charity), &charity);

    inv.donated = split.donation;
    inv.status = CharityInvoiceStatus::Released;
    emit_released(env, inv.id, inv.total, split.donation);
    emit_donated(env, inv.id, &inv.charity, &inv.creator, split.donation);
    Ok(())
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct CharitySplitContract;

#[contractimpl]
impl CharitySplitContract {
    /// Initialise the contract with an admin who vets charities.
    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&admin_key()) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&admin_key(), &admin);
        env.storage().instance().set(&counter_key(), &0u64);
        Ok(())
    }

    /// Admin-only: add a charity to the registry.
    pub fn register_charity(env: Env, admin: Address, charity: Address, name: Symbol) -> Result<(), Error> {
        require_admin(&env, &admin)?;
        let key = charity_key(&charity);
        if env.storage().persistent().has(&key) {
            return Err(Error::CharityAlreadyRegistered);
        }
        let record = Charity {
            address: charity.clone(),
            name: name.clone(),
            active: true,
            registered_at: env.ledger().timestamp(),
            donation_count: 0,
        };
        env.storage().persistent().set(&key, &record);
        emit_charity_registered(&env, &charity, &name);
        Ok(())
    }

    /// Admin-only: activate or deactivate a charity. Deactivated charities
    /// cannot be chosen for new invoices; existing invoices still release.
    pub fn set_charity_active(env: Env, admin: Address, charity: Address, active: bool) -> Result<(), Error> {
        require_admin(&env, &admin)?;
        let mut record = load_charity(&env, &charity)?;
        record.active = active;
        env.storage().persistent().set(&charity_key(&charity), &record);
        emit_charity_status(&env, &charity, active);
        Ok(())
    }

    /// Create a charity invoice. `amounts` are gross per-recipient amounts
    /// that must sum to `total`; `donation_bps` of each is donated to
    /// `charity` on release. Returns the invoice ID.
    pub fn create_invoice(
        env: Env,
        creator: Address,
        token: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        charity: Address,
        donation_bps: u32,
        deadline: u64,
    ) -> Result<u64, Error> {
        creator.require_auth();
        if deadline != 0 && deadline <= env.ledger().timestamp() {
            return Err(Error::InvalidDeadline);
        }
        if !(MIN_DONATION_BPS..=MAX_DONATION_BPS).contains(&donation_bps) {
            return Err(Error::InvalidDonationBps);
        }
        if recipients.is_empty() || recipients.len() != amounts.len() || recipients.len() > MAX_RECIPIENTS {
            return Err(Error::InvalidRecipients);
        }
        if recipients.contains(&charity) {
            return Err(Error::CharityIsRecipient);
        }
        let record = load_charity(&env, &charity)?;
        if !record.active {
            return Err(Error::CharityInactive);
        }
        let mut total: i128 = 0;
        for amount in amounts.iter() {
            if amount <= 0 {
                return Err(Error::InvalidAmount);
            }
            total = total.checked_add(amount).ok_or(Error::Overflow)?;
        }

        let id: u64 = env
            .storage()
            .instance()
            .get(&counter_key())
            .ok_or(Error::NotInitialized)?;
        let next = id.checked_add(1).ok_or(Error::Overflow)?;
        env.storage().instance().set(&counter_key(), &next);

        let inv = CharityInvoice {
            id,
            creator,
            token,
            total,
            funded: 0,
            recipients,
            amounts,
            charity,
            donation_bps,
            donated: 0,
            status: CharityInvoiceStatus::Open,
            created_at: env.ledger().timestamp(),
            deadline,
        };
        save_invoice(&env, &inv);
        emit_invoice_created(&env, &inv);
        Ok(id)
    }

    /// Pay towards an open invoice. Releases automatically when fully funded.
    /// Returns the funded amount after this payment.
    pub fn pay(env: Env, payer: Address, invoice_id: u64, amount: i128) -> Result<i128, Error> {
        payer.require_auth();
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        let mut inv = load_invoice(&env, invoice_id)?;
        if enforce_deadline(&env, &mut inv) {
            return Err(Error::InvoiceExpired);
        }
        if inv.status != CharityInvoiceStatus::Open {
            return Err(Error::InvoiceNotOpen);
        }
        let funded = inv.funded.checked_add(amount).ok_or(Error::Overflow)?;
        if funded > inv.total {
            return Err(Error::OverFunded);
        }

        token::Client::new(&env, &inv.token).transfer(&payer, &env.current_contract_address(), &amount);
        add_to_total(&env, &contribution_key(invoice_id, &payer), amount)?;
        inv.funded = funded;
        emit_paid(&env, invoice_id, &payer, amount, funded);

        if inv.funded == inv.total {
            release(&env, &mut inv)?;
        }
        save_invoice(&env, &inv);
        Ok(funded)
    }

    /// Send an optional extra donation straight to the invoice's charity.
    pub fn donate_extra(env: Env, donor: Address, invoice_id: u64, amount: i128) -> Result<(), Error> {
        donor.require_auth();
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        let mut inv = load_invoice(&env, invoice_id)?;
        enforce_deadline(&env, &mut inv);
        if inv.status == CharityInvoiceStatus::Cancelled || inv.status == CharityInvoiceStatus::Expired {
            return Err(Error::InvoiceNotOpen);
        }
        token::Client::new(&env, &inv.token).transfer(&donor, &inv.charity, &amount);
        add_to_total(&env, &charity_total_key(&inv.charity, &inv.token), amount)?;
        add_to_total(&env, &donor_total_key(&donor, &inv.token), amount)?;
        emit_donated(&env, invoice_id, &inv.charity, &donor, amount);
        Ok(())
    }

    /// Creator cancels an open invoice; payers can then claim refunds.
    pub fn cancel_invoice(env: Env, creator: Address, invoice_id: u64) -> Result<(), Error> {
        creator.require_auth();
        let mut inv = load_invoice(&env, invoice_id)?;
        if inv.creator != creator {
            return Err(Error::NotCreator);
        }
        if inv.status != CharityInvoiceStatus::Open {
            return Err(Error::InvoiceNotOpen);
        }
        inv.status = CharityInvoiceStatus::Cancelled;
        save_invoice(&env, &inv);
        emit_cancelled(&env, invoice_id, inv.funded);
        Ok(())
    }

    /// Issue #1066: permissionless entry point that anyone may call to flip
    /// an `Open` invoice whose deadline has passed into `Expired`, blocking
    /// further payments. Also called implicitly by `pay`/`donate_extra`.
    pub fn expire_invoice(env: Env, invoice_id: u64) -> Result<(), Error> {
        let mut inv = load_invoice(&env, invoice_id)?;
        if inv.status != CharityInvoiceStatus::Open {
            return Err(Error::InvoiceNotOpen);
        }
        if inv.deadline == 0 || env.ledger().timestamp() <= inv.deadline {
            return Err(Error::InvoiceNotExpired);
        }
        inv.status = CharityInvoiceStatus::Expired;
        save_invoice(&env, &inv);
        emit_expired(&env, invoice_id, inv.funded);
        Ok(())
    }

    /// Payer reclaims their contribution to a cancelled or expired invoice.
    pub fn claim_refund(env: Env, payer: Address, invoice_id: u64) -> Result<i128, Error> {
        payer.require_auth();
        let mut inv = load_invoice(&env, invoice_id)?;
        if inv.status != CharityInvoiceStatus::Cancelled && inv.status != CharityInvoiceStatus::Expired {
            return Err(Error::NotCancelled);
        }
        let key = contribution_key(invoice_id, &payer);
        let amount: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if amount <= 0 {
            return Err(Error::NothingToRefund);
        }
        env.storage().persistent().remove(&key);
        inv.funded -= amount;
        save_invoice(&env, &inv);
        token::Client::new(&env, &inv.token).transfer(&env.current_contract_address(), &payer, &amount);
        emit_refunded(&env, invoice_id, &payer, amount);
        Ok(amount)
    }

    /// Issue #1068: creator issues a partial refund of a payer's contribution
    /// to a still-open invoice (e.g. to correct an overpayment or dispute),
    /// without cancelling the whole invoice. The refunded amount is deducted
    /// from both the payer's recorded contribution and the invoice's funded
    /// total, and transferred back to the payer immediately.
    pub fn partial_refund(
        env: Env,
        creator: Address,
        invoice_id: u64,
        payer: Address,
        amount: i128,
    ) -> Result<i128, Error> {
        creator.require_auth();
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        let mut inv = load_invoice(&env, invoice_id)?;
        if inv.creator != creator {
            return Err(Error::NotCreator);
        }
        if inv.status == CharityInvoiceStatus::Cancelled || inv.status == CharityInvoiceStatus::Expired {
            // Full refunds already go through claim_refund for these states.
            return Err(Error::InvoiceNotOpen);
        }

        let key = contribution_key(invoice_id, &payer);
        let contributed: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if amount > contributed {
            return Err(Error::ExceedsContribution);
        }

        let remaining = contributed - amount;
        if remaining > 0 {
            env.storage().persistent().set(&key, &remaining);
        } else {
            env.storage().persistent().remove(&key);
        }
        inv.funded = inv.funded.checked_sub(amount).ok_or(Error::Overflow)?;
        save_invoice(&env, &inv);

        token::Client::new(&env, &inv.token).transfer(&env.current_contract_address(), &payer, &amount);
        emit_refunded(&env, invoice_id, &payer, amount);
        Ok(amount)
    }

    // -----------------------------------------------------------------------
    // Views
    // -----------------------------------------------------------------------

    pub fn get_charity(env: Env, charity: Address) -> Option<Charity> {
        env.storage().persistent().get(&charity_key(&charity))
    }

    pub fn get_invoice(env: Env, invoice_id: u64) -> Option<CharityInvoice> {
        env.storage().persistent().get(&invoice_key(invoice_id))
    }

    /// Preview the net payouts and donation an invoice will produce on release.
    pub fn preview_split(env: Env, invoice_id: u64) -> Result<DonationSplit, Error> {
        let inv = load_invoice(&env, invoice_id)?;
        compute_split(&env, &inv)
    }

    pub fn get_contribution(env: Env, invoice_id: u64, payer: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&contribution_key(invoice_id, &payer))
            .unwrap_or(0)
    }

    /// Total donated to `charity` in `token` (split donations plus extras).
    pub fn get_charity_total(env: Env, charity: Address, token: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&charity_total_key(&charity, &token))
            .unwrap_or(0)
    }

    /// Total extra donations made by `donor` in `token`.
    pub fn get_donor_total(env: Env, donor: Address, token: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&donor_total_key(&donor, &token))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod test;
