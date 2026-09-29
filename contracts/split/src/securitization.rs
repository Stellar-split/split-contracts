//! Issue #861: Invoice securitization for tokenization.
//!
//! A recipient (the *originator*) can sell the receivables they are owed on
//! one or more `Pending` invoices as tokenized units in two tranches:
//!
//! - **Senior** units are repaid first, up to principal plus a fixed coupon
//!   (`units * unit_price * (10_000 + senior_coupon_bps) / 10_000`).
//! - **Junior** units receive everything collected beyond the senior target.
//!
//! Lifecycle:
//!
//! 1. `create_securitization` — originator lists the invoices, unit price and
//!    units per tranche. The offering (`units * unit_price`) may not exceed
//!    the face value of the originator's share of those invoices.
//!    Status: `Offering`.
//! 2. `buy_securitization_units` — investors buy units; the purchase price is
//!    paid straight to the originator (upfront liquidity).
//! 3. `activate_securitization` — originator closes the offering; unsold units
//!    are dropped. Status: `Active`.
//! 4. `deposit_securitization_funds` — collections (normally the
//!    originator's invoice payouts) are paid into the pool and distributed by
//!    the waterfall. Holders pull their share with
//!    `claim_securitization_payout`.
//! 5. `close_securitization` — stops further deposits. The originator may close
//!    at any time; anyone may close once every underlying invoice is final.
//!    Holders can still claim afterwards.
//!
//! Units are transferable at any stage with `transfer_securitization_units`.
//! Per-holder entitlements use a cumulative per-unit accumulator, so transfers
//! settle what each side has accrued before units move.

use crate::*;
use soroban_sdk::{contractimpl, contracttype, symbol_short, token, Address, Env, Symbol, Vec};

/// Maximum invoices backing one securitization pool.
const MAX_SECURITIZED_INVOICES: u32 = 10;

