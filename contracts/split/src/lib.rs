//! StellarSplit — on-chain invoice & payment splitting contract.

#![no_std]

mod events;
mod types;

#[cfg(test)]
mod test;

use soroban_sdk::{
    contract, contractimpl, symbol_short, token, Address, Bytes, BytesN, Env, Map, Symbol, Vec,
};
use types::{
    AuditEntry, CompletionProof, CreateInvoiceParams, Invoice, InvoiceOptions, InvoiceStatus,
    InvoiceTemplate, Payment, SubscriptionParams, Tranche,
};

// ---------------------------------------------------------------------------
// Storage key helpers
// ---------------------------------------------------------------------------

fn admin_key() -> Symbol {
    symbol_short!("admin")
}
fn paused_key() -> Symbol {
    symbol_short!("paused")
}
fn fee_bps_key() -> Symbol {
    symbol_short!("fee_bps")
}
fn counter_key() -> Symbol {
    symbol_short!("counter")
}
fn invoice_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("inv"), id)
}
fn audit_log_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("log"), id)
}
fn subscription_params_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("sub"), id)
}
fn ext_vote_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("ext_vote"), id)
}
fn group_key(group_id: u64) -> (Symbol, u64) {
    (symbol_short!("grp"), group_id)
}
fn invoice_group_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("invgrp"), invoice_id)
}
fn template_key(creator: &Address, name: &Symbol) -> (Symbol, Address, Symbol) {
    (symbol_short!("tmpl"), creator.clone(), name.clone())
}

/// Per-address reputation counter key (issue #24).
fn rep_key(payer: &Address) -> (Symbol, Address) {
    (symbol_short!("rep"), payer.clone())
}

/// Per-payer per-invoice nonce key (issue #21).
fn nonce_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("nonce"), invoice_id, payer.clone())
}

// ---------------------------------------------------------------------------
// Invoice storage helpers
// ---------------------------------------------------------------------------

fn load_invoice(env: &Env, id: u64) -> Invoice {
    env.storage()
        .persistent()
        .get(&invoice_key(id))
        .expect("invoice not found")
}

fn save_invoice(env: &Env, id: u64, invoice: &Invoice) {
    env.storage().persistent().set(&invoice_key(id), invoice);
}

fn append_audit_entry(env: &Env, id: u64, action: Symbol, actor: &Address) {
    let timestamp = env.ledger().timestamp();
    let entry = AuditEntry { action, actor: actor.clone(), timestamp };
    let mut log: Vec<AuditEntry> = env
        .storage()
        .persistent()
        .get(&audit_log_key(id))
        .unwrap_or_else(|| Vec::new(env));
    log.push_back(entry);
    env.storage().persistent().set(&audit_log_key(id), &log);
}

pub fn get_audit_log(env: &Env, id: u64) -> Vec<AuditEntry> {
    env.storage()
        .persistent()
        .get(&audit_log_key(id))
        .unwrap_or_else(|| Vec::new(env))
}

// ---------------------------------------------------------------------------
// Admin / pause helpers
// ---------------------------------------------------------------------------

fn is_paused(env: &Env) -> bool {
    env.storage()
        .persistent()
        .get(&paused_key())
        .unwrap_or(false)
}

fn require_not_paused(env: &Env) {
    assert!(!is_paused(env), "contract is paused");
}

fn require_admin(env: &Env, caller: &Address) {
    let admin: Address = env
        .storage()
        .instance()
        .get(&admin_key())
        .expect("admin not set");
    assert!(admin == *caller, "caller is not admin");
    caller.require_auth();
}

// ---------------------------------------------------------------------------
// Group helpers
// ---------------------------------------------------------------------------

fn load_group(env: &Env, group_id: u64) -> Vec<u64> {
    env.storage()
        .persistent()
        .get(&group_key(group_id))
        .expect("group not found")
}

