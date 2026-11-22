//! Partial invoice payment tracking (issue #911).
//!
//! Tracks how much of each recipient's share has been paid out, with
//! milestone support:
//!
//! - **Per-recipient tracking** — [`RecipientPaymentState`] records the amount
//!   due and paid for every recipient of an invoice.
//! - **Paid flags** — a `u64` bitmask (bit `i` = recipient `i` fully paid) is
//!   stored alongside the tracker so callers can check payment status without
//!   loading the full tracker.
//! - **Milestones** — ascending basis-point thresholds of the invoice total.
//!   Each milestone moves through the state machine
//!   `Pending → Reached → Settled`; a milestone becomes `Reached` once total
//!   paid crosses its threshold and is `Settled` explicitly by the creator,
//!   strictly in order.
//! - **Atomic updates** — every mutating operation validates all of its
//!   inputs and computes the new state in memory before writing; a panic
//!   leaves storage untouched.

use soroban_sdk::{contracttype, symbol_short, Address, Env, Symbol, Vec};

use crate::events;

/// Maximum number of recipients supported by the `u64` paid-flag bitmask.
pub const MAX_TRACKED_RECIPIENTS: u32 = 64;

/// Maximum number of milestones per tracker.
pub const MAX_MILESTONES: u32 = 10;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Milestone lifecycle: `Pending → Reached → Settled`.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum MilestoneState {
    Pending = 0,
    Reached = 1,
    Settled = 2,
}

/// A funding milestone expressed in basis points of the invoice total.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentMilestone {
    pub threshold_bps: u32,
    pub state: MilestoneState,
    /// Timestamp the milestone was reached (0 while pending).
    pub reached_at: u64,
    /// Timestamp the milestone was settled (0 until settled).
    pub settled_at: u64,
}

/// Payment progress of one recipient.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecipientPaymentState {
    pub recipient: Address,
    pub due: i128,
    pub paid: i128,
    pub fully_paid: bool,
}

/// Full partial-payment tracker for one invoice.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentTracker {
    pub invoice_id: u64,
    pub recipients: Vec<RecipientPaymentState>,
    pub milestones: Vec<PaymentMilestone>,
    pub total_due: i128,
    pub total_paid: i128,
    pub last_updated: u64,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

fn tracker_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("pt_trk"), invoice_id)
}

fn paid_flags_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("pt_flags"), invoice_id)
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// Validate milestone thresholds: non-empty, strictly ascending, each in
/// `1..=10_000`, and at most [`MAX_MILESTONES`].
pub fn validate_milestones(milestones: &Vec<u32>) {
    assert!(!milestones.is_empty(), "milestones must not be empty");
    assert!(
        milestones.len() <= MAX_MILESTONES,
        "too many milestones"
    );
    let mut prev: u32 = 0;
    for m in milestones.iter() {
        assert!(m > prev, "milestones must be strictly ascending");
        assert!(m <= 10_000, "milestone basis points must be <= 10000");
        prev = m;
    }
}

/// Basis points of `total` represented by `paid`, capped at 10 000.
pub fn paid_bps(paid: i128, total: i128) -> u32 {
    if total <= 0 || paid <= 0 {
        return 0;
    }
    if paid >= total {
        return 10_000;
    }
    (paid.saturating_mul(10_000) / total) as u32
}

/// Compute the paid-flag bitmask from a recipient list.
pub fn compute_paid_flags(recipients: &Vec<RecipientPaymentState>) -> u64 {
    let mut flags: u64 = 0;
    for (i, r) in recipients.iter().enumerate() {
        if r.fully_paid {
            flags |= 1u64 << i;
        }
    }
    flags
}

// ---------------------------------------------------------------------------
// Storage access
// ---------------------------------------------------------------------------

pub fn get_tracker(env: &Env, invoice_id: u64) -> Option<PaymentTracker> {
    env.storage().persistent().get(&tracker_key(invoice_id))
}

fn load_tracker(env: &Env, invoice_id: u64) -> PaymentTracker {
    get_tracker(env, invoice_id).expect("payment tracking not initialised")
}

pub fn get_paid_flags(env: &Env, invoice_id: u64) -> u64 {
    env.storage()
        .persistent()
        .get(&paid_flags_key(invoice_id))
        .unwrap_or(0u64)
}

pub fn is_recipient_paid(env: &Env, invoice_id: u64, index: u32) -> bool {
    if index >= MAX_TRACKED_RECIPIENTS {
        return false;
    }
    get_paid_flags(env, invoice_id) & (1u64 << index) != 0
}

