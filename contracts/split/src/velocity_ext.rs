//! Issue #775: invoice funding velocity metrics.
//!
//! Every credited payment adds to an hourly bucket (`hour = timestamp / 3600`).
//! At most [`MAX_BUCKETS`] (168 = 7 days) buckets are kept per invoice; when a
//! new hour is added beyond the cap the oldest bucket is removed. Storage is a
//! per-invoice sorted list of active hours plus one entry per bucket, all under
//! this module's own key enum.

use soroban_sdk::{contractimpl, contracttype, symbol_short, Env, Symbol, Vec};

use crate::SplitContract;

pub const MAX_BUCKETS: u32 = 168;

#[contracttype]
#[derive(Clone)]
pub enum VelocityKey {
    /// (invoice_id, hour_bucket) -> i128 amount paid in that hour
    Bucket(u64, u64),
    /// (invoice_id) -> Vec<u64> ascending active hour buckets
    Hours(u64),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VelocityBucket {
    pub hour: u64,
    pub amount: i128,
}

/// Record a credited payment amount for `invoice_id` at the current ledger time.
pub(crate) fn record(env: &Env, invoice_id: u64, amount: i128) {
    if amount <= 0 {
        return;
    }
    let hour = env.ledger().timestamp() / 3600;
    let store = env.storage().persistent();
    let bkey = VelocityKey::Bucket(invoice_id, hour);
    let existing: Option<i128> = store.get(&bkey);
    let cumulative = existing.unwrap_or(0).saturating_add(amount);
    store.set(&bkey, &cumulative);

    if existing.is_none() {
        let hkey = VelocityKey::Hours(invoice_id);
        let mut hours: Vec<u64> = store.get(&hkey).unwrap_or_else(|| Vec::new(env));
        // Keep ascending order (ledger time is monotonic, so this is normally a push).
        let mut idx = hours.len();
        while idx > 0 && hours.get(idx - 1).unwrap() > hour {
            idx -= 1;
        }
        hours.insert(idx, hour);
        while hours.len() > MAX_BUCKETS {
            let oldest = hours.pop_front().unwrap();
            store.remove(&VelocityKey::Bucket(invoice_id, oldest));
        }
        store.set(&hkey, &hours);
    }

    env.events().publish(
        (
            symbol_short!("split"),
            Symbol::new(env, "VelocityUpdated"),
            invoice_id,
        ),
        (hour, cumulative),
    );
}

#[contractimpl]
impl SplitContract {
    /// Issue #775: hourly funding buckets with `from_hour <= hour <= to_hour`,
    /// ascending. Read-only.
    pub fn get_funding_velocity(
        env: Env,
        invoice_id: u64,
        from_hour: u64,
        to_hour: u64,
    ) -> Vec<VelocityBucket> {
        let store = env.storage().persistent();
        let hours: Vec<u64> = store
            .get(&VelocityKey::Hours(invoice_id))
            .unwrap_or_else(|| Vec::new(&env));
        let mut out = Vec::new(&env);
        for h in hours.iter() {
            if h >= from_hour && h <= to_hour {
                let amount: i128 = store
                    .get(&VelocityKey::Bucket(invoice_id, h))
                    .unwrap_or(0);
                out.push_back(VelocityBucket { hour: h, amount });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use crate::ext_test_util::*;
    use soroban_sdk::{testutils::{Address as _, Ledger}, Address};

    #[test]
    fn buckets_populated_and_range_query() {
        let f = fixture();
        let (id, _, _) = new_invoice(&f, 1_000);
        let payer = Address::generate(&f.env);
        mint(&f.env, &f.token, &payer, 1_000);

        f.env.ledger().set_timestamp(3_600 * 10 + 5);
        pay(&f, &payer, id, 100);
        pay(&f, &payer, id, 50);
        f.env.ledger().set_timestamp(3_600 * 12);
        pay(&f, &payer, id, 200);

        let all = f.c.get_funding_velocity(&id, &0, &100);
        assert_eq!(all.len(), 2);
        assert_eq!(all.get(0).unwrap().hour, 10);
        assert_eq!(all.get(0).unwrap().amount, 150);
        assert_eq!(all.get(1).unwrap().hour, 12);
        assert_eq!(all.get(1).unwrap().amount, 200);

        let only_12 = f.c.get_funding_velocity(&id, &11, &12);
        assert_eq!(only_12.len(), 1);
        assert_eq!(only_12.get(0).unwrap().hour, 12);
        assert_eq!(f.c.get_funding_velocity(&id, &13, &20).len(), 0);
    }

    #[test]
    fn oldest_bucket_evicted_beyond_168() {
        let f = fixture();
        let (id, _, _) = new_invoice(&f, 1_000_000);
        let payer = Address::generate(&f.env);
        mint(&f.env, &f.token, &payer, 1_000_000);

        for h in 0..170u64 {
            f.env.ledger().set_timestamp(h * 3_600);
            pay(&f, &payer, id, 1);
        }
        let all = f.c.get_funding_velocity(&id, &0, &1_000);
        assert_eq!(all.len(), 168);
        assert_eq!(all.get(0).unwrap().hour, 2);
        assert_eq!(all.get(167).unwrap().hour, 169);
    }
}