fn group_all_funded(env: &Env, group_id: u64) -> bool {
    for id in load_group(env, group_id).iter() {
        let inv = load_invoice(env, id);
        let total: i128 = inv.amounts.iter().sum();
        if inv.funded < total {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct SplitContract;

#[contractimpl]
impl SplitContract {
    /// Set the contract admin. Can only be called once.
    pub fn initialize(env: Env, admin: Address) {
        assert!(
            !env.storage().instance().has(&admin_key()),
            "already initialized"
        );
        env.storage().instance().set(&admin_key(), &admin);
        env.storage().persistent().set(&paused_key(), &false);
    }

    /// Upgrade the contract WASM. Requires admin auth.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&admin_key())
            .expect("not initialized");
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
    }

    /// Pause all mutating operations. Requires admin auth.
    pub fn pause(env: Env, admin: Address) {
        require_admin(&env, &admin);
        env.storage().persistent().set(&paused_key(), &true);
    }

    /// Unpause the contract. Requires admin auth.
    pub fn unpause(env: Env, admin: Address) {
        require_admin(&env, &admin);
        env.storage().persistent().set(&paused_key(), &false);
    }

    // -----------------------------------------------------------------------
    // Invoice creation
    // -----------------------------------------------------------------------

    /// Create a new invoice.
    ///
    /// * `token`   – token contract address (same for all recipients)
    /// * `options` – optional fields: co_creators, allow_early_withdrawal, bonus_pool,
    ///               bonus_max_payers, prerequisite_id (#22), tranches (#23)
    pub fn create_invoice(
        env: Env,
        creator: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        token: Address,
        deadline: u64,
        options: InvoiceOptions,
    ) -> u64 {
        require_not_paused(&env);
        creator.require_auth();
        Self::_create_invoice_inner(
            &env,
            creator,
            recipients,
            amounts,
            token,
            deadline,
            options.co_creators,
            options.allow_early_withdrawal,
            options.bonus_pool,
            options.bonus_max_payers,
            options.prerequisite_id,
            options.tranches,
        )
    }

    fn _create_invoice_inner(
        env: &Env,
        creator: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        token: Address,
        deadline: u64,
        co_creators: Vec<Address>,
        allow_early_withdrawal: bool,
        bonus_pool: i128,
        bonus_max_payers: u32,
        prerequisite_id: Option<u64>,
        tranches: Vec<Tranche>,
    ) -> u64 {
        assert!(
            recipients.len() == amounts.len(),
            "recipients and amounts length mismatch"
        );
        assert!(!recipients.is_empty(), "must have at least one recipient");
        assert!(deadline > env.ledger().timestamp(), "deadline must be in the future");
        assert!(bonus_pool >= 0, "bonus_pool must be non-negative");

        for amt in amounts.iter() {
            assert!(amt > 0, "amounts must be positive");
        }

        if let Some(prereq_id) = prerequisite_id {
            let _ = load_invoice(env, prereq_id);
        }

        if !tranches.is_empty() {
            let total_bps: u32 = tranches.iter().map(|t| t.basis_points).sum();
            assert!(total_bps == 10_000, "tranches must sum to 10000 basis points");
        }

        let id: u64 = env
            .storage()
            .persistent()
            .get(&counter_key())
            .unwrap_or(0u64)
            + 1;
        env.storage().persistent().set(&counter_key(), &id);

        let total: i128 = amounts.iter().sum();

        if bonus_pool > 0 {
            let token_client = token::Client::new(env, &token);
            token_client.transfer(&creator, &env.current_contract_address(), &bonus_pool);
        }

        // Build per-recipient token vec (all the same token).
        let mut tokens: Vec<Address> = Vec::new(env);
        for _ in recipients.iter() {
            tokens.push_back(token.clone());
        }

        // Initialize per-recipient claimed vec to 0.
        let mut claimed: Vec<i128> = Vec::new(env);
        for _ in recipients.iter() {
            claimed.push_back(0i128);
        }

        let invoice = Invoice {
            creator: creator.clone(),
            co_creators,
            recipients: recipients.clone(),
            amounts,
            tokens,
            deadline,
            funded: 0,
            status: InvoiceStatus::Pending,
            payments: Vec::new(env),
            drip_duration: None,
            release_timestamp: None,
            claimed,
            frozen: false,
            completion_time: None,
            allow_early_withdrawal,
            bonus_pool,
            bonus_max_payers,
            prerequisite_id,
            tranches,
            released_bps: 0,
        };

        save_invoice(env, id, &invoice);
        events::invoice_created(env, id, &creator, total, &None);

        id
    }

    /// Create up to 5 invoices in a single transaction.
    pub fn create_batch(
        env: Env,
        creator: Address,
        invoices: Vec<CreateInvoiceParams>,
    ) -> Vec<u64> {
        creator.require_auth();
        assert!(invoices.len() <= 5, "batch limit exceeded");

        let mut ids: Vec<u64> = Vec::new(&env);
        for params in invoices.iter() {
            let id = Self::_create_invoice_inner(
                &env,
                creator.clone(),
                params.recipients,
                params.amounts,
                params.token,
                params.deadline,
                Vec::new(&env),
                false,
                0,
                0,
                None,
                Vec::new(&env),
            );
            ids.push_back(id);
        }
        ids
    }

    /// Create a subscription chain of invoices for recurring monthly billing.
    pub fn create_subscription(
        env: Env,
        creator: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        token: Address,
        months: u32,
    ) -> u64 {
        creator.require_auth();

        assert!(
            recipients.len() == amounts.len(),
            "recipients and amounts length mismatch"
        );
        assert!(!recipients.is_empty(), "must have at least one recipient");
        assert!(months > 0 && months <= 12, "months must be between 1 and 12");
        for amt in amounts.iter() {
            assert!(amt > 0, "amounts must be positive");
        }

        let deadline = env.ledger().timestamp() + 30 * 24 * 60 * 60;
        let id = Self::_create_invoice_inner(
            &env,
            creator.clone(),
            recipients.clone(),
            amounts.clone(),
            token.clone(),
            deadline,
            Vec::new(&env),
            false,
            0,
            0,
            None,
            Vec::new(&env),
        );

        if months > 1 {
            // Build tokens vec for subscription params storage.
            let mut tokens_vec: Vec<Address> = Vec::new(&env);
            for _ in recipients.iter() {
                tokens_vec.push_back(token.clone());
            }
            let params = SubscriptionParams {
                creator,
                recipients,
                amounts,
                tokens: tokens_vec,
            };
            env.storage()
                .persistent()
                .set(&subscription_params_key(id), &params);
        }

        id
    }

    // -----------------------------------------------------------------------
    // Payment (#21 nonce added)
    // -----------------------------------------------------------------------

    /// Pay toward an invoice.
    ///
    /// `nonce` must equal the current expected nonce for this (invoice_id, payer)
    /// pair — starts at 0 and increments with each successful payment.
    pub fn pay(env: Env, payer: Address, invoice_id: u64, amount: i128, nonce: u64) {
        require_not_paused(&env);
        payer.require_auth();
        Self::_pay(&env, &payer, invoice_id, amount, nonce);
    }

    fn _pay(env: &Env, payer: &Address, invoice_id: u64, amount: i128, nonce: u64) {
        let mut invoice = load_invoice(env, invoice_id);

        assert!(!invoice.frozen, "invoice is frozen");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(
            env.ledger().timestamp() <= invoice.deadline,
            "invoice deadline has passed"
        );
        assert!(amount > 0, "payment amount must be positive");

        let total: i128 = invoice.amounts.iter().sum();
        let remaining = total - invoice.funded;
        assert!(amount <= remaining, "payment exceeds remaining balance");

        // Validate and increment per-payer per-invoice nonce (issue #21).
        let stored_nonce: u64 = env
            .storage()
            .persistent()
            .get(&nonce_key(invoice_id, payer))
            .unwrap_or(0u64);
        assert!(nonce == stored_nonce, "invalid nonce");
        env.storage()
            .persistent()
            .set(&nonce_key(invoice_id, payer), &(stored_nonce + 1));

        let token_client = token::Client::new(env, &invoice.tokens.get(0).expect("no token"));
        token_client.transfer(payer, &env.current_contract_address(), &amount);

        invoice.payments.push_back(Payment { payer: payer.clone(), amount, tip: 0 });
        invoice.funded += amount;

        // Increment per-address reputation counter (issue #24).
        let rep: u64 = env
            .storage()
            .persistent()
            .get(&rep_key(payer))
            .unwrap_or(0u64);
        env.storage()
            .persistent()
            .set(&rep_key(payer), &(rep + 1));

        append_audit_entry(env, invoice_id, symbol_short!("pay"), payer);
        events::payment_received(env, invoice_id, payer, amount);

        if invoice.funded >= total {
            // Auto-release only when no tranches, prerequisite, or group constraint
            // requires a manual release() call.
            let in_group = env
                .storage()
                .persistent()
                .has(&invoice_group_key(invoice_id));
            let guarded =
                invoice.prerequisite_id.is_some() || !invoice.tranches.is_empty() || in_group;
            if guarded {
                save_invoice(env, invoice_id, &invoice);
            } else {
                Self::_release(env, invoice_id, &mut invoice, payer);
            }
        } else {
            save_invoice(env, invoice_id, &invoice);
        }
    }

    // -----------------------------------------------------------------------
    // Release (#22 prerequisite, #23 tranches)
    // -----------------------------------------------------------------------

    /// Release funds to recipients.
    ///
    /// For tranche invoices, only distributes tranches whose timestamp ≤ now.
    /// Blocks with "prerequisite not released" until the prerequisite invoice is Released.
    pub fn release(env: Env, invoice_id: u64) {
        require_not_paused(&env);
        let caller = env.current_contract_address();
        let mut invoice = load_invoice(&env, invoice_id);

        assert!(!invoice.frozen, "invoice is frozen");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        let total: i128 = invoice.amounts.iter().sum();
        assert!(invoice.funded >= total, "invoice not fully funded");

        // Prerequisite check (issue #22).
        if let Some(prereq_id) = invoice.prerequisite_id {
            let prereq = load_invoice(&env, prereq_id);
            assert!(
                prereq.status == InvoiceStatus::Released,
                "prerequisite not released"
            );
        }

        // Group constraint: all members must be fully funded before any can release.
        if let Some(group_id) = env
            .storage()
            .persistent()
            .get::<(Symbol, u64), u64>(&invoice_group_key(invoice_id))
        {
            assert!(group_all_funded(&env, group_id), "group members not fully funded");
        }

        Self::_release(&env, invoice_id, &mut invoice, &caller);
    }

    fn _release(env: &Env, invoice_id: u64, invoice: &mut Invoice, actor: &Address) {
        if invoice.tranches.is_empty() {
            Self::_release_full(env, invoice_id, invoice, actor);
        } else {
            Self::_release_tranches(env, invoice_id, invoice, actor);
        }
    }

    /// Distribute tranches unlocked by the current ledger time (issue #23).
    fn _release_tranches(env: &Env, invoice_id: u64, invoice: &mut Invoice, actor: &Address) {
        let now = env.ledger().timestamp();

        // Sum all basis points whose timestamp has passed.
        let mut unlocked_bps: u32 = 0;
        for tranche in invoice.tranches.iter() {
            if tranche.timestamp <= now {
                unlocked_bps += tranche.basis_points;
            }
        }

        // New basis points not yet distributed.
        let new_bps = unlocked_bps.saturating_sub(invoice.released_bps);
        assert!(new_bps > 0, "no tranches unlocked");

        let token_client =
            token::Client::new(env, &invoice.tokens.get(0).expect("no token"));

        for i in 0..invoice.recipients.len() {
            let recipient = invoice.recipients.get(i).unwrap();
            let amount = invoice.amounts.get(i).unwrap();
            // integer math: avoid overflow via u128 intermediary.
            let payout = (amount as u128)
                .saturating_mul(new_bps as u128)
                / 10_000u128;
            let payout = payout as i128;
            if payout > 0 {
                token_client.transfer(&env.current_contract_address(), &recipient, &payout);
            }
        }

        invoice.released_bps += new_bps;

        if invoice.released_bps >= 10_000 {
            invoice.status = InvoiceStatus::Released;
            invoice.completion_time = Some(now);
            append_audit_entry(env, invoice_id, symbol_short!("release"), actor);
            events::invoice_released(env, invoice_id, &invoice.recipients);
            let total: i128 = invoice.amounts.iter().sum();
            events::invoice_completed(
                env,
                invoice_id,
                &invoice.creator,
                total,
                invoice.recipients.len() as u32,
                now,
            );
        }

        save_invoice(env, invoice_id, invoice);
    }

    /// Full immediate release (no tranches).
    fn _release_full(env: &Env, invoice_id: u64, invoice: &mut Invoice, actor: &Address) {
        let token_client =
            token::Client::new(env, &invoice.tokens.get(0).expect("no token"));

        let fee_bps: u32 = env
            .storage()
            .persistent()
            .get(&fee_bps_key())
            .unwrap_or(0u32);

        for i in 0..invoice.recipients.len() {
            let recipient = invoice.recipients.get(i).unwrap();
            let amount = invoice.amounts.get(i).unwrap();
            let payout = if fee_bps > 0 {
                amount
                    - (amount as u128 * fee_bps as u128 / 10_000u128) as i128
            } else {
                amount
            };
            token_client.transfer(&env.current_contract_address(), &recipient, &payout);
        }

        // Distribute bonus pool among first `bonus_max_payers` unique payers.
        if invoice.bonus_pool > 0 && invoice.bonus_max_payers > 0 {
            let mut unique_payers: Vec<Address> = Vec::new(env);
            for payment in invoice.payments.iter() {
                let already_seen = unique_payers.iter().any(|p| p == payment.payer);
                if !already_seen {
                    unique_payers.push_back(payment.payer.clone());
                    if unique_payers.len() >= invoice.bonus_max_payers {
                        break;
                    }
                }
            }

            if !unique_payers.is_empty() {
                let n = unique_payers.len() as i128;
                let per_payer = invoice.bonus_pool / n;
                let mut distributed: i128 = 0;
                for (i, payer) in unique_payers.iter().enumerate() {
                    let payout = if i as i128 == n - 1 {
                        invoice.bonus_pool - distributed
                    } else {
                        per_payer
                    };
                    token_client.transfer(&env.current_contract_address(), &payer, &payout);
                    distributed += payout;
                }
            }
        }

        // Release all group members if this invoice is part of a group.
        if let Some(group_id) = env
            .storage()
            .persistent()
            .get::<(Symbol, u64), u64>(&invoice_group_key(invoice_id))
        {
            for member_id in load_group(env, group_id).iter() {
                if member_id != invoice_id {
                    let mut member = load_invoice(env, member_id);
                    if member.status == InvoiceStatus::Pending {
                        let member_token =
                            token::Client::new(env, &member.tokens.get(0).expect("no token"));
                        for (recipient, amount) in
                            member.recipients.iter().zip(member.amounts.iter())
                        {
                            member_token.transfer(
                                &env.current_contract_address(),
                                &recipient,
                                &amount,
                            );
                        }
                        member.status = InvoiceStatus::Released;
                        member.completion_time = Some(env.ledger().timestamp());
                        save_invoice(env, member_id, &member);
                        append_audit_entry(env, member_id, symbol_short!("release"), actor);
                        events::invoice_released(env, member_id, &member.recipients);
                        let member_total: i128 = member.amounts.iter().sum();
                        events::invoice_completed(
                            env,
                            member_id,
                            &member.creator,
                            member_total,
                            member.recipients.len() as u32,
                            env.ledger().timestamp(),
                        );
                    }
                }
            }
        }

        invoice.status = InvoiceStatus::Released;
        invoice.completion_time = Some(env.ledger().timestamp());
        save_invoice(env, invoice_id, invoice);
        append_audit_entry(env, invoice_id, symbol_short!("release"), actor);
        events::invoice_released(env, invoice_id, &invoice.recipients);
        let total: i128 = invoice.amounts.iter().sum();
        events::invoice_completed(
            env,
            invoice_id,
            &invoice.creator,
            total,
            invoice.recipients.len() as u32,
            env.ledger().timestamp(),
        );

        // Spin up next subscription invoice if one is scheduled.
        if let Some(params) = env
            .storage()
            .persistent()
            .get::<(Symbol, u64), SubscriptionParams>(&subscription_params_key(invoice_id))
        {
            let next_deadline = env.ledger().timestamp() + 30 * 24 * 60 * 60;
            let first_token = params.tokens.get(0).expect("no token in subscription");
            let _next_id = Self::_create_invoice_inner(
                env,
                params.creator.clone(),
                params.recipients.clone(),
                params.amounts.clone(),
                first_token,
                next_deadline,
                Vec::new(env),
                false,
                0,
                0,
                None,
                Vec::new(env),
            );
            env.storage()
                .persistent()
                .remove(&subscription_params_key(invoice_id));
        }
    }

    // -----------------------------------------------------------------------
    // Refund / cancel / transfer / deadline
    // -----------------------------------------------------------------------

    /// Refund all payers if the deadline has passed and the invoice is not fully funded.
    pub fn refund(env: Env, invoice_id: u64) {
        require_not_paused(&env);
        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(
            env.ledger().timestamp() > invoice.deadline,
            "deadline has not passed"
        );

        let token_client =
            token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));

        let mut totals: Map<Address, i128> = Map::new(&env);
        for payment in invoice.payments.iter() {
            let prev = totals.get(payment.payer.clone()).unwrap_or(0);
            totals.set(payment.payer.clone(), prev + payment.amount);
        }

        for (payer, amount) in totals.iter() {
            token_client.transfer(&env.current_contract_address(), &payer, &amount);
            events::payer_refunded(&env, invoice_id, &payer, amount);
        }

        if invoice.bonus_pool > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &invoice.creator,
                &invoice.bonus_pool,
            );
        }

        invoice.status = InvoiceStatus::Refunded;
        invoice.completion_time = Some(env.ledger().timestamp());
        save_invoice(&env, invoice_id, &invoice);
        let actor = env.current_contract_address();
        append_audit_entry(&env, invoice_id, symbol_short!("refund"), &actor);
        events::invoice_refunded(&env, invoice_id);
    }

    /// Cancel an invoice. Refunds any payments already made.
    pub fn cancel_invoice(env: Env, caller: Address, invoice_id: u64) {
        require_not_paused(&env);
        caller.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(invoice.creator == caller, "only creator can cancel");

        if invoice.funded > 0 {
            // Refund all payments.
            let token_client =
                token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));

            let mut totals: Map<Address, i128> = Map::new(&env);
            for payment in invoice.payments.iter() {
                let prev = totals.get(payment.payer.clone()).unwrap_or(0);
                totals.set(payment.payer.clone(), prev + payment.amount);
            }
            for (payer, amount) in totals.iter() {
                token_client.transfer(&env.current_contract_address(), &payer, &amount);
            }

            if invoice.bonus_pool > 0 {
                token_client.transfer(
                    &env.current_contract_address(),
                    &invoice.creator,
                    &invoice.bonus_pool,
                );
            }

            invoice.status = InvoiceStatus::Refunded;
        } else {
            if invoice.bonus_pool > 0 {
                let token_client =
                    token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
                token_client.transfer(
                    &env.current_contract_address(),
                    &invoice.creator,
                    &invoice.bonus_pool,
                );
            }
            invoice.status = InvoiceStatus::Cancelled;
        }

        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("cancel"), &caller);
    }

    /// Transfer invoice ownership to a new creator.
    pub fn transfer_invoice(env: Env, invoice_id: u64, new_creator: Address) {
        require_not_paused(&env);
        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        invoice.creator.require_auth();
        invoice.creator = new_creator;
        save_invoice(&env, invoice_id, &invoice);
    }

    /// Append a new recipient with `amount` to an invoice.
    ///
    /// Only the creator can call this, and only while no payment has been received
    /// (`funded == 0`) and the invoice is still `Pending`.
    pub fn add_recipient(
        env: Env,
        caller: Address,
        invoice_id: u64,
        new_recipient: Address,
        amount: i128,
    ) {
        require_not_paused(&env);
        caller.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(invoice.creator == caller, "only creator can add recipients");
        assert!(invoice.funded == 0, "cannot add recipient after payment received");
        assert!(amount > 0, "amount must be positive");

        let token = invoice.tokens.get(0).expect("no token");
        invoice.recipients.push_back(new_recipient.clone());
        invoice.amounts.push_back(amount);
        invoice.tokens.push_back(token);
        invoice.claimed.push_back(0i128);

        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("add_rec"), &caller);
        events::recipient_added(&env, invoice_id, &caller, &new_recipient, amount);
    }

    /// Extend the deadline for an invoice (creator only).
    pub fn extend_deadline(env: Env, caller: Address, invoice_id: u64, new_deadline: u64) {
        require_not_paused(&env);
        caller.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(invoice.creator == caller, "only creator can extend deadline");
        assert!(
            new_deadline > env.ledger().timestamp(),
            "new deadline must be in the future"
        );

        invoice.deadline = new_deadline;
        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("extend"), &caller);
    }

    // -----------------------------------------------------------------------
    // Templates
    // -----------------------------------------------------------------------

    /// Save a reusable invoice template.
    pub fn save_template(
        env: Env,
        creator: Address,
        name: Symbol,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        token: Address,
    ) {
        creator.require_auth();
        assert!(
            recipients.len() == amounts.len(),
            "recipients and amounts length mismatch"
        );
        assert!(!recipients.is_empty(), "must have at least one recipient");
        for amt in amounts.iter() {
            assert!(amt > 0, "amounts must be positive");
        }
        let template = InvoiceTemplate { recipients, amounts, token };
        env.storage()
            .persistent()
            .set(&template_key(&creator, &name), &template);
    }

    /// Create a new invoice from a previously saved template.
    pub fn create_from_template(
        env: Env,
        creator: Address,
        name: Symbol,
        deadline: u64,
    ) -> u64 {
        creator.require_auth();
        let tmpl: InvoiceTemplate = env
            .storage()
            .persistent()
            .get(&template_key(&creator, &name))
            .expect("template not found");
        Self::_create_invoice_inner(
            &env,
            creator,
            tmpl.recipients,
            tmpl.amounts,
            tmpl.token,
            deadline,
            Vec::new(&env),
            false,
            0,
            0,
            None,
            Vec::new(&env),
        )
    }

    // -----------------------------------------------------------------------
    // Group
    // -----------------------------------------------------------------------

    /// Link invoices into a group: all must be fully funded before any can be released.
    pub fn create_invoice_group(env: Env, invoice_ids: Vec<u64>) -> u64 {
        assert!(invoice_ids.len() >= 2, "group needs at least 2 invoices");

        let grp_cnt_key = symbol_short!("grp_cnt");
        let group_id: u64 = env
            .storage()
            .persistent()
            .get(&grp_cnt_key)
            .unwrap_or(0u64)
            + 1;
        env.storage().persistent().set(&grp_cnt_key, &group_id);

        for id in invoice_ids.iter() {
            env.storage()
                .persistent()
                .set(&invoice_group_key(id), &group_id);
        }
        env.storage()
            .persistent()
            .set(&group_key(group_id), &invoice_ids);

        group_id
    }

    // -----------------------------------------------------------------------
    // Early withdrawal (#37)
    // -----------------------------------------------------------------------

    /// Allows a payer to reclaim their contribution before the deadline when
    /// `allow_early_withdrawal` is enabled on the invoice.
    pub fn withdraw(env: Env, invoice_id: u64, payer: Address) {
        payer.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(invoice.allow_early_withdrawal, "early withdrawal not allowed");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        let mut total_paid: i128 = 0;
        for payment in invoice.payments.iter() {
            if payment.payer == payer {
                total_paid += payment.amount;
            }
        }
        assert!(total_paid > 0, "no contributions to withdraw");

        let mut new_payments: Vec<Payment> = Vec::new(&env);
        for payment in invoice.payments.iter() {
            if payment.payer != payer {
                new_payments.push_back(payment);
            }
        }
        invoice.payments = new_payments;
        invoice.funded -= total_paid;

        let token_client =
            token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
        token_client.transfer(&env.current_contract_address(), &payer, &total_paid);

        save_invoice(&env, invoice_id, &invoice);
    }

    // -----------------------------------------------------------------------
    // Deadline extension by payer vote (#39)
    // -----------------------------------------------------------------------

    /// Vote to extend the invoice deadline by 7 days.
    /// Once a strict majority of unique payers vote, the deadline is extended.
    pub fn vote_extend_deadline(env: Env, invoice_id: u64, voter: Address) {
        voter.require_auth();

        let invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        let has_paid = invoice.payments.iter().any(|p| p.payer == voter);
        assert!(has_paid, "only payers can vote");

        let mut unique_payers: Vec<Address> = Vec::new(&env);
        for payment in invoice.payments.iter() {
            if !unique_payers.contains(&payment.payer) {
                unique_payers.push_back(payment.payer);
            }
        }

        let vote_key = ext_vote_key(invoice_id);
        let mut votes: Vec<Address> = env
            .storage()
            .persistent()
            .get(&vote_key)
            .unwrap_or_else(|| Vec::new(&env));

        if votes.contains(&voter) {
            return;
        }
        votes.push_back(voter);

        if votes.len() > unique_payers.len() / 2 {
            let mut invoice = load_invoice(&env, invoice_id);
            invoice.deadline += 7 * 24 * 60 * 60;
            save_invoice(&env, invoice_id, &invoice);
            env.storage().persistent().remove(&vote_key);
        } else {
            env.storage().persistent().set(&vote_key, &votes);
        }
    }

    // -----------------------------------------------------------------------
    // Drip / vesting claim
    // -----------------------------------------------------------------------

    /// Claim the vested portion of a drip invoice for a recipient.
    pub fn drip_claim(env: Env, invoice_id: u64, recipient: Address) {
        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Released,
            "invoice not released"
        );
        let drip_duration = invoice.drip_duration.expect("no drip schedule");
        let release_ts = invoice.release_timestamp.expect("no release timestamp");

        let idx = invoice
            .recipients
            .iter()
            .position(|r| r == recipient)
            .expect("recipient not found") as u32;

        let total_amount = invoice.amounts.get(idx).unwrap();
        let already_claimed = invoice.claimed.get(idx).unwrap();

        let elapsed = env.ledger().timestamp().saturating_sub(release_ts);
        let vested = if elapsed >= drip_duration {
            total_amount
        } else {
            (elapsed as i128) * total_amount / (drip_duration as i128)
        };

        let claimable = vested - already_claimed;
        assert!(claimable > 0, "nothing to claim");

        invoice.claimed.set(idx, already_claimed + claimable);
        save_invoice(&env, invoice_id, &invoice);

        let token_client =
            token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
        token_client.transfer(&env.current_contract_address(), &recipient, &claimable);
    }

    // -----------------------------------------------------------------------
    // Read-only
    // -----------------------------------------------------------------------

    pub fn get_invoice(env: Env, invoice_id: u64) -> Invoice {
        load_invoice(&env, invoice_id)
    }

    pub fn get_audit_log(env: Env, invoice_id: u64) -> Vec<AuditEntry> {
        get_audit_log(&env, invoice_id)
    }

    /// Return the total amount contributed by `payer` toward `invoice_id`.
    pub fn get_payer_total(env: Env, invoice_id: u64, payer: Address) -> i128 {
        let invoice = load_invoice(&env, invoice_id);
        invoice
            .payments
            .iter()
            .filter(|p| p.payer == payer)
            .map(|p| p.amount)
            .sum()
    }

    /// Returns the on-chain reputation score (number of successful payments) for an address.
    ///
    /// Returns 0 for an address that has never paid.
    pub fn get_reputation(env: Env, address: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&rep_key(&address))
            .unwrap_or(0u64)
    }

    /// Returns the current expected nonce for a (invoice_id, payer) pair.
    ///
    /// The first payment must use nonce 0; each successful payment increments it by 1.
    /// Returns 0 for a payer that has never paid toward this invoice.
    pub fn get_nonce(env: Env, invoice_id: u64, payer: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&nonce_key(invoice_id, &payer))
            .unwrap_or(0u64)
    }

    /// Generate a completion proof for a finalized invoice.
    pub fn get_completion_proof(env: Env, invoice_id: u64) -> CompletionProof {
        let invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Released
                || invoice.status == InvoiceStatus::Refunded,
            "invoice not finalized"
        );

        let status_byte: u8 = match invoice.status {
            InvoiceStatus::Pending => 0u8,
            InvoiceStatus::Released => 1u8,
            InvoiceStatus::Refunded => 2u8,
            InvoiceStatus::Cancelled => 3u8,
        };

        let mut preimage = [0u8; 17];
        preimage[..8].copy_from_slice(&invoice_id.to_be_bytes());
        preimage[8..16].copy_from_slice(&(invoice.funded as u64).to_be_bytes());
        preimage[16] = status_byte;

        let bytes = Bytes::from_array(&env, &preimage);
        let hash = env.crypto().sha256(&bytes);

        CompletionProof {
            id: invoice_id,
            status: invoice.status,
            funded: invoice.funded,
            timestamp: env.ledger().timestamp(),
            hash: hash.into(),
        }
    }

    /// Returns true if the invoice exists and its status matches `expected_status`.
    pub fn verify_invoice(env: Env, invoice_id: u64, expected_status: InvoiceStatus) -> bool {
        match env
            .storage()
            .persistent()
            .get::<(Symbol, u64), Invoice>(&invoice_key(invoice_id))
        {
            Some(invoice) => invoice.status == expected_status,
            None => false,
        }
    }
}
