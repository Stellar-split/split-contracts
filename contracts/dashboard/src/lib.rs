#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol};

#[contracttype]
#[derive(Clone, Debug)]
pub struct CreatorDashboard {
    pub invoice_count: u64,
    pub total_volume: i128,
    pub released_count: u64,
    pub refunded_count: u64,
    pub released_volume: i128,
}

fn authorized_contract_key() -> Symbol {
    symbol_short!("auth_ctr")
}

fn creator_dashboard_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("cr_dash"), creator.clone())
}

#[contract]
pub struct DashboardContract;

#[contractimpl]
impl DashboardContract {
    pub fn initialize(env: Env, split_contract: Address) {
        assert!(
            !env.storage().instance().has(&authorized_contract_key()),
            "already initialized"
        );
        env.storage()
            .instance()
            .set(&authorized_contract_key(), &split_contract);
    }

    pub fn record_created(env: Env, creator: Address, total: i128) {
        let authorized: Address = env
            .storage()
            .instance()
            .get(&authorized_contract_key())
            .expect("not initialized");
        authorized.require_auth();

        let key = creator_dashboard_key(&creator);
        let mut dashboard: CreatorDashboard = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(CreatorDashboard {
                invoice_count: 0,
                total_volume: 0,
                released_count: 0,
                refunded_count: 0,
                released_volume: 0,
            });
        dashboard.invoice_count = dashboard
            .invoice_count
            .checked_add(1)
            .expect("invoice_count overflow");
        dashboard.total_volume = dashboard
            .total_volume
            .checked_add(total)
            .expect("total_volume overflow");
        env.storage().persistent().set(&key, &dashboard);
    }

    pub fn record_released(env: Env, creator: Address, total: i128) {
        let authorized: Address = env
            .storage()
            .instance()
            .get(&authorized_contract_key())
            .expect("not initialized");
        authorized.require_auth();

        let key = creator_dashboard_key(&creator);
        let mut dashboard: CreatorDashboard = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(CreatorDashboard {
                invoice_count: 0,
                total_volume: 0,
                released_count: 0,
                refunded_count: 0,
                released_volume: 0,
            });
        dashboard.released_count = dashboard
            .released_count
            .checked_add(1)
            .expect("released_count overflow");
        dashboard.released_volume = dashboard
            .released_volume
            .checked_add(total)
            .expect("released_volume overflow");
        env.storage().persistent().set(&key, &dashboard);
    }

    pub fn record_refunded(env: Env, creator: Address) {
        let authorized: Address = env
            .storage()
            .instance()
            .get(&authorized_contract_key())
            .expect("not initialized");
        authorized.require_auth();

        let key = creator_dashboard_key(&creator);
        let mut dashboard: CreatorDashboard = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(CreatorDashboard {
                invoice_count: 0,
                total_volume: 0,
                released_count: 0,
                refunded_count: 0,
                released_volume: 0,
            });
        dashboard.refunded_count = dashboard
            .refunded_count
            .checked_add(1)
            .expect("refunded_count overflow");
        env.storage().persistent().set(&key, &dashboard);
    }

    pub fn get_creator_dashboard(env: Env, creator: Address) -> CreatorDashboard {
        let key = creator_dashboard_key(&creator);
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or(CreatorDashboard {
                invoice_count: 0,
                total_volume: 0,
                released_count: 0,
                refunded_count: 0,
                released_volume: 0,
            })
    }
}

#[cfg(test)]
mod test;
