//! Issue #875: creator brand loyalty program on-chain.
//!
//! Creators opt in by calling `init_loyalty_program`. Once active, every
//! completed payment to one of their invoices credits the payer with
//! loyalty points proportional to the payment amount:
//!
//! ```text
//! points = amount * points_per_unit / POINTS_SCALE
//! ```
//!
//! where `POINTS_SCALE = 1_000_000`. A points-per-unit rate of 1_000_000
//! means 1 point per token unit; 500_000 means 0.5 points per token unit.
//!
//! Payers can redeem accumulated points for a discount on future payments to
//! the same creator. The discount is capped at `max_discount_bps` (max 5_000
//! = 50%) and must be explicitly enabled per invoice by the creator via
//! `enable_loyalty_discount`. Redemption deducts points and reduces the
//! effective payment amount.
//!
//! ## Entry points
//! - `init_loyalty_program(creator, points_per_unit, max_discount_bps)` — creator sets up.
//! - `update_loyalty_program(creator, points_per_unit, max_discount_bps)` — creator updates.
//! - `enable_loyalty_discount(creator, invoice_id)` — creator opts invoice in.
//! - `credit_loyalty_points(creator, payer, invoice_id, amount)` — internal; also public so
//!   admin/integration can call it.
//! - `redeem_loyalty_points(payer, creator, points_to_redeem)` — burn points, returns discount amount.
//! - `get_loyalty_program(creator)` — returns `Option<LoyaltyProgram>`.
//! - `get_loyalty_points(creator, payer)` — returns accumulated points.
//! - `is_loyalty_discount_enabled(invoice_id)` — returns bool.
//!
//! ## Events
//! - `(split, loy_init)` — data: `creator` — program initialised
//! - `(split, loy_upd)` — data: `creator` — program updated
//! - `(split, loy_crd, invoice_id)` — data: `(payer, points)` — points credited
//! - `(split, loy_rdm)` — data: `(payer, creator, points, discount)` — points redeemed
//!
//! ## Storage
//! Uses a local `LoyaltyKey` enum.

use super::*;
use soroban_sdk::{contractimpl, contracttype, symbol_short, Address, Env};

/// Scale factor for `points_per_unit` (1_000_000 = 1 point per token unit).
pub const POINTS_SCALE: i128 = 1_000_000;
/// Maximum discount basis points (50%).
pub const MAX_LOYALTY_DISCOUNT_BPS: u32 = 5_000;

/// A creator's loyalty program configuration.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct LoyaltyProgram {
    pub creator: Address,
    /// Points awarded per `POINTS_SCALE` token units paid.
    pub points_per_unit: u32,
    /// Maximum discount in basis points (max 5_000 = 50%).
    pub max_discount_bps: u32,
    /// Whether the program is currently active.
    pub active: bool,
}

/// Persistent storage keys for the brand loyalty module.
#[contracttype]
#[derive(Clone)]
pub enum LoyaltyKey {
    /// `LoyaltyProgram` config for a creator.
    Program(Address),
    /// `i128` accumulated points for `(creator, payer)`.
    Points(Address, Address),
    /// `bool` flag: loyalty discount enabled for `invoice_id`.
    DiscountEnabled(u64),
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn load_program(env: &Env, creator: &Address) -> Option<LoyaltyProgram> {
    env.storage()
        .persistent()
        .get(&LoyaltyKey::Program(creator.clone()))
}

fn load_points(env: &Env, creator: &Address, payer: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&LoyaltyKey::Points(creator.clone(), payer.clone()))
        .unwrap_or(0i128)
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

#[contractimpl]
impl SplitContract {
    /// Initialise a loyalty program for `creator`.
    ///
    /// - `points_per_unit` must be ≥ 1.
    /// - `max_discount_bps` must be in `[1, 5_000]`.
    pub fn init_loyalty_program(
        env: Env,
        creator: Address,
        points_per_unit: u32,
        max_discount_bps: u32,
    ) {
        require_not_paused(&env);
        creator.require_auth();

        assert!(points_per_unit >= 1, "points_per_unit must be >= 1");
        assert!(
            max_discount_bps >= 1 && max_discount_bps <= MAX_LOYALTY_DISCOUNT_BPS,
            "max_discount_bps out of range [1, 5000]"
        );
        assert!(
            load_program(&env, &creator).is_none(),
            "loyalty program already initialised; use update"
        );

        let program = LoyaltyProgram {
            creator: creator.clone(),
            points_per_unit,
            max_discount_bps,
            active: true,
        };
        env.storage()
            .persistent()
            .set(&LoyaltyKey::Program(creator.clone()), &program);

        env.events().publish(
            (symbol_short!("split"), symbol_short!("loy_init")),
            creator,
        );
    }

