#![no_std]
//! Issue #791: read-only view contract for dashboard queries.
//!
//! Deployed alongside the split contract. Soroban contracts cannot read another
//! contract's storage, so each view calls the split contract's public getters
//! and reshapes the result. The mirror types below must keep the same field
//! names and types as their counterparts in `split::types`, since values are
//! decoded by XDR shape.

use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, Env, IntoVal, Symbol, Val, Vec,
};

#[cfg(test)]
mod test;

/// Mirror of `split::types::CreatorStats`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatorStats {
    pub total_invoices: u32,
    pub total_raised: u64,
    pub total_released: u64,
    pub total_payers: u32,
    pub avg_funding_time_ledgers: u32,
    pub total_refunded: u32,
}

/// Mirror of `split::types::PaymentRecord`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentRecord {
    pub invoice_id: u64,
    pub amount: i128,
    pub ledger: u32,
}

/// Aggregated per-creator figures for a dashboard.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatorDashboard {
    pub total_invoices: u32,
    pub total_raised: u64,
    pub total_released: u64,
    pub total_payers: u32,
    pub total_refunded: u32,
}

fn split_contract_key() -> Symbol {
    symbol_short!("split")
}

#[contract]
pub struct SplitViewContract;

#[contractimpl]
impl SplitViewContract {
    /// Point the view at a deployed split contract. Can only be called once.
    pub fn initialize(env: Env, split_contract: Address) {
        let key = split_contract_key();
        assert!(!env.storage().instance().has(&key), "already initialized");
        env.storage().instance().set(&key, &split_contract);
    }

    /// The split contract this view reads from.
    pub fn get_split_contract(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&split_contract_key())
            .expect("not initialized")
    }

    /// Dashboard totals for `creator`, from the split contract's creator stats.
    pub fn get_creator_dashboard(env: Env, creator: Address) -> CreatorDashboard {
        let split = Self::get_split_contract(env.clone());
        let args: Vec<Val> = (creator,).into_val(&env);
        let stats: CreatorStats =
            env.invoke_contract(&split, &Symbol::new(&env, "get_creator_stats"), args);
        CreatorDashboard {
            total_invoices: stats.total_invoices,
            total_raised: stats.total_raised,
            total_released: stats.total_released,
            total_payers: stats.total_payers,
            total_refunded: stats.total_refunded,
        }
    }

    /// The most recent `limit` payments recorded for `payer`, oldest first.
    pub fn get_payer_history(env: Env, payer: Address, limit: u32) -> Vec<PaymentRecord> {
        let split = Self::get_split_contract(env.clone());
        let args: Vec<Val> = (payer, 0_u32, u32::MAX).into_val(&env);
        let history: Vec<PaymentRecord> =
            env.invoke_contract(&split, &Symbol::new(&env, "get_payer_history"), args);
        let start = history.len().saturating_sub(limit);
        history.slice(start..)
    }
}
