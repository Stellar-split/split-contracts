//! Issue #864: Recipient performance SLA guarantees.
//!
//! A creator can ask a recipient on one of their invoices to back a delivery
//! commitment with a bond. The lifecycle is:
//!
//! 1. `create_recipient_sla` — creator proposes the terms (deadline, bond size,
//!    per-period late penalty, penalty cap). Status: `Proposed`.
//! 2. `accept_recipient_sla` — recipient accepts and posts the bond into the
//!    contract. Status: `Active`.
//! 3. Either:
//!    - `confirm_sla_delivery` — creator confirms delivery. Any accrued late
//!      penalty is paid from the bond to the creator and the remainder is
//!      returned to the recipient. Status: `Met` (on time) or `Breached` (late).
//!    - `claim_sla_breach` — once the penalty has accrued to its cap without a
//!      delivery confirmation, the creator claims the capped penalty and the
//!      remainder is returned to the recipient. Status: `Breached`.
//!
//! Every settled SLA updates the recipient's cross-invoice
//! [`RecipientPerformance`] record, which backs `get_recipient_sla_score`.

use crate::*;
use soroban_sdk::{contractimpl, contracttype, symbol_short, token, Address, Env, Symbol};

/// Upper bound on the number of penalty periods an SLA may span before the
/// cap is reached; keeps `claim_sla_breach` reachable within a sane horizon.
const MAX_SLA_PENALTY_PERIODS: u64 = 10_000;

/// Score reported for a recipient with no settled SLAs (no breaches on record).
const SLA_SCORE_NO_HISTORY: u32 = 10_000;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlaStatus {
    /// Terms proposed by the creator; no bond posted yet.
    Proposed,
    /// Recipient accepted and the bond is held by the contract.
    Active,
    /// Delivery confirmed on or before the deadline; full bond returned.
    Met,
    /// Delivered late or never delivered; a penalty was taken from the bond.
    Breached,
    /// Proposal withdrawn by the creator before acceptance.
    Cancelled,
}

/// A bonded delivery commitment by one recipient on one invoice.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecipientSla {
    pub invoice_id: u64,
    pub recipient: Address,
    /// Address that proposed the SLA and receives any penalty.
    pub beneficiary: Address,
    /// Token the bond is posted in (the invoice's funding token).
    pub token: Address,
    /// Unix timestamp by which delivery must be confirmed to avoid penalties.
    pub delivery_deadline: u64,
    pub bond: i128,
    /// Penalty accrued per started `penalty_period_secs` past the deadline.
    pub penalty_bps_per_period: u32,
    pub penalty_period_secs: u64,
    /// Cap on the total penalty, in basis points of the bond.
    pub max_penalty_bps: u32,
    pub status: SlaStatus,
    pub accepted_at: Option<u64>,
    pub settled_at: Option<u64>,
    pub penalty_paid: i128,
}

/// Cross-invoice SLA track record for a recipient.
#[contracttype]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RecipientPerformance {
    pub slas_accepted: u32,
    pub slas_met: u32,
    pub slas_breached: u32,
    pub total_bonded: i128,
    pub total_penalties: i128,
    /// Sum of seconds delivered late across all breached SLAs.
    pub total_late_secs: u64,
}

// ---------------------------------------------------------------------------
// Storage keys (persistent)
// ---------------------------------------------------------------------------

fn sla_key(invoice_id: u64, recipient: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("sla_rec"), invoice_id, recipient.clone())
}