/// Write the tracker and its derived paid flags together.
fn store_tracker(env: &Env, tracker: &PaymentTracker) {
    let flags = compute_paid_flags(&tracker.recipients);
    env.storage()
        .persistent()
        .set(&tracker_key(tracker.invoice_id), tracker);
    env.storage()
        .persistent()
        .set(&paid_flags_key(tracker.invoice_id), &flags);
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

/// Create the tracker for `invoice_id` from the invoice's recipients and
/// amounts. `caller` must be the invoice creator.
pub fn init_tracking(
    env: &Env,
    caller: &Address,
    invoice_id: u64,
    milestone_bps: Vec<u32>,
) -> PaymentTracker {
    caller.require_auth();
    let invoice = crate::load_invoice(env, invoice_id);
    assert!(invoice.creator == *caller, "only the creator can init tracking");
    assert!(
        get_tracker(env, invoice_id).is_none(),
        "payment tracking already initialised"
    );
    let n = invoice.recipients.len();
    assert!(n > 0, "invoice has no recipients");
    assert!(
        n <= MAX_TRACKED_RECIPIENTS,
        "too many recipients for payment tracking"
    );
    validate_milestones(&milestone_bps);

    let mut recipients: Vec<RecipientPaymentState> = Vec::new(env);
    let mut total_due: i128 = 0;
    for i in 0..n {
        let due = invoice.amounts.get(i).unwrap();
        assert!(due > 0, "recipient amount must be positive");
        total_due = total_due.checked_add(due).expect("ArithmeticOverflow");
        recipients.push_back(RecipientPaymentState {
            recipient: invoice.recipients.get(i).unwrap(),
            due,
            paid: 0,
            fully_paid: false,
        });
    }

    let mut milestones: Vec<PaymentMilestone> = Vec::new(env);
    for bps in milestone_bps.iter() {
        milestones.push_back(PaymentMilestone {
            threshold_bps: bps,
            state: MilestoneState::Pending,
            reached_at: 0,
            settled_at: 0,
        });
    }

    let tracker = PaymentTracker {
        invoice_id,
        recipients,
        milestones,
        total_due,
        total_paid: 0,
        last_updated: env.ledger().timestamp(),
    };
    store_tracker(env, &tracker);
    events::payment_tracking_initialised(env, invoice_id, total_due, milestone_bps.len());
    tracker
}

/// Record a partial payment of `amount` to `recipient`. `caller` must be the
/// invoice creator. Advances any milestones crossed by the new total.
pub fn record_payment(
    env: &Env,
    caller: &Address,
    invoice_id: u64,
    recipient: &Address,
    amount: i128,
) -> PaymentTracker {
    caller.require_auth();
    let invoice = crate::load_invoice(env, invoice_id);
    assert!(invoice.creator == *caller, "only the creator can record payments");
    assert!(amount > 0, "payment amount must be positive");

    let mut tracker = load_tracker(env, invoice_id);
    let now = env.ledger().timestamp();

    // Locate and update the recipient entry in memory.
    let idx = tracker
        .recipients
        .iter()
        .position(|r| r.recipient == *recipient)
        .expect("recipient not tracked for invoice") as u32;
    let mut state = tracker.recipients.get(idx).unwrap();
    assert!(!state.fully_paid, "recipient already fully paid");
    let new_paid = state.paid.checked_add(amount).expect("ArithmeticOverflow");
    assert!(new_paid <= state.due, "payment exceeds recipient amount due");
    state.paid = new_paid;
    state.fully_paid = new_paid == state.due;
    let fully_paid = state.fully_paid;
    tracker.recipients.set(idx, state);

    tracker.total_paid = tracker
        .total_paid
        .checked_add(amount)
        .expect("ArithmeticOverflow");
    tracker.last_updated = now;

    // Advance milestones crossed by the new total.
    let progress = paid_bps(tracker.total_paid, tracker.total_due);
    let mut reached: Vec<u32> = Vec::new(env);
    for i in 0..tracker.milestones.len() {
        let mut m = tracker.milestones.get(i).unwrap();
        if m.state == MilestoneState::Pending && progress >= m.threshold_bps {
            m.state = MilestoneState::Reached;
            m.reached_at = now;
            reached.push_back(m.threshold_bps);
            tracker.milestones.set(i, m);
        }
    }

    // All validation passed — commit in one place.
    store_tracker(env, &tracker);

    events::partial_payment_recorded(env, invoice_id, recipient, amount, new_paid, fully_paid);
    for bps in reached.iter() {
        events::payment_milestone_state(env, invoice_id, bps, MilestoneState::Reached as u32);
    }
    tracker
}

/// Move milestone `index` from `Reached` to `Settled`. Milestones must be
/// settled in order. `caller` must be the invoice creator.
pub fn settle_milestone(env: &Env, caller: &Address, invoice_id: u64, index: u32) -> PaymentTracker {
    caller.require_auth();
    let invoice = crate::load_invoice(env, invoice_id);
    assert!(invoice.creator == *caller, "only the creator can settle milestones");

    let mut tracker = load_tracker(env, invoice_id);
    assert!(index < tracker.milestones.len(), "milestone index out of range");
    if index > 0 {
        let prev = tracker.milestones.get(index - 1).unwrap();
        assert!(
            prev.state == MilestoneState::Settled,
            "previous milestone not settled"
        );
    }
    let mut m = tracker.milestones.get(index).unwrap();
    assert!(m.state != MilestoneState::Settled, "milestone already settled");
    assert!(m.state == MilestoneState::Reached, "milestone not reached");
    m.state = MilestoneState::Settled;
    m.settled_at = env.ledger().timestamp();
    let bps = m.threshold_bps;
    tracker.milestones.set(index, m);
    tracker.last_updated = env.ledger().timestamp();

    store_tracker(env, &tracker);
    events::payment_milestone_state(env, invoice_id, bps, MilestoneState::Settled as u32);
    tracker
}