    /// Update an existing loyalty program (creator only). The program must
    /// already exist.
    pub fn update_loyalty_program(
        env: Env,
        creator: Address,
        points_per_unit: u32,
        max_discount_bps: u32,
    ) {
        require_not_paused(&env);
        creator.require_auth();

        assert!(points_per_unit >= 1, "points_per_unit must be >= 1");
        assert!(
            max_discount_bps >= 1 && max_discount_bps <= MAX_LOYALTY_DISCOUNT_BPS,
            "max_discount_bps out of range [1, 5000]"
        );

        let mut program = load_program(&env, &creator).expect("no loyalty program found");
        program.points_per_unit = points_per_unit;
        program.max_discount_bps = max_discount_bps;
        env.storage()
            .persistent()
            .set(&LoyaltyKey::Program(creator.clone()), &program);

        env.events().publish(
            (symbol_short!("split"), symbol_short!("loy_upd")),
            creator,
        );
    }

    /// Enable loyalty discounts for a specific invoice (creator only).
    /// The creator must have an active loyalty program.
    pub fn enable_loyalty_discount(env: Env, creator: Address, invoice_id: u64) {
        require_not_paused(&env);
        creator.require_auth();

        let invoice = load_invoice(&env, invoice_id);
        assert!(invoice.creator == creator, "only creator can enable discount");
        assert!(
            load_program(&env, &creator).is_some(),
            "no loyalty program configured"
        );
        env.storage()
            .persistent()
            .set(&LoyaltyKey::DiscountEnabled(invoice_id), &true);
    }

    /// Credit loyalty points to `payer` for a payment of `amount` to
    /// `creator`'s invoice. No-op if the creator has no active program.
    ///
    /// This is a standalone public entry point so integrators can call it
    /// after a `pay`; in a production setup this would be hooked into `_pay`.
    pub fn credit_loyalty_points(
        env: Env,
        creator: Address,
        payer: Address,
        invoice_id: u64,
        amount: i128,
    ) {
        require_not_paused(&env);
        creator.require_auth();

        let program = match load_program(&env, &creator) {
            Some(p) if p.active => p,
            _ => return,
        };
        let invoice = load_invoice(&env, invoice_id);
        assert!(invoice.creator == creator, "invoice belongs to a different creator");
        assert!(amount > 0, "amount must be positive");

        let points = amount * program.points_per_unit as i128 / POINTS_SCALE;
        if points <= 0 {
            return;
        }

        let prev = load_points(&env, &creator, &payer);
        env.storage().persistent().set(
            &LoyaltyKey::Points(creator.clone(), payer.clone()),
            &(prev + points),
        );

        env.events().publish(
            (symbol_short!("split"), symbol_short!("loy_crd"), invoice_id),
            (payer, points),
        );
    }

    /// Redeem `points_to_redeem` loyalty points for a token-unit discount.
    ///
    /// Returns the discount amount (in token units):
    /// ```text
    /// discount = points_to_redeem * POINTS_SCALE / program.points_per_unit
    /// ```
    /// capped at `program.max_discount_bps / 10_000` of `points_to_redeem *
    /// POINTS_SCALE / program.points_per_unit`.
    ///
    /// The caller must have at least `points_to_redeem` points for `creator`.
    pub fn redeem_loyalty_points(
        env: Env,
        payer: Address,
        creator: Address,
        points_to_redeem: i128,
    ) -> i128 {
        require_not_paused(&env);
        payer.require_auth();

        assert!(points_to_redeem > 0, "points_to_redeem must be positive");

        let program = load_program(&env, &creator).expect("no loyalty program found");
        let balance = load_points(&env, &creator, &payer);
        assert!(balance >= points_to_redeem, "insufficient loyalty points");

        // Raw discount in token units before cap.
        let raw_discount = points_to_redeem * POINTS_SCALE / program.points_per_unit as i128;
        // Apply max_discount_bps cap: discount ≤ raw_discount * max_discount_bps / 10_000.
        let discount = raw_discount * program.max_discount_bps as i128 / 10_000;

        let new_balance = balance - points_to_redeem;
        env.storage().persistent().set(
            &LoyaltyKey::Points(creator.clone(), payer.clone()),
            &new_balance,
        );

        env.events().publish(
            (symbol_short!("split"), symbol_short!("loy_rdm")),
            (payer, creator, points_to_redeem, discount),
        );

        discount
    }

    /// Returns the loyalty program config for `creator`, or `None`.
    pub fn get_loyalty_program(env: Env, creator: Address) -> Option<LoyaltyProgram> {
        load_program(&env, &creator)
    }

    /// Returns the accumulated loyalty points `payer` has earned from
    /// `creator`'s invoices.
    pub fn get_loyalty_points(env: Env, creator: Address, payer: Address) -> i128 {
        load_points(&env, &creator, &payer)
    }

