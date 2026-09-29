//! Issue #776: on-chain creator -> invoice-ID index with cursor pagination.
//!
//! Owns its own key enum so it does not consume a slot in the shared
//! `StorageKey` enum (Soroban's 50-variant limit).

use crate::error::ContractError;
use soroban_sdk::{contracttype, Address, Env, Vec};

/// Maximum page size accepted by `get_creator_invoices`.
pub const MAX_PAGE_LIMIT: u32 = 50;

#[contracttype]
#[derive(Clone)]
pub enum SearchKey {
    /// `Vec<u64>` of invoice IDs (ascending) created by this address.
    CreatorInvoices(Address),
}

/// A page of invoice IDs. `next_cursor` is `Some(last_id)` when more remain.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvoicePage {
    pub items: Vec<u64>,
    pub next_cursor: Option<u64>,
    pub total: u32,
}

pub fn index_invoice(env: &Env, creator: &Address, invoice_id: u64) {
    let key = SearchKey::CreatorInvoices(creator.clone());
    let mut ids: Vec<u64> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|| Vec::new(env));
    ids.push_back(invoice_id);
    env.storage().persistent().set(&key, &ids);
}

pub fn get_creator_invoices(
    env: &Env,
    creator: Address,
    limit: u32,
    cursor: Option<u64>,
) -> InvoicePage {
    if limit > MAX_PAGE_LIMIT {
        env.panic_with_error(ContractError::LimitTooLarge);
    }
    let ids: Vec<u64> = env
        .storage()
        .persistent()
        .get(&SearchKey::CreatorInvoices(creator))
        .unwrap_or_else(|| Vec::new(env));
    let total = ids.len();
    let mut items = Vec::new(env);
    let mut has_more = false;
    for id in ids.iter() {
        if let Some(c) = cursor {
            if id <= c {
                continue;
            }
        }
        if items.len() >= limit {
            has_more = true;
            break;
        }
        items.push_back(id);
    }
    let next_cursor = if has_more { items.last() } else { None };
    InvoicePage { items, next_cursor, total }
}
