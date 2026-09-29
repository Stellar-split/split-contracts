//! Issue #764: dynamic recipient management (add / remove / update share in
//! basis points) before the first payment.
//!
//! Shares are kept in a side map `(rcp_bps, invoice_id)` seeded lazily from the
//! invoice's absolute amounts. The invoice total (`sum(amounts)`) is frozen in
//! `(rcp_tot, invoice_id)` on first use; per-recipient amounts are recomputed as
//! `total * bps / 10000` after every change (rounding remainder goes to the last
//! recipient once the shares sum to exactly 10000). While shares do not sum to
//! 10000, payments are rejected (`RecipientSharesIncomplete`).

use super::*;
use soroban_sdk::{contractimpl, symbol_short, Address, Env, Symbol, Vec};

fn bps_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("rcp_bps"), id)
}

fn total_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("rcp_tot"), id)
}

/// True when no dynamic shares are configured, or they sum to exactly 10000.
pub(crate) fn shares_complete(env: &Env, invoice_id: u64) -> bool {
    match env.storage().persistent().get::<_, Vec<u32>>(&bps_key(invoice_id)) {
        None => true,
        Some(bps) => bps.iter().sum::<u32>() == 10_000,
    }
}

fn load_shares(env: &Env, invoice_id: u64, invoice: &Invoice) -> (Vec<u32>, i128) {
    if let Some(bps) = env.storage().persistent().get::<_, Vec<u32>>(&bps_key(invoice_id)) {
        let total: i128 = env.storage().persistent().get(&total_key(invoice_id)).unwrap_or(0);
        return (bps, total);
    }
    let total: i128 = invoice.amounts.iter().sum();
    let mut bps: Vec<u32> = Vec::new(env);
    let mut acc: u32 = 0;
    let n = invoice.amounts.len();
    for (i, a) in invoice.amounts.iter().enumerate() {
        let b = if i as u32 + 1 == n {
            10_000 - acc
        } else {
            (a * 10_000 / total) as u32
        };
        acc += b;
        bps.push_back(b);
    }
    (bps, total)
}

fn guard(env: &Env, creator: &Address, invoice_id: u64) -> Invoice {
    require_not_paused(env);
    creator.require_auth();
    let invoice = load_invoice(env, invoice_id);
    assert!(
        invoice.creator == *creator,
        "only creator can modify recipients"
    );
    assert!(
        invoice.status == InvoiceStatus::Pending,
        "invoice is not pending"
    );
    assert!(!invoice.disputed, "invoice is disputed");
    assert!(
        invoice.funded == 0 && invoice.payments.is_empty(),
        "RecipientModificationAfterPayment"
    );
    invoice
}

fn commit(env: &Env, invoice_id: u64, mut invoice: Invoice, bps: Vec<u32>, total: i128) {
    let mut amounts: Vec<i128> = Vec::new(env);
    let mut sum: i128 = 0;
    for b in bps.iter() {
        let a = total * b as i128 / 10_000;
        sum += a;
        amounts.push_back(a);
    }
    if bps.iter().sum::<u32>() == 10_000 && !amounts.is_empty() {
        let last = amounts.len() - 1;
        let v = amounts.get(last).unwrap() + (total - sum);
        amounts.set(last, v);
    }
    invoice.amounts = amounts;
    env.storage().persistent().set(&bps_key(invoice_id), &bps);
    env.storage().persistent().set(&total_key(invoice_id), &total);

    let mut out: Vec<(Address, u32)> = Vec::new(env);
    for (i, r) in invoice.recipients.iter().enumerate() {
        out.push_back((r, bps.get(i as u32).unwrap()));
    }
    save_invoice(env, invoice_id, &invoice);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("rcp_upd"), invoice_id),
        out,
    );
}

#[contractimpl]
impl SplitContract {
    /// Add a recipient with `share_bps`; total bps must stay <= 10000.
    /// Panics with `RecipientModificationAfterPayment` once a payment exists.
    pub fn add_invoice_recipient(
        env: Env,
        invoice_id: u64,
        creator: Address,
        recipient: Address,
        share_bps: u32,
    ) {
        let mut invoice = guard(&env, &creator, invoice_id);
        assert!(share_bps > 0, "share must be positive");
        assert!(
            !invoice.recipients.iter().any(|r| r == recipient),
            "DuplicateRecipient"
        );
        let (mut bps, total) = load_shares(&env, invoice_id, &invoice);
        assert!(
            bps.iter().sum::<u32>() + share_bps <= 10_000,
            "total bps exceeds 10000"
        );
        let token = invoice.tokens.get(0).expect("no token");
        invoice.recipients.push_back(recipient.clone());
        invoice.tokens.push_back(token);
        invoice.claimed.push_back(0i128);
        bps.push_back(share_bps);
        commit(&env, invoice_id, invoice, bps, total);

        let key = recipient_invoice_ids_key(&recipient);
        let mut ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(&env));
        ids.push_back(invoice_id);
        env.storage().persistent().set(&key, &ids);
    }

    /// Remove a recipient. Remaining shares must be re-balanced afterwards
    /// (via `update_recipient_share` / `add_invoice_recipient`) before payments open.
    pub fn remove_invoice_recipient(
        env: Env,
        invoice_id: u64,
        creator: Address,
        recipient: Address,
    ) {
        let mut invoice = guard(&env, &creator, invoice_id);
        let idx = invoice
            .recipients
            .iter()
            .position(|r| r == recipient)
            .expect("RecipientNotFound") as u32;
        let (mut bps, total) = load_shares(&env, invoice_id, &invoice);
        invoice.recipients.remove(idx);
        invoice.tokens.remove(idx);
        invoice.claimed.remove(idx);
        bps.remove(idx);
        commit(&env, invoice_id, invoice, bps, total);

        let key = recipient_invoice_ids_key(&recipient);
        let ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(&env));
        let mut kept: Vec<u64> = Vec::new(&env);
        for i in ids.iter() {
            if i != invoice_id {
                kept.push_back(i);
            }
        }
        env.storage().persistent().set(&key, &kept);
    }

    /// Update one recipient's share. Total bps must equal exactly 10000 afterwards.
    pub fn update_recipient_share(
        env: Env,
        invoice_id: u64,
        creator: Address,
        recipient: Address,
        new_bps: u32,
    ) {
        let invoice = guard(&env, &creator, invoice_id);
        let idx = invoice
            .recipients
            .iter()
            .position(|r| r == recipient)
            .expect("RecipientNotFound") as u32;
        assert!(new_bps > 0, "share must be positive");
        let (mut bps, total) = load_shares(&env, invoice_id, &invoice);
        bps.set(idx, new_bps);
        assert!(
            bps.iter().sum::<u32>() == 10_000,
            "total bps must equal 10000"
        );
        commit(&env, invoice_id, invoice, bps, total);
    }

    /// Current per-recipient shares in basis points.
    pub fn get_recipient_shares(env: Env, invoice_id: u64) -> Vec<(Address, u32)> {
        let invoice = load_invoice(&env, invoice_id);
        let (bps, _) = load_shares(&env, invoice_id, &invoice);
        let mut out: Vec<(Address, u32)> = Vec::new(&env);
        for (i, r) in invoice.recipients.iter().enumerate() {
            out.push_back((r, bps.get(i as u32).unwrap()));
        }
        out
    }
}