/// Fixed-point scale for the per-unit payout accumulator.
const SEC_ACC_SCALE: i128 = 1_000_000_000_000;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecTranche {
    Senior,
    Junior,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecStatus {
    Offering,
    Active,
    Closed,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecuritizationPool {
    pub id: u64,
    pub originator: Address,
    pub token: Address,
    pub invoice_ids: Vec<u64>,
    /// Sum of the originator's amounts across the backing invoices.
    pub face_value: i128,
    pub unit_price: i128,
    pub senior_units: u64,
    pub junior_units: u64,
    pub senior_sold: u64,
    pub junior_sold: u64,
    pub senior_coupon_bps: u32,
    /// Total collections deposited into the pool.
    pub collected: i128,
    pub senior_claimed: i128,
    pub junior_claimed: i128,
    pub status: SecStatus,
    pub created_at: u64,
}

/// A holder's units in one tranche of one pool.
#[contracttype]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SecHolding {
    pub units: u64,
    /// Accrued entitlement already accounted for (accumulator snapshot).
    pub reward_debt: i128,
    /// Settled but unclaimed payout.
    pub owed: i128,
    pub claimed: i128,
}

/// Current split of collections between the tranches.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecWaterfall {
    pub senior_target: i128,
    pub senior_pool: i128,
    pub junior_pool: i128,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

/// Instance storage: last issued pool ID.
fn sec_counter_key() -> Symbol {
    symbol_short!("sec_ctr")
}

/// Persistent storage: pool ID → `SecuritizationPool`.
fn sec_pool_key(pool_id: u64) -> (Symbol, u64) {
    (symbol_short!("sec_pool"), pool_id)
}

/// Persistent storage: (pool ID, tranche, holder) → `SecHolding`.
fn sec_holding_key(pool_id: u64, tranche: SecTranche, holder: &Address) -> (Symbol, u64, SecTranche, Address) {
    (symbol_short!("sec_hold"), pool_id, tranche, holder.clone())
}

/// Persistent storage: (invoice ID, originator) → pool ID. Prevents the same
/// receivable from backing two pools.
fn sec_invoice_key(invoice_id: u64, originator: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("sec_inv"), invoice_id, originator.clone())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn load_pool(env: &Env, pool_id: u64) -> SecuritizationPool {
    env.storage()
        .persistent()
        .get(&sec_pool_key(pool_id))
        .expect("securitization not found")
}

fn save_pool(env: &Env, pool: &SecuritizationPool) {
    env.storage().persistent().set(&sec_pool_key(pool.id), pool);
}

fn load_holding(env: &Env, pool_id: u64, tranche: SecTranche, holder: &Address) -> SecHolding {
    env.storage()
        .persistent()
        .get(&sec_holding_key(pool_id, tranche, holder))
        .unwrap_or_default()
}

fn save_holding(env: &Env, pool_id: u64, tranche: SecTranche, holder: &Address, holding: &SecHolding) {
    env.storage()
        .persistent()
        .set(&sec_holding_key(pool_id, tranche, holder), holding);
}

/// Split collections between tranches. When one tranche sold no units the
/// other receives everything.
pub(crate) fn sec_waterfall(pool: &SecuritizationPool) -> SecWaterfall {
    let principal = (pool.senior_sold as i128)
        .checked_mul(pool.unit_price)
        .expect("senior principal overflow");
    let senior_target = principal
        .checked_mul(10_000 + pool.senior_coupon_bps as i128)
        .expect("senior target overflow")
        / 10_000;
    let senior_pool = if pool.junior_sold == 0 {
        pool.collected
    } else if pool.senior_sold == 0 {
        0
    } else {
        pool.collected.min(senior_target)
    };
    SecWaterfall {
        senior_target,
        senior_pool,
        junior_pool: pool.collected - senior_pool,
    }
}

/// Cumulative payout per unit for `tranche`, scaled by `SEC_ACC_SCALE`.
fn acc_per_unit(pool: &SecuritizationPool, tranche: SecTranche) -> i128 {
    let waterfall = sec_waterfall(pool);
    let (tranche_pool, sold) = match tranche {
        SecTranche::Senior => (waterfall.senior_pool, pool.senior_sold),
        SecTranche::Junior => (waterfall.junior_pool, pool.junior_sold),
    };
    if sold == 0 {
        return 0;
    }
    tranche_pool
        .checked_mul(SEC_ACC_SCALE)
        .expect("accumulator overflow")
        / sold as i128
}

fn accrued(units: u64, acc: i128) -> i128 {
    (units as i128)
        .checked_mul(acc)
        .expect("accrual overflow")
        / SEC_ACC_SCALE
}

/// Move newly accrued entitlement into `owed` and snapshot the accumulator.
fn settle_holding(holding: &mut SecHolding, acc: i128) {
    let now_accrued = accrued(holding.units, acc);
    holding.owed += now_accrued - holding.reward_debt;
    holding.reward_debt = now_accrued;
}

/// Remaining undistributed amount in `tranche`'s share of collections.
fn tranche_unclaimed(pool: &SecuritizationPool, tranche: SecTranche) -> i128 {
    let waterfall = sec_waterfall(pool);
    match tranche {
        SecTranche::Senior => waterfall.senior_pool - pool.senior_claimed,
        SecTranche::Junior => waterfall.junior_pool - pool.junior_claimed,
    }
}

fn is_invoice_final(status: &InvoiceStatus) -> bool {
    !matches!(
        status,
        InvoiceStatus::Pending | InvoiceStatus::Disputed | InvoiceStatus::PartiallyReleased
    )
}

// ---------------------------------------------------------------------------
// Contract entry points
// ---------------------------------------------------------------------------

#[contractimpl]
impl SplitContract {
    /// Create a securitization pool over the originator's receivables on
    /// `invoice_ids`. The originator must be a recipient on every invoice and
    /// each invoice must be `Pending` and funded in `token`. Returns the pool ID.
    pub fn create_securitization(
        env: Env,
        originator: Address,
        invoice_ids: Vec<u64>,
        token: Address,
        unit_price: i128,
        senior_units: u64,
        junior_units: u64,
        senior_coupon_bps: u32,
    ) -> u64 {
        require_not_paused(&env);
        originator.require_auth();
        assert!(
            !invoice_ids.is_empty() && invoice_ids.len() <= MAX_SECURITIZED_INVOICES,
            "invalid number of invoices"
        );
        assert!(unit_price > 0, "unit price must be positive");
        assert!(senior_units + junior_units > 0, "no units offered");
        assert!(senior_coupon_bps <= 10_000, "senior coupon must be <= 10000 bps");

        let mut face_value: i128 = 0;
        for (i, invoice_id) in invoice_ids.iter().enumerate() {
            assert!(
                invoice_ids.first_index_of(invoice_id) == Some(i as u32),
                "duplicate invoice"
            );
            let invoice = load_invoice(&env, invoice_id);
            assert!(
                invoice.status == InvoiceStatus::Pending,
                "invoice must be pending"
            );
            assert!(invoice.funding_token == token, "invoice token mismatch");
            assert!(
                !env.storage()
                    .persistent()
                    .has(&sec_invoice_key(invoice_id, &originator)),
                "receivable already securitized"
            );
            let idx = invoice
                .recipients
                .first_index_of(&originator)
                .expect("originator is not a recipient on invoice");
            let share = invoice.amounts.get(idx).expect("recipient amount missing");
            face_value = face_value
                .checked_add(share)
                .expect("face value overflow");
        }

        let offering = ((senior_units + junior_units) as i128)
            .checked_mul(unit_price)
            .expect("offering size overflow");
        assert!(offering <= face_value, "offering exceeds face value");

        let pool_id: u64 = env
            .storage()
            .instance()
            .get(&sec_counter_key())
            .unwrap_or(0u64)
            + 1;
        env.storage().instance().set(&sec_counter_key(), &pool_id);

        for invoice_id in invoice_ids.iter() {
            env.storage()
                .persistent()
                .set(&sec_invoice_key(invoice_id, &originator), &pool_id);
        }

        let pool = SecuritizationPool {
            id: pool_id,
            originator: originator.clone(),
            token,
            invoice_ids,
            face_value,
            unit_price,
            senior_units,
            junior_units,
            senior_sold: 0,
            junior_sold: 0,
            senior_coupon_bps,
            collected: 0,
            senior_claimed: 0,
            junior_claimed: 0,
            status: SecStatus::Offering,
            created_at: env.ledger().timestamp(),
        };
        save_pool(&env, &pool);

        events::securitization_created(&env, pool_id, &originator, face_value, offering);
        pool_id
    }

    /// Buy `units` of `tranche` during the offering. The cost
    /// (`units * unit_price`) is paid directly to the originator. Returns the cost.
    pub fn buy_securitization_units(
        env: Env,
        investor: Address,
        pool_id: u64,
        tranche: SecTranche,
        units: u64,
    ) -> i128 {
        require_not_paused(&env);
        investor.require_auth();
        assert!(units > 0, "units must be positive");

        let mut pool = load_pool(&env, pool_id);
        assert!(pool.status == SecStatus::Offering, "offering is closed");
        assert!(investor != pool.originator, "originator cannot buy own units");
        let (offered, sold) = match tranche {
            SecTranche::Senior => (pool.senior_units, pool.senior_sold),
            SecTranche::Junior => (pool.junior_units, pool.junior_sold),
        };
        assert!(sold + units <= offered, "not enough units available");

        let cost = (units as i128)
            .checked_mul(pool.unit_price)
            .expect("purchase cost overflow");
        token::Client::new(&env, &pool.token).transfer(&investor, &pool.originator, &cost);

        match tranche {
            SecTranche::Senior => pool.senior_sold += units,
            SecTranche::Junior => pool.junior_sold += units,
        }
        save_pool(&env, &pool);

        // No collections accrue during the offering, so the accumulator is zero
        // and the holding needs no settlement.
        let mut holding = load_holding(&env, pool_id, tranche, &investor);
        holding.units += units;
        save_holding(&env, pool_id, tranche, &investor, &holding);

        events::securitization_units_bought(&env, pool_id, &investor, tranche, units, cost);
        cost
    }

    /// Close the offering and start distributing collections. Unsold units are
    /// dropped. At least one unit must have been sold.
    pub fn activate_securitization(env: Env, originator: Address, pool_id: u64) {
        require_not_paused(&env);
        originator.require_auth();
        let mut pool = load_pool(&env, pool_id);
        assert!(pool.originator == originator, "only the originator may activate");
        assert!(pool.status == SecStatus::Offering, "securitization is not offering");
        assert!(pool.senior_sold + pool.junior_sold > 0, "no units sold");

        pool.senior_units = pool.senior_sold;
        pool.junior_units = pool.junior_sold;
        pool.status = SecStatus::Active;
        save_pool(&env, &pool);

        events::securitization_activated(&env, pool_id, pool.senior_sold, pool.junior_sold);
    }

    /// Pay collections into an active pool for distribution to holders.
    pub fn deposit_securitization_funds(env: Env, depositor: Address, pool_id: u64, amount: i128) {
        require_not_paused(&env);
        depositor.require_auth();
        assert!(amount > 0, "amount must be positive");

        let mut pool = load_pool(&env, pool_id);
        assert!(pool.status == SecStatus::Active, "securitization is not active");

        token::Client::new(&env, &pool.token).transfer(
            &depositor,
            &env.current_contract_address(),
            &amount,
        );
        pool.collected = pool
            .collected
            .checked_add(amount)
            .expect("collections overflow");
        save_pool(&env, &pool);

        let waterfall = sec_waterfall(&pool);
        events::securitization_collected(
            &env,
            pool_id,
            &depositor,
            amount,
            waterfall.senior_pool,
            waterfall.junior_pool,
        );
    }

    /// Transfer `units` of `tranche` from `from` to `to`. Accrued payouts stay
    /// with the side that earned them.
    pub fn transfer_securitization_units(
        env: Env,
        from: Address,
        to: Address,
        pool_id: u64,
        tranche: SecTranche,
        units: u64,
    ) {
        require_not_paused(&env);
        from.require_auth();
        assert!(units > 0, "units must be positive");
        assert!(from != to, "cannot transfer to self");

        let pool = load_pool(&env, pool_id);
        let acc = acc_per_unit(&pool, tranche);

        let mut sender = load_holding(&env, pool_id, tranche, &from);
        assert!(sender.units >= units, "insufficient units");
        let mut receiver = load_holding(&env, pool_id, tranche, &to);

        settle_holding(&mut sender, acc);
        settle_holding(&mut receiver, acc);
        sender.units -= units;
        receiver.units += units;
        sender.reward_debt = accrued(sender.units, acc);
        receiver.reward_debt = accrued(receiver.units, acc);

        save_holding(&env, pool_id, tranche, &from, &sender);
        save_holding(&env, pool_id, tranche, &to, &receiver);

        events::securitization_units_transferred(&env, pool_id, &from, &to, tranche, units);
    }

    /// Claim everything owed to `holder` in `tranche`. Returns the amount paid.
    pub fn claim_securitization_payout(env: Env, holder: Address, pool_id: u64, tranche: SecTranche) -> i128 {
        require_not_paused(&env);
        holder.require_auth();

        let mut pool = load_pool(&env, pool_id);
        assert!(pool.status != SecStatus::Offering, "securitization not active");

        let mut holding = load_holding(&env, pool_id, tranche, &holder);
        settle_holding(&mut holding, acc_per_unit(&pool, tranche));
        // Guard against accumulated rounding: never pay out more than the
        // tranche has left.
        let payout = holding.owed.min(tranche_unclaimed(&pool, tranche));
        assert!(payout > 0, "nothing to claim");

        holding.owed -= payout;
        holding.claimed += payout;
        save_holding(&env, pool_id, tranche, &holder, &holding);
        match tranche {
            SecTranche::Senior => pool.senior_claimed += payout,
            SecTranche::Junior => pool.junior_claimed += payout,
        }
        save_pool(&env, &pool);

        token::Client::new(&env, &pool.token).transfer(
            &env.current_contract_address(),
            &holder,
            &payout,
        );

        events::securitization_payout_claimed(&env, pool_id, &holder, tranche, payout);
        payout
    }

    /// Stop accepting collections. The originator may close at any time;
    /// anyone may close once every backing invoice is final.
    pub fn close_securitization(env: Env, caller: Address, pool_id: u64) {
        caller.require_auth();
        let mut pool = load_pool(&env, pool_id);
        assert!(pool.status == SecStatus::Active, "securitization is not active");
        if caller != pool.originator {
            for invoice_id in pool.invoice_ids.iter() {
                let invoice = load_invoice(&env, invoice_id);
                assert!(
                    is_invoice_final(&invoice.status),
                    "backing invoices are not final"
                );
            }
        }

        pool.status = SecStatus::Closed;
        save_pool(&env, &pool);

        events::securitization_closed(&env, pool_id, &caller, pool.collected);
    }

    /// Return a securitization pool by ID.
    pub fn get_securitization(env: Env, pool_id: u64) -> SecuritizationPool {
        load_pool(&env, pool_id)
    }

    /// Return a holder's position in one tranche (zeroed if none).
    pub fn get_securitization_holding(env: Env, pool_id: u64, tranche: SecTranche, holder: Address) -> SecHolding {
        load_holding(&env, pool_id, tranche, &holder)
    }

    /// Amount `holder` could claim from `tranche` right now.
    pub fn get_claimable_securitization(env: Env, pool_id: u64, tranche: SecTranche, holder: Address) -> i128 {
        let pool = load_pool(&env, pool_id);
        if pool.status == SecStatus::Offering {
            return 0;
        }
        let mut holding = load_holding(&env, pool_id, tranche, &holder);
        settle_holding(&mut holding, acc_per_unit(&pool, tranche));
        holding.owed.min(tranche_unclaimed(&pool, tranche))
    }

    /// Current senior/junior split of collections.
    pub fn get_securitization_waterfall(env: Env, pool_id: u64) -> SecWaterfall {
        sec_waterfall(&load_pool(&env, pool_id))
    }

    /// Pool backing the originator's receivable on `invoice_id`, if any.
    pub fn get_invoice_securitization(env: Env, invoice_id: u64, originator: Address) -> Option<u64> {
        env.storage()
            .persistent()
            .get(&sec_invoice_key(invoice_id, &originator))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test {
    use super::*;
    use crate::test::{client, make_invoice, setup_initialized};
    use soroban_sdk::testutils::{Address as _, Events as _, Ledger};
    use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
    use soroban_sdk::vec;

    struct Ctx {
        env: Env,
        contract_id: Address,
        token_id: Address,
        originator: Address,
        invoice_id: u64,
        pool_id: u64,
    }

    /// Originator is owed 1_000 on one invoice and offers 50 senior + 40 junior
    /// units at 10 each (900 raise) with a 10% senior coupon.
    fn ctx() -> Ctx {
        let (env, contract_id, token_id) = setup_initialized();
        let c = client(&env, &contract_id);
        env.ledger().set_timestamp(1_000);
        let creator = Address::generate(&env);
        let originator = Address::generate(&env);
        let invoice_id = make_invoice(&env, &c, &creator, &originator, 1_000, &token_id, 100_000);
        let pool_id = c.create_securitization(
            &originator,
            &vec![&env, invoice_id],
            &token_id,
            &10,
            &50,
            &40,
            &1_000,
        );
        Ctx {
            env,
            contract_id,
            token_id,
            originator,
            invoice_id,
            pool_id,
        }
    }

    fn investor(t: &Ctx, balance: i128) -> Address {
        let a = Address::generate(&t.env);
        StellarAssetClient::new(&t.env, &t.token_id).mint(&a, &balance);
        a
    }

    fn fund_and_deposit(t: &Ctx, amount: i128) {
        let depositor = investor(t, amount);
        client(&t.env, &t.contract_id).deposit_securitization_funds(&depositor, &t.pool_id, &amount);
    }

    fn sample_pool(env: &Env, senior_sold: u64, junior_sold: u64, collected: i128) -> SecuritizationPool {
        SecuritizationPool {
            id: 1,
            originator: Address::generate(env),
            token: Address::generate(env),
            invoice_ids: Vec::new(env),
            face_value: 0,
            unit_price: 10,
            senior_units: senior_sold,
            junior_units: junior_sold,
            senior_sold,
            junior_sold,
            senior_coupon_bps: 1_000,
            collected,
            senior_claimed: 0,
            junior_claimed: 0,
            status: SecStatus::Active,
            created_at: 0,
        }
    }

    #[test]
    fn waterfall_fills_senior_then_junior() {
        let env = Env::default();
        let w = sec_waterfall(&sample_pool(&env, 50, 40, 300));
        assert_eq!((w.senior_target, w.senior_pool, w.junior_pool), (550, 300, 0));

        let w = sec_waterfall(&sample_pool(&env, 50, 40, 1_000));
        assert_eq!((w.senior_pool, w.junior_pool), (550, 450));

        // Single-tranche pools take everything.
        let w = sec_waterfall(&sample_pool(&env, 50, 0, 1_000));
        assert_eq!((w.senior_pool, w.junior_pool), (1_000, 0));
        let w = sec_waterfall(&sample_pool(&env, 0, 40, 1_000));
        assert_eq!((w.senior_pool, w.junior_pool), (0, 1_000));
    }

    #[test]
    fn create_records_pool_and_locks_receivable() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        let pool = c.get_securitization(&t.pool_id);
        assert_eq!(pool.face_value, 1_000);
        assert_eq!(pool.status, SecStatus::Offering);
        assert_eq!(
            c.get_invoice_securitization(&t.invoice_id, &t.originator),
            Some(t.pool_id)
        );
        assert!(!t.env.events().all().is_empty());
    }

    #[test]
    #[should_panic(expected = "receivable already securitized")]
    fn receivable_cannot_back_two_pools() {
        let t = ctx();
        client(&t.env, &t.contract_id).create_securitization(
            &t.originator,
            &vec![&t.env, t.invoice_id],
            &t.token_id,
            &1,
            &1,
            &0,
            &0,
        );
    }

    #[test]
    #[should_panic(expected = "offering exceeds face value")]
    fn offering_cannot_exceed_face_value() {
        let (env, contract_id, token_id) = setup_initialized();
        let c = client(&env, &contract_id);
        env.ledger().set_timestamp(1_000);
        let creator = Address::generate(&env);
        let originator = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &originator, 100, &token_id, 100_000);
        c.create_securitization(&originator, &vec![&env, id], &token_id, &10, &6, &5, &0);
    }

    #[test]
    #[should_panic(expected = "originator is not a recipient on invoice")]
    fn originator_must_be_recipient() {
        let t = ctx();
        let stranger = Address::generate(&t.env);
        client(&t.env, &t.contract_id).create_securitization(
            &stranger,
            &vec![&t.env, t.invoice_id],
            &t.token_id,
            &1,
            &1,
            &0,
            &0,
        );
    }

    #[test]
    fn buying_pays_originator_upfront() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        let tk = TokenClient::new(&t.env, &t.token_id);
        let senior = investor(&t, 1_000);

        let cost = c.buy_securitization_units(&senior, &t.pool_id, &SecTranche::Senior, &50);

        assert_eq!(cost, 500);
        assert_eq!(tk.balance(&t.originator), 500);
        assert_eq!(tk.balance(&senior), 500);
        assert_eq!(c.get_securitization(&t.pool_id).senior_sold, 50);
        assert_eq!(
            c.get_securitization_holding(&t.pool_id, &SecTranche::Senior, &senior).units,
            50
        );
    }

    #[test]
    #[should_panic(expected = "not enough units available")]
    fn cannot_oversubscribe_tranche() {
        let t = ctx();
        let a = investor(&t, 1_000);
        client(&t.env, &t.contract_id).buy_securitization_units(&a, &t.pool_id, &SecTranche::Junior, &41);
    }

    #[test]
    #[should_panic(expected = "securitization is not active")]
    fn deposits_require_activation() {
        let t = ctx();
        fund_and_deposit(&t, 100);
    }

    #[test]
    fn activation_drops_unsold_units() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        let a = investor(&t, 1_000);
        c.buy_securitization_units(&a, &t.pool_id, &SecTranche::Junior, &10);
        c.activate_securitization(&t.originator, &t.pool_id);

        let pool = c.get_securitization(&t.pool_id);
        assert_eq!(pool.status, SecStatus::Active);
        assert_eq!((pool.senior_units, pool.junior_units), (0, 10));
    }

    #[test]
    fn end_to_end_waterfall_distribution() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        let tk = TokenClient::new(&t.env, &t.token_id);
        let senior = investor(&t, 500);
        let j1 = investor(&t, 300);
        let j2 = investor(&t, 100);
        c.buy_securitization_units(&senior, &t.pool_id, &SecTranche::Senior, &50);
        c.buy_securitization_units(&j1, &t.pool_id, &SecTranche::Junior, &30);
        c.buy_securitization_units(&j2, &t.pool_id, &SecTranche::Junior, &10);
        c.activate_securitization(&t.originator, &t.pool_id);

        // The invoice pays out to the originator, who passes collections through.
        let payer = investor(&t, 1_000);
        c.pay(&payer, &t.invoice_id, &1_000_i128, &0_u64, &false, &false, &None);
        assert_eq!(tk.balance(&t.originator), 1_900);
        c.deposit_securitization_funds(&t.originator, &t.pool_id, &1_000);

        assert_eq!(c.claim_securitization_payout(&senior, &t.pool_id, &SecTranche::Senior), 550);
        assert_eq!(c.claim_securitization_payout(&j1, &t.pool_id, &SecTranche::Junior), 337);
        assert_eq!(c.claim_securitization_payout(&j2, &t.pool_id, &SecTranche::Junior), 112);
        assert_eq!(tk.balance(&senior), 550);

        // Invoice is final, so anyone may close the pool.
        c.close_securitization(&payer, &t.pool_id);
        assert_eq!(c.get_securitization(&t.pool_id).status, SecStatus::Closed);
    }

    #[test]
    fn transfers_keep_accrued_payouts_with_earner() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        let senior = investor(&t, 500);
        let a = investor(&t, 300);
        let b = investor(&t, 100);
        let newcomer = Address::generate(&t.env);
        c.buy_securitization_units(&senior, &t.pool_id, &SecTranche::Senior, &50);
        c.buy_securitization_units(&a, &t.pool_id, &SecTranche::Junior, &30);
        c.buy_securitization_units(&b, &t.pool_id, &SecTranche::Junior, &10);
        c.activate_securitization(&t.originator, &t.pool_id);

        // 600 collected: senior 550, junior 50 (1.25 per unit).
        fund_and_deposit(&t, 600);
        c.transfer_securitization_units(&a, &newcomer, &t.pool_id, &SecTranche::Junior, &10);
        // 400 more: junior pool 450 (11.25 per unit).
        fund_and_deposit(&t, 400);

        assert_eq!(c.get_claimable_securitization(&t.pool_id, &SecTranche::Junior, &a), 237);
        assert_eq!(c.get_claimable_securitization(&t.pool_id, &SecTranche::Junior, &newcomer), 100);
        assert_eq!(c.get_claimable_securitization(&t.pool_id, &SecTranche::Junior, &b), 112);
        assert_eq!(c.claim_securitization_payout(&newcomer, &t.pool_id, &SecTranche::Junior), 100);
    }

    #[test]
    #[should_panic(expected = "nothing to claim")]
    fn claim_with_nothing_owed_panics() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        let a = investor(&t, 100);
        c.buy_securitization_units(&a, &t.pool_id, &SecTranche::Junior, &10);
        c.activate_securitization(&t.originator, &t.pool_id);
        c.claim_securitization_payout(&a, &t.pool_id, &SecTranche::Junior);
    }

    #[test]
    #[should_panic(expected = "backing invoices are not final")]
    fn outsiders_cannot_close_early() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        let a = investor(&t, 100);
        c.buy_securitization_units(&a, &t.pool_id, &SecTranche::Junior, &10);
        c.activate_securitization(&t.originator, &t.pool_id);
        c.close_securitization(&a, &t.pool_id);
    }

    #[test]
    #[should_panic(expected = "securitization is not active")]
    fn closed_pool_rejects_deposits() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        let a = investor(&t, 100);
        c.buy_securitization_units(&a, &t.pool_id, &SecTranche::Junior, &10);
        c.activate_securitization(&t.originator, &t.pool_id);
        c.close_securitization(&t.originator, &t.pool_id);
        fund_and_deposit(&t, 100);
    }
}
