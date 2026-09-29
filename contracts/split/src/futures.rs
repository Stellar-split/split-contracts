//! Issue #863: Invoice futures contracts for payment predictions.
//!
//! An invoice future is a binary prediction market on a single invoice:
//! "will this invoice be fully paid and released by `expiry`?"
//!
//! - `open_invoice_future` creates a market on a `Pending` invoice.
//! - `take_future_position` stakes tokens on the `Paid` or `Unpaid` side while
//!   the market is open (before `expiry` and while the invoice is `Pending`).
//!   The invoice creator and co-creators cannot trade their own invoices.
//! - `settle_invoice_future` is permissionless and resolves the market once
//!   the outcome is known:
//!     - `Paid`   — invoice released at or before `expiry`;
//!     - `Unpaid` — invoice refunded/expired/cancelled/deleted, released after
//!       `expiry`, or still unreleased once `expiry` has passed;
//!     - `Void`   — nobody backed the winning side; every stake is refunded.
//! - `claim_future_payout` pays winners their pro-rata share of the whole
//!   pool: `stake * (paid_pool + unpaid_pool) / winning_pool`. Rounding dust
//!   stays in the contract.

use crate::*;
use soroban_sdk::{contractimpl, contracttype, symbol_short, token, Address, Env, Symbol, Vec};

/// Maximum number of futures markets that may be opened against one invoice.
const MAX_FUTURES_PER_INVOICE: u32 = 20;

/// Implied probability reported for a market with no stakes on either side.
const FUTURE_ODDS_NO_STAKES_BPS: u32 = 5_000;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FutureOutcome {
    /// Market open or awaiting settlement.
    Pending,
    /// Invoice released on or before expiry.
    Paid,
    /// Invoice not released by expiry.
    Unpaid,
    /// Winning side had no stakes; all stakes are refundable.
    Void,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvoiceFuture {
    pub id: u64,
    pub invoice_id: u64,
    pub opener: Address,
    pub stake_token: Address,
    /// Unix timestamp by which the invoice must be released for `Paid`.
    pub expiry: u64,
    pub paid_pool: i128,
    pub unpaid_pool: i128,
    pub traders: u32,
    pub outcome: FutureOutcome,
    pub settled_at: Option<u64>,
}

#[contracttype]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FuturePosition {
    pub paid_stake: i128,
    pub unpaid_stake: i128,
    pub claimed: bool,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

/// Instance storage: last issued future ID.
fn future_counter_key() -> Symbol {
    symbol_short!("fut_ctr")
}

/// Persistent storage: future ID → `InvoiceFuture`.
fn future_key(future_id: u64) -> (Symbol, u64) {
    (symbol_short!("fut_mkt"), future_id)
}

/// Persistent storage: (future ID, trader) → `FuturePosition`.
fn future_position_key(future_id: u64, trader: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("fut_pos"), future_id, trader.clone())
}

/// Persistent storage: invoice ID → `Vec<u64>` of future IDs on that invoice.
fn invoice_futures_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("fut_inv"), invoice_id)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn load_future(env: &Env, future_id: u64) -> InvoiceFuture {
    env.storage()
        .persistent()
        .get(&future_key(future_id))
        .expect("future not found")
}

fn save_future(env: &Env, future: &InvoiceFuture) {
    env.storage().persistent().set(&future_key(future.id), future);
}

fn load_position(env: &Env, future_id: u64, trader: &Address) -> Option<FuturePosition> {
    env.storage()
        .persistent()
        .get(&future_position_key(future_id, trader))
}

/// Resolve whether the invoice counts as paid by `expiry`, given the current
/// time. `None` while the outcome is still undetermined.
pub(crate) fn resolve_payment_outcome(
    status: &InvoiceStatus,
    completion_time: Option<u64>,
    expiry: u64,
    now: u64,
) -> Option<bool> {
    match status {
        InvoiceStatus::Released | InvoiceStatus::Finalised => {
            Some(completion_time.unwrap_or(now) <= expiry)
        }
        InvoiceStatus::Refunded
        | InvoiceStatus::Expired
        | InvoiceStatus::Cancelled
        | InvoiceStatus::Deleted => Some(false),
        InvoiceStatus::Pending | InvoiceStatus::Disputed | InvoiceStatus::PartiallyReleased => {
            if now > expiry {
                Some(false)
            } else {
                None
            }
        }
    }
}

