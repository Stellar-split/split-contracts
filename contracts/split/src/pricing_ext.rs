//! Issue #872: Advanced invoice pricing model.
//!
//! A creator can attach a tiered pricing model to an invoice. The effective
//! price for a given `amount` is computed as:
//!
//!   effective = base_price * (10_000 - discount_bps) / 10_000 * surge_bps / 10_000
//!
//! where `discount_bps` is the highest tier whose `min_amount` ≤ `amount`.

use super::*;
use crate::events;
use crate::storage_keys::InvoiceKey;
use soroban_sdk::{contractimpl, panic_with_error, Address, Env, Vec};

fn pricing_key(invoice_id: u64) -> InvoiceKey {
    InvoiceKey::PricingModel(invoice_id)
}

#[contractimpl]
impl SplitContract {
    /// Set or replace the pricing model for an invoice (creator only).
    ///
    /// Tiers are stored as-is; the caller is responsible for sorting them
    /// ascending by `min_amount`.
    pub fn set_invoice_pricing(
        env: Env,
        creator: Address,
        invoice_id: u64,
        base_price: i128,
        tiers: Vec<InvoicePriceTier>,
        surge_bps: u32,
    ) {
        require_not_paused(&env);
        creator.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        if invoice.creator != creator {
            panic_with_error!(&env, ContractError::NotAuthorized);
        }
        if base_price <= 0 {
            panic_with_error!(&env, ContractError::InvalidAmount);
        }
        let model = InvoicePricingModel {
            base_price,
            tiers: tiers.clone(),
            surge_bps,
            surge_active: false,
        };
        env.storage()
            .persistent()
            .set(&pricing_key(invoice_id), &model);
        let tier_count = tiers.len();
        events::pricing_model_set(&env, invoice_id, base_price, tier_count);
    }

    /// Activate or deactivate surge pricing on an invoice (creator only).
    pub fn set_invoice_surge(
        env: Env,
        creator: Address,
        invoice_id: u64,
        active: bool,
    ) {
        require_not_paused(&env);
        creator.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        if invoice.creator != creator {
            panic_with_error!(&env, ContractError::NotAuthorized);
        }
        let key = pricing_key(invoice_id);
        let mut model: InvoicePricingModel = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, ContractError::NoPricingModel));
        if model.surge_active == active {
            panic_with_error!(&env, ContractError::SurgeStateUnchanged);
        }
        model.surge_active = active;
        env.storage().persistent().set(&key, &model);
        events::pricing_surge_toggled(&env, invoice_id, active);
    }

    /// Get the pricing model for an invoice. Returns None if not configured.
    pub fn get_invoice_pricing(env: Env, invoice_id: u64) -> Option<InvoicePricingModel> {
        env.storage()
            .persistent()
            .get(&pricing_key(invoice_id))
    }

    /// Compute the effective price for a given `amount` using the invoice's
    /// pricing model. Returns `amount` unchanged if no model is configured.
    pub fn compute_effective_price(env: Env, invoice_id: u64, amount: i128) -> i128 {
        let key = pricing_key(invoice_id);
        let model: InvoicePricingModel = match env.storage().persistent().get(&key) {
            Some(m) => m,
            None => return amount,
        };
        // Find the highest matching tier (sorted ascending by min_amount).
        let mut discount_bps: u32 = 0;
        for tier in model.tiers.iter() {
            if amount >= tier.min_amount {
                discount_bps = tier.discount_bps;
            }
        }
        // Apply discount.
        let discounted = model.base_price
            .saturating_mul(10_000 - discount_bps as i128)
            / 10_000;
        // Apply surge multiplier if active.
        if model.surge_active {
            discounted
                .saturating_mul(model.surge_bps as i128)
                / 10_000
        } else {
            discounted
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{client, make_invoice, setup_initialized};
    use crate::types::InvoicePriceTier;
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn set_and_get_pricing_model() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &Address::generate(&env), 1000, &token, 9_999);
        let tiers = soroban_sdk::vec![
            &env,
            InvoicePriceTier { min_amount: 500, discount_bps: 200 },
            InvoicePriceTier { min_amount: 1000, discount_bps: 500 },
        ];
        c.set_invoice_pricing(&creator, &id, &900, &tiers, &10_000);
        let model = c.get_invoice_pricing(&id).expect("should have model");
        assert_eq!(model.base_price, 900);
        assert_eq!(model.tiers.len(), 2);
        assert!(!model.surge_active);
    }

    #[test]
    fn compute_effective_price_no_model_returns_amount() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &Address::generate(&env), 1000, &token, 9_999);
        assert_eq!(c.compute_effective_price(&id, &1000), 1000);
    }

    #[test]
    fn compute_effective_price_applies_tier_discount() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &Address::generate(&env), 1000, &token, 9_999);
        // base_price=1000, tier at min_amount=500 with 10% discount, no surge
        let tiers = soroban_sdk::vec![
            &env,
            InvoicePriceTier { min_amount: 500, discount_bps: 1_000 },
        ];
        c.set_invoice_pricing(&creator, &id, &1000, &tiers, &10_000);
        // amount=700 >= 500 so 10% discount: 1000 * 9000/10000 = 900
        assert_eq!(c.compute_effective_price(&id, &700), 900);
        // amount=300 < 500 so no discount: 1000 * 10000/10000 = 1000
        assert_eq!(c.compute_effective_price(&id, &300), 1000);
    }

    #[test]
    fn surge_pricing_toggle() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &Address::generate(&env), 1000, &token, 9_999);
        // base_price=1000, no tiers, surge 1.5x
        c.set_invoice_pricing(&creator, &id, &1000, &soroban_sdk::vec![&env], &10_000);
        c.set_invoice_surge(&creator, &id, &true);
        let model = c.get_invoice_pricing(&id).expect("model");
        assert!(model.surge_active);
        // toggling again to same state should error
        assert!(c.try_set_invoice_surge(&creator, &id, &true).is_err());
    }

    #[test]
    fn only_creator_can_set_pricing() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let other = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &Address::generate(&env), 1000, &token, 9_999);
        assert!(c
            .try_set_invoice_pricing(&other, &id, &1000, &soroban_sdk::vec![&env], &10_000)
            .is_err());
    }
}
