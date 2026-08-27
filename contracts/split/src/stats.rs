use soroban_sdk::{Address, Env, Symbol, symbol_short, Vec};

pub fn get_stats(env: &Env) -> (u64, i128, i128, i128) {
    let total_invoices = env
        .storage()
        .persistent()
        .get(&total_invoices_key())
        .unwrap_or(0u64);
    let total_volume = env
        .storage()
        .persistent()
        .get(&total_volume_key())
        .unwrap_or(0i128);
    let total_released = env
        .storage()
        .persistent()
        .get(&total_released_key())
        .unwrap_or(0i128);
    let total_refunded = env
        .storage()
        .persistent()
        .get(&total_refunded_key())
        .unwrap_or(0i128);
    (total_invoices, total_volume, total_released, total_refunded)
}

pub fn increment_invoice_count(env: &Env) {
    let count: u64 = env
        .storage()
        .persistent()
        .get(&total_invoices_key())
        .unwrap_or(0u64);
    env.storage()
        .persistent()
        .set(&total_invoices_key(), &count.checked_add(1).expect("overflow"));
}

pub fn increment_volume(env: &Env, amount: i128) {
    let volume: i128 = env
        .storage()
        .persistent()
        .get(&total_volume_key())
        .unwrap_or(0i128);
    env.storage()
        .persistent()
        .set(&total_volume_key(), &volume.checked_add(amount).expect("overflow"));
}

pub fn increment_released(env: &Env, amount: i128) {
    let released: i128 = env
        .storage()
        .persistent()
        .get(&total_released_key())
        .unwrap_or(0i128);
    env.storage()
        .persistent()
        .set(&total_released_key(), &released.checked_add(amount).expect("overflow"));
}

pub fn increment_refunded(env: &Env, amount: i128) {
    let refunded: i128 = env
        .storage()
        .persistent()
        .get(&total_refunded_key())
        .unwrap_or(0i128);
    env.storage()
        .persistent()
        .set(&total_refunded_key(), &refunded.checked_add(amount).expect("overflow"));
}

fn total_invoices_key() -> Symbol {
    symbol_short!("tot_inv")
}

fn total_volume_key() -> Symbol {
    symbol_short!("tot_vol")
}

fn total_released_key() -> Symbol {
    symbol_short!("tot_rel")
}

fn total_refunded_key() -> Symbol {
    symbol_short!("tot_ref")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_stats_defaults_to_zero() {
        let env = Env::default();

        let stats = get_stats(&env);
        assert_eq!(stats.0, 0, "total_invoices should default to 0");
        assert_eq!(stats.1, 0, "total_volume should default to 0");
        assert_eq!(stats.2, 0, "total_released should default to 0");
        assert_eq!(stats.3, 0, "total_refunded should default to 0");
    }

    #[test]
    fn record_invoice_created_increments_counter() {
        let env = Env::default();

        let stats_before = get_stats(&env);
        assert_eq!(stats_before.0, 0, "invoice count should start at 0");

        increment_invoice_count(&env);

        let stats_after = get_stats(&env);
        assert_eq!(stats_after.0, 1, "invoice count should increment to 1");
        assert_eq!(stats_before.1, stats_after.1, "volume should remain unchanged");
        assert_eq!(stats_before.2, stats_after.2, "released should remain unchanged");
        assert_eq!(stats_before.3, stats_after.3, "refunded should remain unchanged");
    }
}