    /// Returns `true` if the loyalty discount is enabled for `invoice_id`.
    pub fn is_loyalty_discount_enabled(env: Env, invoice_id: u64) -> bool {
        env.storage()
            .persistent()
            .get(&LoyaltyKey::DiscountEnabled(invoice_id))
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ext_test_util::{fixture, mint, new_invoice, pay};
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::Address;

    #[test]
    fn init_and_get_program() {
        let f = fixture();
        let creator = Address::generate(&f.env);
        f.c.init_loyalty_program(&creator, &1_000_000u32, &500u32);
        let prog = f.c.get_loyalty_program(&creator).unwrap();
        assert_eq!(prog.points_per_unit, 1_000_000u32);
        assert_eq!(prog.max_discount_bps, 500u32);
        assert!(prog.active);
    }

    #[test]
    #[should_panic(expected = "loyalty program already initialised")]
    fn double_init_rejected() {
        let f = fixture();
        let creator = Address::generate(&f.env);
        f.c.init_loyalty_program(&creator, &1_000_000u32, &500u32);
        f.c.init_loyalty_program(&creator, &2_000_000u32, &500u32);
    }

    #[test]
    fn update_program() {
        let f = fixture();
        let creator = Address::generate(&f.env);
        f.c.init_loyalty_program(&creator, &1_000_000u32, &500u32);
        f.c.update_loyalty_program(&creator, &2_000_000u32, &1_000u32);
        let prog = f.c.get_loyalty_program(&creator).unwrap();
        assert_eq!(prog.points_per_unit, 2_000_000u32);
        assert_eq!(prog.max_discount_bps, 1_000u32);
    }

    #[test]
    fn credit_and_read_points() {
        let f = fixture();
        let (id, creator, _) = new_invoice(&f, 1_000);
        let payer = Address::generate(&f.env);
        mint(&f.env, &f.token, &payer, 500);
        pay(&f, &payer, id, 500);
        // 1 point per unit, amount=500 → 500 points.
        f.c.init_loyalty_program(&creator, &1_000_000u32, &500u32);
        f.c.credit_loyalty_points(&creator, &payer, &id, &500i128);
        assert_eq!(f.c.get_loyalty_points(&creator, &payer), 500i128);
    }

    #[test]
    fn redeem_points() {
        let f = fixture();
        let (id, creator, _) = new_invoice(&f, 1_000);
        let payer = Address::generate(&f.env);
        mint(&f.env, &f.token, &payer, 500);
        pay(&f, &payer, id, 500);
        f.c.init_loyalty_program(&creator, &1_000_000u32, &5_000u32);
        f.c.credit_loyalty_points(&creator, &payer, &id, &1_000_000i128);
        // 1_000_000 points * SCALE / ppu = 1_000_000 raw; capped at 50% = 500_000
        let discount = f.c.redeem_loyalty_points(&payer, &creator, &1_000_000i128);
        assert_eq!(discount, 500_000i128);
        assert_eq!(f.c.get_loyalty_points(&creator, &payer), 0i128);
    }

    #[test]
    #[should_panic(expected = "insufficient loyalty points")]
    fn redeem_more_than_balance_rejected() {
        let f = fixture();
        let (id, creator, _) = new_invoice(&f, 100);
        let payer = Address::generate(&f.env);
        mint(&f.env, &f.token, &payer, 50);
        pay(&f, &payer, id, 50);
        f.c.init_loyalty_program(&creator, &1_000_000u32, &500u32);
        f.c.credit_loyalty_points(&creator, &payer, &id, &50i128);
        f.c.redeem_loyalty_points(&payer, &creator, &1_000i128);
    }

    #[test]
    fn enable_loyalty_discount_flag() {
        let f = fixture();
        let (id, creator, _) = new_invoice(&f, 100);
        f.c.init_loyalty_program(&creator, &1_000_000u32, &500u32);
        assert!(!f.c.is_loyalty_discount_enabled(&id));
        f.c.enable_loyalty_discount(&creator, &id);
        assert!(f.c.is_loyalty_discount_enabled(&id));
    }

    #[test]
    fn default_points_are_zero() {
        let f = fixture();
        let creator = Address::generate(&f.env);
        let payer = Address::generate(&f.env);
        assert_eq!(f.c.get_loyalty_points(&creator, &payer), 0i128);
    }

    #[test]
    #[should_panic(expected = "max_discount_bps out of range")]
    fn excessive_discount_bps_rejected() {
        let f = fixture();
        let creator = Address::generate(&f.env);
        f.c.init_loyalty_program(&creator, &1_000_000u32, &6_000u32);
    }
}
