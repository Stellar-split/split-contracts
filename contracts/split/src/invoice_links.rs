//! Issue #859: Cross-contract invoice linking.
//!
//! Lets an invoice creator associate a local invoice with an invoice held by
//! another split contract deployment (or another invoice in this contract).
//! Links are stored locally and can be verified on demand: verification calls
//! `get_invoice` on the remote contract and records whether the remote invoice
//! exists and its status at that time. A remote call that fails (missing
//! invoice, incompatible contract) never aborts the caller's transaction — the
//! link is simply marked unverified.

use crate::error::ContractError;
use crate::types::{ExternalInvoiceLink, InvoiceCore};
use crate::{
    events, invoice_key, load_invoice, require_creator_or_cocreator, require_not_paused, SplitContract,
    SplitContractArgs, SplitContractClient,
};
use soroban_sdk::{
    contractimpl, panic_with_error, symbol_short, Address, Env, IntoVal, Symbol, Vec,
};

/// Upper bound on links per invoice to keep storage bounded.
pub const MAX_EXTERNAL_LINKS: u32 = 10;

fn links_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("xlinks"), invoice_id)
}

fn load_links(env: &Env, invoice_id: u64) -> Vec<ExternalInvoiceLink> {
    env.storage()
        .persistent()
        .get(&links_key(invoice_id))
        .unwrap_or(Vec::new(env))
}

fn save_links(env: &Env, invoice_id: u64, links: &Vec<ExternalInvoiceLink>) {
    if links.is_empty() {
        env.storage().persistent().remove(&links_key(invoice_id));
    } else {
        env.storage().persistent().set(&links_key(invoice_id), links);
    }
}

fn find_link(links: &Vec<ExternalInvoiceLink>, remote: &Address, remote_id: u64) -> Option<u32> {
    links
        .iter()
        .position(|l| l.remote_contract == *remote && l.remote_invoice_id == remote_id)
        .map(|i| i as u32)
}

/// Fetch a remote invoice without propagating remote failures. Links that
/// point back at this contract are resolved locally, since Soroban forbids a
/// contract from re-entering itself.
fn fetch_remote_invoice(env: &Env, remote: &Address, remote_id: u64) -> Option<InvoiceCore> {
    if *remote == env.current_contract_address() {
        let key = invoice_key(remote_id);
        if !env.storage().persistent().has(&key) && !env.storage().instance().has(&key) {
            return None;
        }
        return Some(load_invoice(env, remote_id).split().0);
    }
    let args: Vec<soroban_sdk::Val> = (remote_id,).into_val(env);
    match env.try_invoke_contract::<InvoiceCore, soroban_sdk::Error>(
        remote,
        &Symbol::new(env, "get_invoice"),
        args,
    ) {
        Ok(Ok(inv)) => Some(inv),
        _ => None,
    }
}

#[contractimpl]
impl SplitContract {
    /// Link `invoice_id` to `remote_invoice_id` on `remote_contract`.
    /// Caller must be the invoice creator or a co-creator.
    pub fn link_external_invoice(
        env: Env,
        caller: Address,
        invoice_id: u64,
        remote_contract: Address,
        remote_invoice_id: u64,
    ) {
        require_not_paused(&env);
        caller.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        require_creator_or_cocreator(&invoice, &caller);

        if remote_contract == env.current_contract_address() && remote_invoice_id == invoice_id {
            panic_with_error!(&env, ContractError::InvalidLink);
        }

        let mut links = load_links(&env, invoice_id);
        if find_link(&links, &remote_contract, remote_invoice_id).is_some() {
            panic_with_error!(&env, ContractError::LinkAlreadyExists);
        }
        if links.len() >= MAX_EXTERNAL_LINKS {
            panic_with_error!(&env, ContractError::LinkLimitReached);
        }

        links.push_back(ExternalInvoiceLink {
            remote_contract: remote_contract.clone(),
            remote_invoice_id,
            verified: false,
            remote_status: None,
            linked_at: env.ledger().timestamp(),
        });
        save_links(&env, invoice_id, &links);

        events::external_link_added(&env, invoice_id, &remote_contract, remote_invoice_id);
    }

    /// Remove a previously created link. Caller must be creator or co-creator.
    pub fn unlink_external_invoice(
        env: Env,
        caller: Address,
        invoice_id: u64,
        remote_contract: Address,
        remote_invoice_id: u64,
    ) {
        require_not_paused(&env);
        caller.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        require_creator_or_cocreator(&invoice, &caller);

        let mut links = load_links(&env, invoice_id);
        let idx = find_link(&links, &remote_contract, remote_invoice_id)
            .unwrap_or_else(|| panic_with_error!(&env, ContractError::LinkNotFound));
        links.remove(idx);
        save_links(&env, invoice_id, &links);

        events::external_link_removed(&env, invoice_id, &remote_contract, remote_invoice_id);
    }

    /// Query the remote contract and refresh the link's verification state.
    /// Permissionless. Returns `true` if the remote invoice was found.
    pub fn verify_external_link(
        env: Env,
        invoice_id: u64,
        remote_contract: Address,
        remote_invoice_id: u64,
    ) -> bool {
        let mut links = load_links(&env, invoice_id);
        let idx = find_link(&links, &remote_contract, remote_invoice_id)
            .unwrap_or_else(|| panic_with_error!(&env, ContractError::LinkNotFound));

        let remote = fetch_remote_invoice(&env, &remote_contract, remote_invoice_id);
        let mut link = links.get(idx).unwrap();
        link.verified = remote.is_some();
        link.remote_status = remote.map(|inv| inv.status);
        let verified = link.verified;
        links.set(idx, link);
        save_links(&env, invoice_id, &links);

        events::external_link_verified(&env, invoice_id, &remote_contract, remote_invoice_id, verified);
        verified
    }

    /// All cross-contract links recorded for `invoice_id`.
    pub fn get_external_links(env: Env, invoice_id: u64) -> Vec<ExternalInvoiceLink> {
        load_links(&env, invoice_id)
    }

    /// Whether `invoice_id` is linked to `remote_invoice_id` on `remote_contract`.
    pub fn is_externally_linked(
        env: Env,
        invoice_id: u64,
        remote_contract: Address,
        remote_invoice_id: u64,
    ) -> bool {
        find_link(&load_links(&env, invoice_id), &remote_contract, remote_invoice_id).is_some()
    }
}
