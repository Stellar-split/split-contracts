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

/// Issue #1039: Derived performance metrics computed from a creator's raw
/// `CreatorDashboard` counters. Nothing new is stored — this is a read-only
/// aggregation over data the contract already tracks, so it stays
/// consistent with `record_created`/`record_released`/`record_refunded`
/// automatically and can't drift out of sync with the raw counters.
#[contracttype]
#[derive(Clone, Debug)]
pub struct CreatorPerformanceMetrics {
    /// Raw counters this view is derived from.
    pub raw: CreatorDashboard,
    /// `released_count / invoice_count`, in basis points (0 when no invoices yet).
    pub release_rate_bps: u32,
    /// `refunded_count / invoice_count`, in basis points (0 when no invoices yet).
    pub refund_rate_bps: u32,
    /// Invoices neither released nor refunded yet.
    pub pending_count: u64,
    /// `total_volume / invoice_count` (0 when no invoices yet).
    pub avg_invoice_amount: i128,
    /// `released_volume / released_count` (0 when nothing released yet).
    pub avg_released_amount: i128,
}

fn compute_metrics(dashboard: CreatorDashboard) -> CreatorPerformanceMetrics {
    const BPS: i128 = 10_000;

    let release_rate_bps = if dashboard.invoice_count > 0 {
        ((dashboard.released_count as i128 * BPS) / dashboard.invoice_count as i128) as u32
    } else {
        0
    };
    let refund_rate_bps = if dashboard.invoice_count > 0 {
        ((dashboard.refunded_count as i128 * BPS) / dashboard.invoice_count as i128) as u32
    } else {
        0
    };
    let pending_count = dashboard
        .invoice_count
        .saturating_sub(dashboard.released_count)
        .saturating_sub(dashboard.refunded_count);
    let avg_invoice_amount = if dashboard.invoice_count > 0 {
        dashboard.total_volume / dashboard.invoice_count as i128
    } else {
        0
    };
    let avg_released_amount = if dashboard.released_count > 0 {
        dashboard.released_volume / dashboard.released_count as i128
    } else {
        0
    };

    CreatorPerformanceMetrics {
        raw: dashboard,
        release_rate_bps,
        refund_rate_bps,
        pending_count,
        avg_invoice_amount,
        avg_released_amount,
    }
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
        let mut dashboard: CreatorDashboard =
            env.storage()
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
        let mut dashboard: CreatorDashboard =
            env.storage()
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
        let mut dashboard: CreatorDashboard =
            env.storage()
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

    /// Issue #1039: Performance dashboard view — derived rates and averages
    /// computed on-demand from the creator's existing raw counters.
    pub fn get_creator_performance_metrics(env: Env, creator: Address) -> CreatorPerformanceMetrics {
        compute_metrics(Self::get_creator_dashboard(env, creator))
    }
}

#[cfg(test)]
mod test;