/// Amount owed to a position once the market has an outcome.
pub(crate) fn future_payout(future: &InvoiceFuture, position: &FuturePosition) -> i128 {
    let total = future
        .paid_pool
        .checked_add(future.unpaid_pool)
        .expect("future pool overflow");
    let (stake, winning_pool) = match future.outcome {
        FutureOutcome::Pending => return 0,
        FutureOutcome::Void => {
            return position
                .paid_stake
                .checked_add(position.unpaid_stake)
                .expect("future stake overflow")
        }
        FutureOutcome::Paid => (position.paid_stake, future.paid_pool),
        FutureOutcome::Unpaid => (position.unpaid_stake, future.unpaid_pool),
    };
    if stake == 0 || winning_pool == 0 {
        return 0;
    }
    stake.checked_mul(total).expect("future payout overflow") / winning_pool
}

// ---------------------------------------------------------------------------
// Contract entry points
// ---------------------------------------------------------------------------

#[contractimpl]
impl SplitContract {
    /// Open a futures market on whether `invoice_id` is released by `expiry`.
    /// Returns the new future ID.
    pub fn open_invoice_future(
        env: Env,
        opener: Address,
        invoice_id: u64,
        stake_token: Address,
        expiry: u64,
    ) -> u64 {
        require_not_paused(&env);
        opener.require_auth();

        let invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice must be pending"
        );
        assert!(
            expiry > env.ledger().timestamp(),
            "expiry must be in the future"
        );
        validate_allowed_token(&env, &stake_token);

