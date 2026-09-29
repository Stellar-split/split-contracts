//! Issue #818: cross-invoice payment pools.
//!
//! A creator groups several pending invoices into a pool. A single
//! `contribute_to_pool` call funds the pooled invoices in order, paying each
//! up to its remaining balance until the contribution is exhausted. Unused
//! contribution is never taken from the payer.

use super::*;
use soroban_sdk::{contractimpl, contracttype, symbol_short, Address, Env, Symbol, Vec};

pub const MAX_POOL_INVOICES: u32 = 20;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentPool {
    pub creator: Address,
    pub invoice_ids: Vec<u64>,
    pub total_contributed: i128,
}

fn pool_key(pool_id: u64) -> (Symbol, u64) {
    (symbol_short!("xpool"), pool_id)
}

fn pool_count_key() -> Symbol {
    symbol_short!("xpool_n")
}

fn remaining(invoice: &Invoice) -> i128 {
    let total: i128 = invoice.amounts.iter().sum();
    (total - invoice.funded).max(0)
}

#[contractimpl]
impl SplitContract {
    /// Create a pool over `invoice_ids`; every invoice must be pending and owned by `creator`.
    pub fn create_payment_pool(env: Env, creator: Address, invoice_ids: Vec<u64>) -> u64 {
        require_not_paused(&env);
        creator.require_auth();
        assert!(
            !invoice_ids.is_empty() && invoice_ids.len() <= MAX_POOL_INVOICES,
            "invalid pool size"
        );
        for (i, id) in invoice_ids.iter().enumerate() {
            let inv = load_invoice(&env, id);
            assert!(inv.creator == creator, "only creator can pool invoice");
            assert!(inv.status == InvoiceStatus::Pending, "invoice not pending");
            for other in invoice_ids.iter().skip(i + 1) {
                assert!(other != id, "duplicate invoice in pool");
            }
        }
        let pool_id: u64 = env.storage().instance().get(&pool_count_key()).unwrap_or(0) + 1;
        env.storage().instance().set(&pool_count_key(), &pool_id);
        let pool = PaymentPool {
            creator,
            invoice_ids: invoice_ids.clone(),
            total_contributed: 0,
        };
        env.storage().persistent().set(&pool_key(pool_id), &pool);
        env.events().publish(
            (symbol_short!("split"), symbol_short!("xpool_new"), pool_id),
            invoice_ids,
        );
        pool_id
    }

    /// Fund the pooled invoices in order; returns the amount actually applied.
    pub fn contribute_to_pool(env: Env, payer: Address, pool_id: u64, amount: i128) -> i128 {
        require_fn_not_paused(&env, &symbol_short!("pay"));
        require_not_frozen(&env);
        payer.require_auth();
        assert!(amount > 0, "amount must be positive");
        let mut pool: PaymentPool = env
            .storage()
            .persistent()
            .get(&pool_key(pool_id))
            .expect("pool not found");
        let mut left = amount;
        for id in pool.invoice_ids.iter() {
            if left == 0 {
                break;
            }
            let inv = load_invoice(&env, id);
            if inv.status != InvoiceStatus::Pending {
                continue;
            }
            let chunk = remaining(&inv).min(left);
            if chunk == 0 {
                continue;
            }
            Self::enforce_invoice_rate_limit(&env, id, &payer);
            let net = Self::_withhold_protocol_fee(&env, id, &payer, chunk);
            Self::_pay(&env, &payer, id, net, 0, false, None, None, false);
            left -= chunk;
        }
        let applied = amount - left;
        assert!(applied > 0, "pool fully funded");
        pool.total_contributed += applied;
        env.storage().persistent().set(&pool_key(pool_id), &pool);
        env.events().publish(
            (symbol_short!("split"), symbol_short!("xpool_pay"), pool_id),
            (payer, applied),
        );
        applied
    }

    pub fn get_payment_pool(env: Env, pool_id: u64) -> Option<PaymentPool> {
        env.storage().persistent().get(&pool_key(pool_id))
    }

    /// Total outstanding balance across all pending invoices in the pool.
    pub fn get_pool_remaining(env: Env, pool_id: u64) -> i128 {
        let pool: PaymentPool = env
            .storage()
            .persistent()
            .get(&pool_key(pool_id))
            .expect("pool not found");
        pool.invoice_ids
            .iter()
            .map(|id| load_invoice(&env, id))
            .filter(|inv| inv.status == InvoiceStatus::Pending)
            .map(|inv| remaining(&inv))
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use crate::ext_test_util::{default_options, fixture, mint};
    use soroban_sdk::{testutils::Address as _, Address, Vec};

    fn inv(f: &crate::ext_test_util::Fixture, creator: &Address, amount: i128) -> u64 {
        let mut rs = Vec::new(&f.env);
        rs.push_back(Address::generate(&f.env));
        let mut am = Vec::new(&f.env);
        am.push_back(amount);
        f.c.create_invoice(
            creator,
            &rs,
            &am,
            &f.token,
            &9_999_999_u64,
            &default_options(&f.env),
        )
    }

    #[test]
    fn contribution_spreads_across_invoices() {
        let f = fixture();
        let creator = Address::generate(&f.env);
        let a = inv(&f, &creator, 100);
        let b = inv(&f, &creator, 200);
        let mut ids = Vec::new(&f.env);
        ids.push_back(a);
        ids.push_back(b);
        let pool = f.c.create_payment_pool(&creator, &ids);
        assert_eq!(f.c.get_pool_remaining(&pool), 300);

        let payer = Address::generate(&f.env);
        mint(&f.env, &f.token, &payer, 1_000);
        assert_eq!(f.c.contribute_to_pool(&payer, &pool, &150), 150);
        assert_eq!(f.c.get_invoice(&b).funded, 50);
        assert_eq!(f.c.get_pool_remaining(&pool), 150);
        assert_eq!(f.c.contribute_to_pool(&payer, &pool, &500), 150);
        assert_eq!(f.c.get_pool_remaining(&pool), 0);
        assert_eq!(f.c.get_payment_pool(&pool).unwrap().total_contributed, 300);
    }

    #[test]
    #[should_panic(expected = "only creator can pool invoice")]
    fn cannot_pool_foreign_invoice() {
        let f = fixture();
        let creator = Address::generate(&f.env);
        let a = inv(&f, &Address::generate(&f.env), 100);
        let mut ids = Vec::new(&f.env);
        ids.push_back(a);
        f.c.create_payment_pool(&creator, &ids);
    }

    #[test]
    #[should_panic(expected = "duplicate invoice in pool")]
    fn duplicate_invoice_rejected() {
        let f = fixture();
        let creator = Address::generate(&f.env);
        let a = inv(&f, &creator, 100);
        let mut ids = Vec::new(&f.env);
        ids.push_back(a);
        ids.push_back(a);
        f.c.create_payment_pool(&creator, &ids);
    }
}