fn sla_perf_key(recipient: &Address) -> (Symbol, Address) {
    (symbol_short!("sla_perf"), recipient.clone())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn load_sla(env: &Env, invoice_id: u64, recipient: &Address) -> RecipientSla {
    env.storage()
        .persistent()
        .get(&sla_key(invoice_id, recipient))
        .expect("SLA not found")
}

fn save_sla(env: &Env, sla: &RecipientSla) {
    env.storage()
        .persistent()
        .set(&sla_key(sla.invoice_id, &sla.recipient), sla);
}

fn load_performance(env: &Env, recipient: &Address) -> RecipientPerformance {
    env.storage()
        .persistent()
        .get(&sla_perf_key(recipient))
        .unwrap_or_default()
}

fn save_performance(env: &Env, recipient: &Address, perf: &RecipientPerformance) {
    env.storage().persistent().set(&sla_perf_key(recipient), perf);
}

/// Penalty in basis points of the bond for a delivery at `at`.
///
/// Every started `period_secs` past `deadline` adds `bps_per_period`, capped at
/// `max_bps`. Delivery at or before the deadline accrues nothing.
pub(crate) fn sla_penalty_bps(
    deadline: u64,
    at: u64,
    bps_per_period: u32,
    period_secs: u64,
    max_bps: u32,
) -> u32 {
    if at <= deadline || period_secs == 0 {
        return 0;
    }
    let periods = (at - deadline).div_ceil(period_secs);
    periods
        .saturating_mul(bps_per_period as u64)
        .min(max_bps as u64) as u32
}

/// `amount * bps / 10_000`, panicking on overflow.
fn bps_of(amount: i128, bps: u32) -> i128 {
    amount
        .checked_mul(bps as i128)
        .expect("SLA penalty overflow")
        / 10_000
}

/// Timestamp at which the penalty reaches `max_penalty_bps`.
fn sla_cap_reached_at(sla: &RecipientSla) -> u64 {
    let periods_to_cap = (sla.max_penalty_bps as u64).div_ceil(sla.penalty_bps_per_period as u64);
    sla.delivery_deadline
        .saturating_add(periods_to_cap.saturating_mul(sla.penalty_period_secs))
}

/// Pay `penalty` to the beneficiary and refund the rest of the bond to the
/// recipient, then record the outcome on the SLA and the recipient's record.
fn settle_sla(env: &Env, sla: &mut RecipientSla, penalty: i128, now: u64) {
    let token_client = token::Client::new(env, &sla.token);
    let contract = env.current_contract_address();
    if penalty > 0 {
        token_client.transfer(&contract, &sla.beneficiary, &penalty);
    }
    let refund = sla.bond - penalty;
    if refund > 0 {
        token_client.transfer(&contract, &sla.recipient, &refund);
    }

    let mut perf = load_performance(env, &sla.recipient);
    if now <= sla.delivery_deadline {
        sla.status = SlaStatus::Met;
        perf.slas_met = perf.slas_met.saturating_add(1);
    } else {
        sla.status = SlaStatus::Breached;
        perf.slas_breached = perf.slas_breached.saturating_add(1);
        perf.total_late_secs = perf
            .total_late_secs
            .saturating_add(now - sla.delivery_deadline);
    }
    perf.total_penalties = perf
        .total_penalties
        .checked_add(penalty)
        .expect("SLA penalty total overflow");
    save_performance(env, &sla.recipient, &perf);

    sla.penalty_paid = penalty;
    sla.settled_at = Some(now);
    save_sla(env, sla);

    events::sla_settled(
        env,
        sla.invoice_id,
        &sla.recipient,
        sla.status == SlaStatus::Met,
        penalty,
        refund,
    );
}

// ---------------------------------------------------------------------------
// Contract entry points
// ---------------------------------------------------------------------------

#[contractimpl]
impl SplitContract {
    /// Propose a bonded delivery SLA for `recipient` on `invoice_id`.
    ///
    /// `caller` must be the invoice creator or a co-creator; they become the
    /// beneficiary of any penalty. The invoice must be `Pending` and the
    /// recipient must be listed on it. At most one live SLA per
    /// (invoice, recipient); a `Cancelled` one may be replaced.
    pub fn create_recipient_sla(
        env: Env,
        caller: Address,
        invoice_id: u64,
        recipient: Address,
        delivery_deadline: u64,
        bond: i128,
        penalty_bps_per_period: u32,
        penalty_period_secs: u64,
        max_penalty_bps: u32,
    ) {
        require_not_paused(&env);
        caller.require_auth();

        let invoice = load_invoice(&env, invoice_id);
        require_creator_or_cocreator(&invoice, &caller);
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice must be pending"
        );
        assert!(
            invoice.recipients.contains(&recipient),
            "recipient not on invoice"
        );
        assert!(bond > 0, "bond must be positive");
        assert!(
            delivery_deadline > env.ledger().timestamp(),
            "delivery deadline must be in the future"
        );
        assert!(penalty_period_secs > 0, "penalty period must be positive");
        assert!(
            penalty_bps_per_period > 0 && penalty_bps_per_period <= 10_000,
            "penalty_bps_per_period must be in 1..=10000"
        );
        assert!(
            max_penalty_bps > 0 && max_penalty_bps <= 10_000,
            "max_penalty_bps must be in 1..=10000"
        );
        assert!(
            (max_penalty_bps as u64).div_ceil(penalty_bps_per_period as u64)
                <= MAX_SLA_PENALTY_PERIODS,
            "too many penalty periods before cap"
        );

        if let Some(existing) = env
            .storage()
            .persistent()
            .get::<_, RecipientSla>(&sla_key(invoice_id, &recipient))
        {
            assert!(
                existing.status == SlaStatus::Cancelled,
                "SLA already exists for recipient"
            );
        }

        let sla = RecipientSla {
            invoice_id,
            recipient: recipient.clone(),
            beneficiary: caller.clone(),
            token: funding_token_for(&invoice),
            delivery_deadline,
            bond,
            penalty_bps_per_period,
            penalty_period_secs,
            max_penalty_bps,
            status: SlaStatus::Proposed,
            accepted_at: None,
            settled_at: None,
            penalty_paid: 0,
        };
        save_sla(&env, &sla);

        events::sla_proposed(&env, invoice_id, &recipient, &caller, bond, delivery_deadline);
    }

    /// Accept a proposed SLA and post its bond. Only the named recipient may
    /// accept, and only before the delivery deadline.
    pub fn accept_recipient_sla(env: Env, recipient: Address, invoice_id: u64) {
        require_not_paused(&env);
        recipient.require_auth();

        let mut sla = load_sla(&env, invoice_id, &recipient);
        assert!(sla.status == SlaStatus::Proposed, "SLA is not proposed");
        let now = env.ledger().timestamp();
        assert!(now < sla.delivery_deadline, "delivery deadline passed");

        token::Client::new(&env, &sla.token).transfer(
            &recipient,
            &env.current_contract_address(),
            &sla.bond,
        );

        sla.status = SlaStatus::Active;
        sla.accepted_at = Some(now);
        save_sla(&env, &sla);

        let mut perf = load_performance(&env, &recipient);
        perf.slas_accepted = perf.slas_accepted.saturating_add(1);
        perf.total_bonded = perf
            .total_bonded
            .checked_add(sla.bond)
            .expect("SLA bond total overflow");
        save_performance(&env, &recipient, &perf);

        events::sla_accepted(&env, invoice_id, &recipient, sla.bond);
    }

    /// Withdraw a proposed SLA before the recipient accepts it.
    pub fn cancel_recipient_sla(env: Env, caller: Address, invoice_id: u64, recipient: Address) {
        caller.require_auth();
        let mut sla = load_sla(&env, invoice_id, &recipient);
        assert!(caller == sla.beneficiary, "only the SLA beneficiary may cancel");
        assert!(sla.status == SlaStatus::Proposed, "SLA is not proposed");

        sla.status = SlaStatus::Cancelled;
        save_sla(&env, &sla);

        events::sla_cancelled(&env, invoice_id, &recipient);
    }

    /// Confirm delivery for an active SLA. Settles immediately: any accrued
    /// late penalty goes to the beneficiary, the rest of the bond to the
    /// recipient. Returns the penalty taken.
    pub fn confirm_sla_delivery(
        env: Env,
        caller: Address,
        invoice_id: u64,
        recipient: Address,
    ) -> i128 {
        require_not_paused(&env);
        caller.require_auth();
        let mut sla = load_sla(&env, invoice_id, &recipient);
        assert!(
            caller == sla.beneficiary,
            "only the SLA beneficiary may confirm delivery"
        );
        assert!(sla.status == SlaStatus::Active, "SLA is not active");

        let now = env.ledger().timestamp();
        let bps = sla_penalty_bps(
            sla.delivery_deadline,
            now,
            sla.penalty_bps_per_period,
            sla.penalty_period_secs,
            sla.max_penalty_bps,
        );
        let penalty = bps_of(sla.bond, bps);
        settle_sla(&env, &mut sla, penalty, now);
        penalty
    }

    /// Claim the capped penalty for an active SLA whose delivery was never
    /// confirmed. Callable by the beneficiary once the accrued penalty has
    /// reached `max_penalty_bps`. Returns the penalty taken.
    pub fn claim_sla_breach(env: Env, caller: Address, invoice_id: u64, recipient: Address) -> i128 {
        require_not_paused(&env);
        caller.require_auth();
        let mut sla = load_sla(&env, invoice_id, &recipient);
        assert!(
            caller == sla.beneficiary,
            "only the SLA beneficiary may claim a breach"
        );
        assert!(sla.status == SlaStatus::Active, "SLA is not active");

        let now = env.ledger().timestamp();
        assert!(now >= sla_cap_reached_at(&sla), "SLA penalty cap not yet reached");

        let penalty = bps_of(sla.bond, sla.max_penalty_bps);
        settle_sla(&env, &mut sla, penalty, now);
        penalty
    }

    /// Return the SLA for (invoice, recipient), if any.
    pub fn get_recipient_sla(env: Env, invoice_id: u64, recipient: Address) -> Option<RecipientSla> {
        env.storage()
            .persistent()
            .get(&sla_key(invoice_id, &recipient))
    }

    /// Penalty that would be taken if delivery were confirmed now. Zero for
    /// SLAs that are not `Active`.
    pub fn preview_sla_penalty(env: Env, invoice_id: u64, recipient: Address) -> i128 {
        let sla = load_sla(&env, invoice_id, &recipient);
        if sla.status != SlaStatus::Active {
            return 0;
        }
        let bps = sla_penalty_bps(
            sla.delivery_deadline,
            env.ledger().timestamp(),
            sla.penalty_bps_per_period,
            sla.penalty_period_secs,
            sla.max_penalty_bps,
        );
        bps_of(sla.bond, bps)
    }

    /// Cross-invoice SLA track record for `recipient`.
    pub fn get_recipient_performance(env: Env, recipient: Address) -> RecipientPerformance {
        load_performance(&env, &recipient)
    }

    /// On-time delivery rate in basis points: `met * 10_000 / (met + breached)`.
    /// Returns 10 000 for a recipient with no settled SLAs.
    pub fn get_recipient_sla_score(env: Env, recipient: Address) -> u32 {
        let perf = load_performance(&env, &recipient);
        let settled = perf.slas_met as u64 + perf.slas_breached as u64;
        if settled == 0 {
            return SLA_SCORE_NO_HISTORY;
        }
        (perf.slas_met as u64 * 10_000 / settled) as u32
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
        recipient: Address,
        invoice_id: u64,
    }

    fn ctx() -> Ctx {
        let (env, contract_id, token_id) = setup_initialized();
        let c = client(&env, &contract_id);
        let creator = Address::generate(&env);
        let recipient = Address::generate(&env);
        env.ledger().set_timestamp(1_000);
        let invoice_id = make_invoice(&env, &c, &creator, &recipient, 500, &token_id, 100_000);
        StellarAssetClient::new(&env, &token_id).mint(&recipient, &10_000);
        Ctx {
            env,
            contract_id,
            token_id,
            creator,
            recipient,
            invoice_id,
        }
    }

    /// Deadline 2_000, bond 1_000, 10% per started hour, capped at 50%.
    fn propose_default(t: &Ctx) {
        client(&t.env, &t.contract_id).create_recipient_sla(
            &t.creator,
            &t.invoice_id,
            &t.recipient,
            &2_000,
            &1_000,
            &1_000,
            &3_600,
            &5_000,
        );
    }

    fn propose_and_accept(t: &Ctx) {
        propose_default(t);
        client(&t.env, &t.contract_id).accept_recipient_sla(&t.recipient, &t.invoice_id);
    }

    #[test]
    fn penalty_bps_is_zero_on_time() {
        assert_eq!(sla_penalty_bps(100, 50, 500, 10, 5_000), 0);
        assert_eq!(sla_penalty_bps(100, 100, 500, 10, 5_000), 0);
    }

    #[test]
    fn penalty_bps_counts_started_periods_and_caps() {
        assert_eq!(sla_penalty_bps(100, 101, 500, 10, 5_000), 500);
        assert_eq!(sla_penalty_bps(100, 110, 500, 10, 5_000), 500);
        assert_eq!(sla_penalty_bps(100, 111, 500, 10, 5_000), 1_000);
        assert_eq!(sla_penalty_bps(100, 10_000, 500, 10, 5_000), 5_000);
        assert_eq!(sla_penalty_bps(0, u64::MAX, u32::MAX, 1, 10_000), 10_000);
    }

    #[test]
    fn create_stores_proposed_sla_and_emits_event() {
        let t = ctx();
        propose_default(&t);
        let c = client(&t.env, &t.contract_id);

        let sla = c.get_recipient_sla(&t.invoice_id, &t.recipient).unwrap();
        assert_eq!(sla.status, SlaStatus::Proposed);
        assert_eq!(sla.beneficiary, t.creator);
        assert_eq!(sla.token, t.token_id);
        assert_eq!(sla.bond, 1_000);
        assert!(!t.env.events().all().is_empty());
    }

    #[test]
    #[should_panic(expected = "recipient not on invoice")]
    fn create_rejects_unknown_recipient() {
        let t = ctx();
        let stranger = Address::generate(&t.env);
        client(&t.env, &t.contract_id).create_recipient_sla(
            &t.creator, &t.invoice_id, &stranger, &2_000, &1_000, &1_000, &3_600, &5_000,
        );
    }

    #[test]
    #[should_panic(expected = "NotAuthorized")]
    fn create_rejects_non_creator() {
        let t = ctx();
        let stranger = Address::generate(&t.env);
        client(&t.env, &t.contract_id).create_recipient_sla(
            &stranger, &t.invoice_id, &t.recipient, &2_000, &1_000, &1_000, &3_600, &5_000,
        );
    }

    #[test]
    #[should_panic(expected = "delivery deadline must be in the future")]
    fn create_rejects_past_deadline() {
        let t = ctx();
        client(&t.env, &t.contract_id).create_recipient_sla(
            &t.creator, &t.invoice_id, &t.recipient, &500, &1_000, &1_000, &3_600, &5_000,
        );
    }

    #[test]
    #[should_panic(expected = "SLA already exists for recipient")]
    fn create_rejects_duplicate() {
        let t = ctx();
        propose_default(&t);
        propose_default(&t);
    }

    #[test]
    fn cancelled_sla_can_be_reproposed() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        propose_default(&t);
        c.cancel_recipient_sla(&t.creator, &t.invoice_id, &t.recipient);
        assert_eq!(
            c.get_recipient_sla(&t.invoice_id, &t.recipient).unwrap().status,
            SlaStatus::Cancelled
        );
        propose_default(&t);
        assert_eq!(
            c.get_recipient_sla(&t.invoice_id, &t.recipient).unwrap().status,
            SlaStatus::Proposed
        );
    }

    #[test]
    fn accept_escrows_bond() {
        let t = ctx();
        propose_and_accept(&t);
        let c = client(&t.env, &t.contract_id);
        let tk = TokenClient::new(&t.env, &t.token_id);

        assert_eq!(tk.balance(&t.recipient), 9_000);
        let sla = c.get_recipient_sla(&t.invoice_id, &t.recipient).unwrap();
        assert_eq!(sla.status, SlaStatus::Active);
        assert_eq!(sla.accepted_at, Some(1_000));

        let perf = c.get_recipient_performance(&t.recipient);
        assert_eq!(perf.slas_accepted, 1);
        assert_eq!(perf.total_bonded, 1_000);
    }

    #[test]
    #[should_panic(expected = "delivery deadline passed")]
    fn accept_rejects_after_deadline() {
        let t = ctx();
        propose_default(&t);
        t.env.ledger().set_timestamp(2_000);
        client(&t.env, &t.contract_id).accept_recipient_sla(&t.recipient, &t.invoice_id);
    }

    #[test]
    #[should_panic(expected = "SLA is not proposed")]
    fn cancel_rejects_active_sla() {
        let t = ctx();
        propose_and_accept(&t);
        client(&t.env, &t.contract_id).cancel_recipient_sla(&t.creator, &t.invoice_id, &t.recipient);
    }

    #[test]
    fn on_time_delivery_returns_full_bond() {
        let t = ctx();
        propose_and_accept(&t);
        let c = client(&t.env, &t.contract_id);
        let tk = TokenClient::new(&t.env, &t.token_id);

        t.env.ledger().set_timestamp(2_000);
        let penalty = c.confirm_sla_delivery(&t.creator, &t.invoice_id, &t.recipient);

        assert_eq!(penalty, 0);
        assert_eq!(tk.balance(&t.recipient), 10_000);
        let sla = c.get_recipient_sla(&t.invoice_id, &t.recipient).unwrap();
        assert_eq!(sla.status, SlaStatus::Met);
        assert_eq!(sla.settled_at, Some(2_000));
        assert_eq!(c.get_recipient_performance(&t.recipient).slas_met, 1);
        assert_eq!(c.get_recipient_sla_score(&t.recipient), 10_000);
    }

    #[test]
    fn late_delivery_pays_prorated_penalty() {
        let t = ctx();
        propose_and_accept(&t);
        let c = client(&t.env, &t.contract_id);
        let tk = TokenClient::new(&t.env, &t.token_id);

        // 1h + 1s late → two started periods → 20% of 1_000.
        t.env.ledger().set_timestamp(2_000 + 3_601);
        assert_eq!(c.preview_sla_penalty(&t.invoice_id, &t.recipient), 200);
        let penalty = c.confirm_sla_delivery(&t.creator, &t.invoice_id, &t.recipient);

        assert_eq!(penalty, 200);
        assert_eq!(tk.balance(&t.creator), 200);
        assert_eq!(tk.balance(&t.recipient), 9_800);
        let perf = c.get_recipient_performance(&t.recipient);
        assert_eq!(perf.slas_breached, 1);
        assert_eq!(perf.total_penalties, 200);
        assert_eq!(perf.total_late_secs, 3_601);
        assert_eq!(c.get_recipient_sla_score(&t.recipient), 0);
    }

    #[test]
    fn late_delivery_penalty_is_capped() {
        let t = ctx();
        propose_and_accept(&t);
        let c = client(&t.env, &t.contract_id);

        t.env.ledger().set_timestamp(2_000 + 100 * 3_600);
        let penalty = c.confirm_sla_delivery(&t.creator, &t.invoice_id, &t.recipient);
        assert_eq!(penalty, 500);
    }

    #[test]
    #[should_panic(expected = "only the SLA beneficiary may confirm delivery")]
    fn confirm_rejects_non_beneficiary() {
        let t = ctx();
        propose_and_accept(&t);
        client(&t.env, &t.contract_id).confirm_sla_delivery(&t.recipient, &t.invoice_id, &t.recipient);
    }

    #[test]
    #[should_panic(expected = "SLA is not active")]
    fn confirm_twice_panics() {
        let t = ctx();
        propose_and_accept(&t);
        let c = client(&t.env, &t.contract_id);
        c.confirm_sla_delivery(&t.creator, &t.invoice_id, &t.recipient);
        c.confirm_sla_delivery(&t.creator, &t.invoice_id, &t.recipient);
    }

    #[test]
    #[should_panic(expected = "SLA penalty cap not yet reached")]
    fn breach_claim_before_cap_panics() {
        let t = ctx();
        propose_and_accept(&t);
        // Cap (50%) is reached after 5 periods.
        t.env.ledger().set_timestamp(2_000 + 4 * 3_600);
        client(&t.env, &t.contract_id).claim_sla_breach(&t.creator, &t.invoice_id, &t.recipient);
    }

    #[test]
    fn breach_claim_after_cap_takes_max_penalty() {
        let t = ctx();
        propose_and_accept(&t);
        let c = client(&t.env, &t.contract_id);
        let tk = TokenClient::new(&t.env, &t.token_id);

        t.env.ledger().set_timestamp(2_000 + 5 * 3_600);
        let penalty = c.claim_sla_breach(&t.creator, &t.invoice_id, &t.recipient);

        assert_eq!(penalty, 500);
        assert_eq!(tk.balance(&t.creator), 500);
        assert_eq!(tk.balance(&t.recipient), 9_500);
        assert_eq!(
            c.get_recipient_sla(&t.invoice_id, &t.recipient).unwrap().status,
            SlaStatus::Breached
        );
    }

    #[test]
    fn score_mixes_met_and_breached_across_invoices() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        propose_and_accept(&t);
        c.confirm_sla_delivery(&t.creator, &t.invoice_id, &t.recipient);

        let second = make_invoice(&t.env, &c, &t.creator, &t.recipient, 500, &t.token_id, 100_000);
        c.create_recipient_sla(&t.creator, &second, &t.recipient, &2_000, &1_000, &1_000, &3_600, &5_000);
        c.accept_recipient_sla(&t.recipient, &second);
        t.env.ledger().set_timestamp(2_001);
        c.confirm_sla_delivery(&t.creator, &second, &t.recipient);

        assert_eq!(c.get_recipient_sla_score(&t.recipient), 5_000);
        assert_eq!(c.get_recipient_performance(&t.recipient).slas_accepted, 2);
    }

    #[test]
    fn score_defaults_to_full_without_history() {
        let t = ctx();
        let nobody = Address::generate(&t.env);
        assert_eq!(client(&t.env, &t.contract_id).get_recipient_sla_score(&nobody), 10_000);
    }
}
