//! Issue #889: Invoice sustainability impact tracking.
//!
//! Invoice creators record sustainability impact (e.g. grams of CO2 offset,
//! trees planted) against their invoices, grouped by category. Per-invoice and
//! global running totals are maintained and an event is emitted per record.

use crate::SplitContract;
#[allow(unused_imports)]
use crate::{SplitContractArgs, SplitContractClient};
use soroban_sdk::{contractimpl, contracttype, symbol_short, Address, Env, Symbol, Vec};

/// Maximum distinct impact categories tracked per invoice.
const MAX_IMPACT_CATEGORIES: u32 = 20;

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum SustainabilityKey {
    /// Impact total per (invoice, category): i128
    Impact(u64, Symbol),
    /// Categories recorded for an invoice: Vec<Symbol>
    Categories(u64),
    /// Global total per category: i128
    GlobalImpact(Symbol),
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ImpactEntry {
    pub category: Symbol,
    pub value: i128,
}

#[contractimpl]
impl SplitContract {
    /// Record a sustainability impact `value` in `category` for an invoice.
    /// Only the invoice creator may record. Values accumulate.
    pub fn record_sustainability_impact(
        env: Env,
        creator: Address,
        invoice_id: u64,
        category: Symbol,
        value: i128,
    ) {
        creator.require_auth();
        assert!(value > 0, "impact value must be positive");
        let invoice = crate::load_invoice(&env, invoice_id);
        assert!(invoice.creator == creator, "only creator can record impact");

        let mut cats: Vec<Symbol> = env
            .storage()
            .persistent()
            .get(&SustainabilityKey::Categories(invoice_id))
            .unwrap_or(Vec::new(&env));
        if !cats.contains(category.clone()) {
            assert!(cats.len() < MAX_IMPACT_CATEGORIES, "too many impact categories");
            cats.push_back(category.clone());
            env.storage()
                .persistent()
                .set(&SustainabilityKey::Categories(invoice_id), &cats);
        }

        let key = SustainabilityKey::Impact(invoice_id, category.clone());
        let total: i128 = env.storage().persistent().get(&key).unwrap_or(0) + value;
        env.storage().persistent().set(&key, &total);

        let gkey = SustainabilityKey::GlobalImpact(category.clone());
        let global: i128 = env.storage().persistent().get(&gkey).unwrap_or(0) + value;
        env.storage().persistent().set(&gkey, &global);

        env.events().publish(
            (symbol_short!("split"), symbol_short!("sust_rec"), invoice_id),
            (category, value, total),
        );
    }

    /// Impact total for one category on an invoice.
    pub fn get_invoice_impact(env: Env, invoice_id: u64, category: Symbol) -> i128 {
        env.storage()
            .persistent()
            .get(&SustainabilityKey::Impact(invoice_id, category))
            .unwrap_or(0)
    }

    /// All recorded impact entries for an invoice.
    pub fn get_invoice_impact_report(env: Env, invoice_id: u64) -> Vec<ImpactEntry> {
        let cats: Vec<Symbol> = env
            .storage()
            .persistent()
            .get(&SustainabilityKey::Categories(invoice_id))
            .unwrap_or(Vec::new(&env));
        let mut out = Vec::new(&env);
        for category in cats.iter() {
            let value: i128 = env
                .storage()
                .persistent()
                .get(&SustainabilityKey::Impact(invoice_id, category.clone()))
                .unwrap_or(0);
            out.push_back(ImpactEntry { category, value });
        }
        out
    }

    /// Global impact total for a category across all invoices.
    pub fn get_global_impact(env: Env, category: Symbol) -> i128 {
        env.storage()
            .persistent()
            .get(&SustainabilityKey::GlobalImpact(category))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::default_options;
    use soroban_sdk::testutils::{Address as _, Ledger};
    use soroban_sdk::{vec, Env};

    fn setup() -> (Env, SplitContractClient<'static>, Address, u64) {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(1_000);
        let id = env.register(SplitContract, ());
        let c = SplitContractClient::new(&env, &id);
        let token = env
            .register_stellar_asset_contract_v2(Address::generate(&env))
            .address();
        c.initialize(
            &Address::generate(&env),
            &0_i128,
            &Address::generate(&env),
            &token,
            &0_u32,
            &None,
            &0_u32,
            &0_u32,
            &0_u64,
        );
        let creator = Address::generate(&env);
        let invoice_id = c.create_invoice(
            &creator,
            &vec![&env, Address::generate(&env)],
            &vec![&env, 100_i128],
            &token,
            &100_000_u64,
            &default_options(&env),
        );
        (env, c, creator, invoice_id)
    }

    #[test]
    fn test_record_accumulates_and_reports() {
        let (_env, c, creator, id) = setup();
        let co2 = symbol_short!("co2");
        c.record_sustainability_impact(&creator, &id, &co2, &50);
        c.record_sustainability_impact(&creator, &id, &co2, &25);
        c.record_sustainability_impact(&creator, &id, &symbol_short!("trees"), &3);
        assert_eq!(c.get_invoice_impact(&id, &co2), 75);
        assert_eq!(c.get_global_impact(&co2), 75);
        assert_eq!(c.get_invoice_impact_report(&id).len(), 2);
    }

    #[test]
    #[should_panic(expected = "only creator can record impact")]
    fn test_non_creator_rejected() {
        let (env, c, _creator, id) = setup();
        c.record_sustainability_impact(&Address::generate(&env), &id, &symbol_short!("co2"), &1);
    }

    #[test]
    #[should_panic(expected = "impact value must be positive")]
    fn test_zero_value_rejected() {
        let (_env, c, creator, id) = setup();
        c.record_sustainability_impact(&creator, &id, &symbol_short!("co2"), &0);
    }
}
