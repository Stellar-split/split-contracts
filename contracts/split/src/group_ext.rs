//! Issue #779: named campaign groups linking related invoices.

use crate::error::ContractError;
use soroban_sdk::{contracttype, symbol_short, Address, Bytes, Env, Symbol, Vec};

pub const MAX_GROUP_INVOICES: u32 = 20;

#[contracttype]
#[derive(Clone)]
pub enum GroupKey {
    Counter,
    Group(u64),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CampaignGroup {
    pub creator: Address,
    pub name: Symbol,
    pub description: Bytes,
    pub invoices: Vec<u64>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupStats {
    pub name: Symbol,
    pub total_target: i128,
    pub total_funded: i128,
    pub invoice_count: u32,
    pub fully_funded_count: u32,
}

pub fn get_group(env: &Env, id: u64) -> CampaignGroup {
    match env.storage().persistent().get(&GroupKey::Group(id)) {
        Some(g) => g,
        None => env.panic_with_error(ContractError::GroupNotFound),
    }
}

pub fn get_invoices(env: &Env, id: u64) -> Vec<u64> {
    get_group(env, id).invoices
}

pub fn create_group(env: &Env, creator: Address, name: Symbol, description: Bytes) -> u64 {
    creator.require_auth();
    let id: u64 = env.storage().instance().get(&GroupKey::Counter).unwrap_or(0u64) + 1;
    env.storage().instance().set(&GroupKey::Counter, &id);
    let g = CampaignGroup {
        creator: creator.clone(),
        name,
        description,
        invoices: Vec::new(env),
    };
    env.storage().persistent().set(&GroupKey::Group(id), &g);
    env.events()
        .publish((symbol_short!("split"), symbol_short!("grp_new"), id), creator);
    id
}

pub fn add_invoice(env: &Env, group_id: u64, invoice_id: u64, creator: &Address) {
    let mut g = get_group(env, group_id);
    if g.creator != *creator {
        env.panic_with_error(ContractError::NotAuthorized);
    }
    if g.invoices.len() >= MAX_GROUP_INVOICES {
        env.panic_with_error(ContractError::GroupFull);
    }
    if g.invoices.contains(invoice_id) {
        env.panic_with_error(ContractError::AlreadyInGroup);
    }
    g.invoices.push_back(invoice_id);
    env.storage().persistent().set(&GroupKey::Group(group_id), &g);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("grp_add"), group_id),
        invoice_id,
    );
}