        let mut ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&invoice_futures_key(invoice_id))
            .unwrap_or_else(|| Vec::new(&env));
        assert!(
            ids.len() < MAX_FUTURES_PER_INVOICE,
            "too many futures on invoice"
        );

        let future_id: u64 = env
            .storage()
            .instance()
            .get(&future_counter_key())
            .unwrap_or(0u64)
            + 1;
        env.storage().instance().set(&future_counter_key(), &future_id);

        let future = InvoiceFuture {
            id: future_id,
            invoice_id,
            opener: opener.clone(),
            stake_token: stake_token.clone(),
            expiry,
            paid_pool: 0,
            unpaid_pool: 0,
            traders: 0,
            outcome: FutureOutcome::Pending,
            settled_at: None,
        };
        save_future(&env, &future);

        ids.push_back(future_id);
        env.storage()
            .persistent()
            .set(&invoice_futures_key(invoice_id), &ids);

        events::future_opened(&env, invoice_id, future_id, &opener, &stake_token, expiry);
        future_id
    }

    /// Stake `amount` on the invoice being paid (`predict_paid = true`) or not
    /// paid (`false`) by the market's expiry. Positions accumulate.
    pub fn take_future_position(
        env: Env,
        trader: Address,
        future_id: u64,
        predict_paid: bool,
        amount: i128,
    ) {
        require_not_paused(&env);
        trader.require_auth();
        assert!(amount > 0, "stake must be positive");

        let mut future = load_future(&env, future_id);
        assert!(
            future.outcome == FutureOutcome::Pending,
            "future already settled"
        );
        assert!(
            env.ledger().timestamp() < future.expiry,
            "future trading closed"
        );

        let invoice = load_invoice(&env, future.invoice_id);
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice no longer pending"
        );
        assert!(
            trader != invoice.creator && !invoice.co_creators.contains(&trader),
            "invoice creators cannot trade their own invoice"
        );

        token::Client::new(&env, &future.stake_token).transfer(
            &trader,
            &env.current_contract_address(),
            &amount,
        );

        let existing = load_position(&env, future_id, &trader);
        if existing.is_none() {
            future.traders = future.traders.saturating_add(1);
        }
        let mut position = existing.unwrap_or_default();
        if predict_paid {
            position.paid_stake = position
                .paid_stake
                .checked_add(amount)
                .expect("future stake overflow");
            future.paid_pool = future
                .paid_pool
                .checked_add(amount)
                .expect("future pool overflow");
        } else {
            position.unpaid_stake = position
                .unpaid_stake
                .checked_add(amount)
                .expect("future stake overflow");
            future.unpaid_pool = future
                .unpaid_pool
                .checked_add(amount)
                .expect("future pool overflow");
        }
        env.storage()
            .persistent()
            .set(&future_position_key(future_id, &trader), &position);
        save_future(&env, &future);

        events::future_position_taken(&env, future.invoice_id, future_id, &trader, predict_paid, amount);
    }

    /// Resolve a futures market. Permissionless; panics if the outcome is not
    /// yet determined. Returns the outcome.
    pub fn settle_invoice_future(env: Env, future_id: u64) -> FutureOutcome {
        require_not_paused(&env);
        let mut future = load_future(&env, future_id);
        assert!(
            future.outcome == FutureOutcome::Pending,
            "future already settled"
        );

        let invoice = load_invoice(&env, future.invoice_id);
        let now = env.ledger().timestamp();
        let paid = resolve_payment_outcome(&invoice.status, invoice.completion_time, future.expiry, now)
            .expect("future outcome not yet determined");

        let winning_pool = if paid {
            future.paid_pool
        } else {
            future.unpaid_pool
        };
        future.outcome = if winning_pool == 0 {
            FutureOutcome::Void
        } else if paid {
            FutureOutcome::Paid
        } else {
            FutureOutcome::Unpaid
        };
        future.settled_at = Some(now);
        save_future(&env, &future);

        events::future_settled(
            &env,
            future.invoice_id,
            future_id,
            future.outcome,
            future.paid_pool,
            future.unpaid_pool,
        );
        future.outcome
    }

    /// Claim the payout for a settled market. Returns the amount paid.
    pub fn claim_future_payout(env: Env, trader: Address, future_id: u64) -> i128 {
        require_not_paused(&env);
        trader.require_auth();

        let future = load_future(&env, future_id);
        assert!(
            future.outcome != FutureOutcome::Pending,
            "future not settled"
        );
        let mut position = load_position(&env, future_id, &trader).expect("no position");
        assert!(!position.claimed, "payout already claimed");

        let payout = future_payout(&future, &position);
        assert!(payout > 0, "no payout for position");

        position.claimed = true;
        env.storage()
            .persistent()
            .set(&future_position_key(future_id, &trader), &position);

        token::Client::new(&env, &future.stake_token).transfer(
            &env.current_contract_address(),
            &trader,
            &payout,
        );

        events::future_payout_claimed(&env, future.invoice_id, future_id, &trader, payout);
        payout
    }

    /// Return a futures market by ID.
    pub fn get_invoice_future(env: Env, future_id: u64) -> InvoiceFuture {
        load_future(&env, future_id)
    }

    /// Return a trader's position in a market, if any.
    pub fn get_future_position(env: Env, future_id: u64, trader: Address) -> Option<FuturePosition> {
        load_position(&env, future_id, &trader)
    }

    /// Return all future IDs opened against an invoice.
    pub fn get_invoice_futures(env: Env, invoice_id: u64) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&invoice_futures_key(invoice_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Market-implied probability that the invoice is paid, in basis points:
    /// `paid_pool * 10_000 / (paid_pool + unpaid_pool)`. 5 000 with no stakes.
    pub fn get_future_implied_odds(env: Env, future_id: u64) -> u32 {
        let future = load_future(&env, future_id);
        let total = future.paid_pool + future.unpaid_pool;
        if total == 0 {
            return FUTURE_ODDS_NO_STAKES_BPS;
        }
        (future.paid_pool * 10_000 / total) as u32
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

    struct Ctx {
        env: Env,
        contract_id: Address,
        token_id: Address,
        creator: Address,
        invoice_id: u64,
        future_id: u64,
        bull: Address,
        bear: Address,
    }

    /// Invoice for 200 due at 10_000; future expiring at 5_000; two funded traders.
    fn ctx() -> Ctx {
        let (env, contract_id, token_id) = setup_initialized();
        let c = client(&env, &contract_id);
        let creator = Address::generate(&env);
        let recipient = Address::generate(&env);
        env.ledger().set_timestamp(1_000);
        let invoice_id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 10_000);

        let opener = Address::generate(&env);
        let future_id = c.open_invoice_future(&opener, &invoice_id, &token_id, &5_000);

        let bull = Address::generate(&env);
        let bear = Address::generate(&env);
        let sac = StellarAssetClient::new(&env, &token_id);
        sac.mint(&bull, &1_000);
        sac.mint(&bear, &1_000);
        Ctx {
            env,
            contract_id,
            token_id,
            creator,
            invoice_id,
            future_id,
            bull,
            bear,
        }
    }

    fn pay_invoice_in_full(t: &Ctx) {
        let payer = Address::generate(&t.env);
        StellarAssetClient::new(&t.env, &t.token_id).mint(&payer, &200);
        client(&t.env, &t.contract_id).pay(&payer, &t.invoice_id, &200_i128, &0_u64, &false, &false, &None);
    }

    fn sample_future(paid_pool: i128, unpaid_pool: i128, outcome: FutureOutcome, env: &Env) -> InvoiceFuture {
        InvoiceFuture {
            id: 1,
            invoice_id: 1,
            opener: Address::generate(env),
            stake_token: Address::generate(env),
            expiry: 0,
            paid_pool,
            unpaid_pool,
            traders: 0,
            outcome,
            settled_at: None,
        }
    }

    #[test]
    fn resolve_outcome_rules() {
        use InvoiceStatus::*;
        assert_eq!(resolve_payment_outcome(&Released, Some(90), 100, 200), Some(true));
        assert_eq!(resolve_payment_outcome(&Released, Some(101), 100, 200), Some(false));
        assert_eq!(resolve_payment_outcome(&Finalised, None, 100, 50), Some(true));
        assert_eq!(resolve_payment_outcome(&Cancelled, None, 100, 50), Some(false));
        assert_eq!(resolve_payment_outcome(&Refunded, None, 100, 50), Some(false));
        assert_eq!(resolve_payment_outcome(&Pending, None, 100, 100), None);
        assert_eq!(resolve_payment_outcome(&Pending, None, 100, 101), Some(false));
        assert_eq!(resolve_payment_outcome(&Disputed, None, 100, 50), None);
    }

    #[test]
    fn payout_math() {
        let env = Env::default();
        let pos = FuturePosition { paid_stake: 30, unpaid_stake: 10, claimed: false };

        let paid = sample_future(60, 40, FutureOutcome::Paid, &env);
        assert_eq!(future_payout(&paid, &pos), 50);

        let unpaid = sample_future(60, 40, FutureOutcome::Unpaid, &env);
        assert_eq!(future_payout(&unpaid, &pos), 25);

        let void = sample_future(60, 0, FutureOutcome::Void, &env);
        assert_eq!(future_payout(&void, &pos), 40);

        let pending = sample_future(60, 40, FutureOutcome::Pending, &env);
        assert_eq!(future_payout(&pending, &pos), 0);
    }

    #[test]
    fn open_registers_market() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        let f = c.get_invoice_future(&t.future_id);
        assert_eq!(f.invoice_id, t.invoice_id);
        assert_eq!(f.outcome, FutureOutcome::Pending);
        assert_eq!(c.get_invoice_futures(&t.invoice_id).len(), 1);
        assert_eq!(c.get_future_implied_odds(&t.future_id), 5_000);
    }

    #[test]
    #[should_panic(expected = "expiry must be in the future")]
    fn open_rejects_past_expiry() {
        let t = ctx();
        let opener = Address::generate(&t.env);
        client(&t.env, &t.contract_id).open_invoice_future(&opener, &t.invoice_id, &t.token_id, &1_000);
    }

    #[test]
    fn positions_escrow_stakes_and_move_odds() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        let tk = TokenClient::new(&t.env, &t.token_id);

        c.take_future_position(&t.bull, &t.future_id, &true, &300);
        c.take_future_position(&t.bull, &t.future_id, &true, &300);
        c.take_future_position(&t.bear, &t.future_id, &false, &400);
        assert!(!t.env.events().all().is_empty());

        assert_eq!(tk.balance(&t.bull), 400);
        assert_eq!(tk.balance(&t.bear), 600);
        let f = c.get_invoice_future(&t.future_id);
        assert_eq!(f.paid_pool, 600);
        assert_eq!(f.unpaid_pool, 400);
        assert_eq!(f.traders, 2);
        assert_eq!(c.get_future_implied_odds(&t.future_id), 6_000);
        assert_eq!(
            c.get_future_position(&t.future_id, &t.bull).unwrap().paid_stake,
            600
        );
    }

    #[test]
    #[should_panic(expected = "invoice creators cannot trade their own invoice")]
    fn creator_cannot_trade() {
        let t = ctx();
        StellarAssetClient::new(&t.env, &t.token_id).mint(&t.creator, &100);
        client(&t.env, &t.contract_id).take_future_position(&t.creator, &t.future_id, &false, &100);
    }

    #[test]
    #[should_panic(expected = "future trading closed")]
    fn trading_closes_at_expiry() {
        let t = ctx();
        t.env.ledger().set_timestamp(5_000);
        client(&t.env, &t.contract_id).take_future_position(&t.bull, &t.future_id, &true, &100);
    }

    #[test]
    #[should_panic(expected = "future outcome not yet determined")]
    fn settle_before_outcome_panics() {
        let t = ctx();
        client(&t.env, &t.contract_id).settle_invoice_future(&t.future_id);
    }

    #[test]
    fn paid_outcome_pays_bulls_whole_pool() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        let tk = TokenClient::new(&t.env, &t.token_id);
        c.take_future_position(&t.bull, &t.future_id, &true, &300);
        c.take_future_position(&t.bear, &t.future_id, &false, &600);

        pay_invoice_in_full(&t);
        assert_eq!(c.settle_invoice_future(&t.future_id), FutureOutcome::Paid);

        assert_eq!(c.claim_future_payout(&t.bull, &t.future_id), 900);
        assert_eq!(tk.balance(&t.bull), 1_600);
    }

    #[test]
    #[should_panic(expected = "no payout for position")]
    fn losers_cannot_claim() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        c.take_future_position(&t.bull, &t.future_id, &true, &300);
        c.take_future_position(&t.bear, &t.future_id, &false, &600);
        pay_invoice_in_full(&t);
        c.settle_invoice_future(&t.future_id);
        c.claim_future_payout(&t.bear, &t.future_id);
    }

    #[test]
    fn expiry_without_release_resolves_unpaid() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        c.take_future_position(&t.bull, &t.future_id, &true, &300);
        c.take_future_position(&t.bear, &t.future_id, &false, &100);

        t.env.ledger().set_timestamp(5_001);
        assert_eq!(c.settle_invoice_future(&t.future_id), FutureOutcome::Unpaid);
        assert_eq!(c.claim_future_payout(&t.bear, &t.future_id), 400);
    }

    #[test]
    fn cancelled_invoice_resolves_unpaid_before_expiry() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        c.take_future_position(&t.bear, &t.future_id, &false, &100);
        c.cancel_invoice(&t.creator, &t.invoice_id);
        assert_eq!(c.settle_invoice_future(&t.future_id), FutureOutcome::Unpaid);
    }

    #[test]
    fn empty_winning_side_voids_and_refunds() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        let tk = TokenClient::new(&t.env, &t.token_id);
        c.take_future_position(&t.bear, &t.future_id, &false, &250);

        pay_invoice_in_full(&t);
        assert_eq!(c.settle_invoice_future(&t.future_id), FutureOutcome::Void);
        assert_eq!(c.claim_future_payout(&t.bear, &t.future_id), 250);
        assert_eq!(tk.balance(&t.bear), 1_000);
    }

    #[test]
    #[should_panic(expected = "payout already claimed")]
    fn double_claim_panics() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        c.take_future_position(&t.bear, &t.future_id, &false, &250);
        pay_invoice_in_full(&t);
        c.settle_invoice_future(&t.future_id);
        c.claim_future_payout(&t.bear, &t.future_id);
        c.claim_future_payout(&t.bear, &t.future_id);
    }

    #[test]
    #[should_panic(expected = "future already settled")]
    fn double_settle_panics() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        pay_invoice_in_full(&t);
        c.settle_invoice_future(&t.future_id);
        c.settle_invoice_future(&t.future_id);
    }
}
