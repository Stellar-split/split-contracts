//! Issue #918: Invoice prioritization.
//!
//! Lets creators (and admins) attach a priority score to a pending invoice so
//! off-chain keepers and on-chain processing can order work and allocate
//! resources by importance.
//!
//! ## Tier system
//! Every score in `0..=MAX_PRIORITY_SCORE` maps to exactly one tier:
//!
//! | Tier       | Score range | Weight |
//! |------------|-------------|--------|
//! | `Low`      | 0 – 249     | 1      |
//! | `Normal`   | 250 – 499   | 2      |
//! | `High`     | 500 – 749   | 4      |
//! | `Critical` | 750 – 1000  | 8      |
//!
//! The weight is the relative share of processing resources an invoice in
//! that tier should receive (e.g. keeper batch slots).
//!
//! ## Ordering
//! Invoices are processed by descending score; equal scores fall back to the
//! lower invoice id first (FIFO, since ids are allocated monotonically).
//!
//! ## Authorization
//! * The creator or a co-creator may set any score below the `Critical` tier.
//! * Admins (`Operator` or `SuperAdmin`) may set any score, including `Critical`,
//!   on any invoice.
//!
//! ## Storage
//! * `(ipri_rec, invoice_id)` → [`InvoicePriority`] (persistent)
//! * `(ipri_q, tier)` → `Vec<(u64, u32)>` per-tier queue of `(invoice_id, score)`,
//!   kept sorted in processing order (persistent)

use soroban_sdk::{contractimpl, contracttype, symbol_short, Address, Env, Map, Symbol, Vec};

use crate::events;
use crate::types::{AdminRole, InvoiceStatus};
use crate::{admins_key, load_invoice, SplitContract, SplitContractArgs, SplitContractClient};

/// Highest accepted priority score (inclusive).
pub const MAX_PRIORITY_SCORE: u32 = 1_000;

/// Maximum number of invoice ids accepted by `sort_invoices_by_priority`.
pub const MAX_PRIORITY_SORT_BATCH: u32 = 50;

/// Maximum number of invoices tracked in a single tier queue.
pub const MAX_PRIORITY_QUEUE_LEN: u32 = 200;

/// Lowest score that falls in the `Normal` tier.
const NORMAL_TIER_MIN_SCORE: u32 = 250;
/// Lowest score that falls in the `High` tier.
const HIGH_TIER_MIN_SCORE: u32 = 500;
/// Lowest score that falls in the `Critical` tier.
const CRITICAL_TIER_MIN_SCORE: u32 = 750;

/// Priority tier derived from an invoice's score.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum PriorityTier {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

impl PriorityTier {
    /// Map a (validated) score onto its tier.
    pub fn from_score(score: u32) -> Self {
        if score >= CRITICAL_TIER_MIN_SCORE {
            PriorityTier::Critical
        } else if score >= HIGH_TIER_MIN_SCORE {
            PriorityTier::High
        } else if score >= NORMAL_TIER_MIN_SCORE {
            PriorityTier::Normal
        } else {
            PriorityTier::Low
        }
    }

    /// Relative share of processing resources for invoices in this tier.
    pub fn weight(self) -> u32 {
        match self {
            PriorityTier::Low => 1,
            PriorityTier::Normal => 2,
            PriorityTier::High => 4,
            PriorityTier::Critical => 8,
        }
    }

    /// All tiers from most to least urgent.
    fn descending() -> [PriorityTier; 4] {
        [
            PriorityTier::Critical,
            PriorityTier::High,
            PriorityTier::Normal,
            PriorityTier::Low,
        ]
    }
}

/// Priority record stored per invoice.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvoicePriority {
    pub invoice_id: u64,
    /// Score in `0..=MAX_PRIORITY_SCORE`; higher is processed first.
    pub score: u32,
    /// Tier derived from `score` at the time it was set.
    pub tier: PriorityTier,
    /// Address that last set the priority (creator, co-creator or admin).
    pub set_by: Address,
    /// Ledger timestamp of the last update.
    pub updated_at: u64,
}

fn priority_record_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("ipri_rec"), invoice_id)
}

fn priority_queue_key(tier: PriorityTier) -> (Symbol, u32) {
    (symbol_short!("ipri_q"), tier as u32)
}

/// `true` when `(a_score, a_id)` must be processed before `(b_score, b_id)`.
fn ranks_before(a_score: u32, a_id: u64, b_score: u32, b_id: u64) -> bool {
    a_score > b_score || (a_score == b_score && a_id < b_id)
}

/// Index at which `(invoice_id, score)` must be inserted to keep `entries`
/// in processing order. Binary search — O(log n) comparisons.
fn insertion_index(entries: &Vec<(u64, u32)>, invoice_id: u64, score: u32) -> u32 {
    let mut lo: u32 = 0;
    let mut hi: u32 = entries.len();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let (mid_id, mid_score) = entries.get(mid).unwrap();
        if ranks_before(mid_score, mid_id, score, invoice_id) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

fn load_queue(env: &Env, tier: PriorityTier) -> Vec<(u64, u32)> {
    env.storage()
        .persistent()
        .get(&priority_queue_key(tier))
        .unwrap_or(Vec::new(env))
}

fn save_queue(env: &Env, tier: PriorityTier, queue: &Vec<(u64, u32)>) {
    if queue.is_empty() {
        env.storage().persistent().remove(&priority_queue_key(tier));
    } else {
        env.storage().persistent().set(&priority_queue_key(tier), queue);
    }
}

fn remove_from_queue(env: &Env, tier: PriorityTier, invoice_id: u64) {
    let mut queue = load_queue(env, tier);
    if let Some(pos) = queue.iter().position(|(id, _)| id == invoice_id) {
        queue.remove(pos as u32);
        save_queue(env, tier, &queue);
    }
}

fn insert_into_queue(env: &Env, tier: PriorityTier, invoice_id: u64, score: u32) {
    let mut queue = load_queue(env, tier);
    assert!(
        queue.len() < MAX_PRIORITY_QUEUE_LEN,
        "priority queue is full for this tier"
    );
    let idx = insertion_index(&queue, invoice_id, score);
    queue.insert(idx, (invoice_id, score));
    save_queue(env, tier, &queue);
}

/// `true` when `caller` holds `Operator` or `SuperAdmin`. Read-only: does not
/// call `require_auth` (the caller has already been authenticated).
fn is_priority_admin(env: &Env, caller: &Address) -> bool {
    env.storage()
        .instance()
        .get::<_, Map<Address, AdminRole>>(&admins_key())
        .and_then(|admins| admins.get(caller.clone()))
        .is_some()
}

/// Validate the score and the caller's right to apply it to `invoice_id`.
fn validate_priority_update(env: &Env, caller: &Address, invoice_id: u64, score: u32) {
    assert!(score <= MAX_PRIORITY_SCORE, "priority score out of range");

    let invoice = load_invoice(env, invoice_id);
    assert!(
        invoice.status == InvoiceStatus::Pending
            || invoice.status == InvoiceStatus::PartiallyReleased,
        "invoice is not open for prioritization"
    );

    let is_creator =
        invoice.creator == *caller || invoice.co_creators.iter().any(|c| c == *caller);
    let is_admin = is_priority_admin(env, caller);
    assert!(is_creator || is_admin, "NotAuthorized: cannot prioritize this invoice");

    if PriorityTier::from_score(score) == PriorityTier::Critical {
        assert!(is_admin, "only admins may assign Critical priority");
    }
}

#[contractimpl]
impl SplitContract {
    /// Issue #918: Set (or update) the priority score of an open invoice.
    ///
    /// Moves the invoice between tier queues when its tier changes and keeps
    /// each queue sorted in processing order.
    ///
    /// # Panics
    /// * `"priority score out of range"` — `score > MAX_PRIORITY_SCORE`.
    /// * `"invoice is not open for prioritization"` — invoice is finalised,
    ///   refunded, cancelled, etc.
    /// * `"NotAuthorized: ..."` — caller is neither creator/co-creator nor admin.
    /// * `"only admins may assign Critical priority"`.
    /// * `"priority queue is full for this tier"`.
    pub fn set_invoice_priority(
        env: Env,
        caller: Address,
        invoice_id: u64,
        score: u32,
    ) -> InvoicePriority {
        crate::require_not_paused(&env);
        caller.require_auth();
        validate_priority_update(&env, &caller, invoice_id, score);

        let tier = PriorityTier::from_score(score);
        let previous: Option<InvoicePriority> = env
            .storage()
            .persistent()
            .get(&priority_record_key(invoice_id));
        if let Some(ref prev) = previous {
            remove_from_queue(&env, prev.tier, invoice_id);
        }
        insert_into_queue(&env, tier, invoice_id, score);

        let record = InvoicePriority {
            invoice_id,
            score,
            tier,
            set_by: caller.clone(),
            updated_at: env.ledger().timestamp(),
        };
        env.storage()
            .persistent()
            .set(&priority_record_key(invoice_id), &record);

        let old_score = previous.map(|p| p.score);
        events::invoice_priority_set(&env, invoice_id, &caller, old_score, score, tier as u32);
        record
    }

    /// Issue #918: Remove an invoice's priority and drop it from its tier queue.
    ///
    /// Callable by the creator, a co-creator or an admin. Clearing a
    /// `Critical` priority requires an admin.
    pub fn clear_invoice_priority(env: Env, caller: Address, invoice_id: u64) {
        crate::require_not_paused(&env);
        caller.require_auth();
        let record: InvoicePriority = env
            .storage()
            .persistent()
            .get(&priority_record_key(invoice_id))
            .expect("invoice has no priority");

        let is_admin = is_priority_admin(&env, &caller);
        if !is_admin {
            let invoice = load_invoice(&env, invoice_id);
            let is_creator =
                invoice.creator == caller || invoice.co_creators.iter().any(|c| c == caller);
            assert!(is_creator, "NotAuthorized: cannot prioritize this invoice");
            assert!(
                record.tier != PriorityTier::Critical,
                "only admins may assign Critical priority"
            );
        }

        remove_from_queue(&env, record.tier, invoice_id);
        env.storage()
            .persistent()
            .remove(&priority_record_key(invoice_id));
        events::invoice_priority_cleared(&env, invoice_id, &caller);
    }

    /// Issue #918: Priority record for an invoice, if one has been set.
    pub fn get_invoice_priority(env: Env, invoice_id: u64) -> Option<InvoicePriority> {
        env.storage()
            .persistent()
            .get(&priority_record_key(invoice_id))
    }

    /// Issue #918: Tier a given score maps to (pure lookup).
    pub fn get_priority_tier_for_score(_env: Env, score: u32) -> PriorityTier {
        assert!(score <= MAX_PRIORITY_SCORE, "priority score out of range");
        PriorityTier::from_score(score)
    }

    /// Issue #918: Resource-allocation weight of a tier.
    pub fn get_priority_tier_weight(_env: Env, tier: PriorityTier) -> u32 {
        tier.weight()
    }

    /// Issue #918: Invoice ids queued in `tier`, in processing order.
    pub fn get_priority_queue(env: Env, tier: PriorityTier) -> Vec<u64> {
        let mut ids = Vec::new(&env);
        for (id, _) in load_queue(&env, tier).iter() {
            ids.push_back(id);
        }
        ids
    }

    /// Issue #918: The invoice that should be processed next — the head of the
    /// most urgent non-empty tier queue. `None` when nothing is queued.
    pub fn get_next_priority_invoice(env: Env) -> Option<u64> {
        for tier in PriorityTier::descending() {
            if let Some((id, _)) = load_queue(&env, tier).first() {
                return Some(id);
            }
        }
        None
    }

    /// Issue #918: Sort `invoice_ids` into processing order (descending score,
    /// then ascending id). Invoices without a priority are treated as score 0.
    ///
    /// Bounded by `MAX_PRIORITY_SORT_BATCH`; uses binary insertion so the
    /// comparison count is O(n log n).
    pub fn sort_invoices_by_priority(env: Env, invoice_ids: Vec<u64>) -> Vec<u64> {
        assert!(
            invoice_ids.len() <= MAX_PRIORITY_SORT_BATCH,
            "too many invoices to sort"
        );
        let mut ranked: Vec<(u64, u32)> = Vec::new(&env);
        for id in invoice_ids.iter() {
            let score = env
                .storage()
                .persistent()
                .get::<_, InvoicePriority>(&priority_record_key(id))
                .map(|p| p.score)
                .unwrap_or(0);
            let idx = insertion_index(&ranked, id, score);
            ranked.insert(idx, (id, score));
        }
        let mut sorted = Vec::new(&env);
        for (id, _) in ranked.iter() {
            sorted.push_back(id);
        }
        sorted
    }

    /// Issue #918: Drop queue entries for invoices that are no longer open
    /// (released, refunded, cancelled, ...). Permissionless housekeeping;
    /// returns the number of entries removed.
    pub fn prune_priority_queue(env: Env, tier: PriorityTier) -> u32 {
        let queue = load_queue(&env, tier);
        let mut kept: Vec<(u64, u32)> = Vec::new(&env);
        let mut removed: u32 = 0;
        for (id, score) in queue.iter() {
            let key = crate::invoice_key(id);
            let exists =
                env.storage().persistent().has(&key) || env.storage().instance().has(&key);
            let open = exists
                && {
                    let status = load_invoice(&env, id).status;
                    status == InvoiceStatus::Pending || status == InvoiceStatus::PartiallyReleased
                };
            if open {
                kept.push_back((id, score));
            } else {
                env.storage().persistent().remove(&priority_record_key(id));
                removed += 1;
            }
        }
        if removed > 0 {
            save_queue(&env, tier, &kept);
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_from_score_boundaries() {
        assert_eq!(PriorityTier::from_score(0), PriorityTier::Low);
        assert_eq!(PriorityTier::from_score(249), PriorityTier::Low);
        assert_eq!(PriorityTier::from_score(250), PriorityTier::Normal);
        assert_eq!(PriorityTier::from_score(499), PriorityTier::Normal);
        assert_eq!(PriorityTier::from_score(500), PriorityTier::High);
        assert_eq!(PriorityTier::from_score(749), PriorityTier::High);
        assert_eq!(PriorityTier::from_score(750), PriorityTier::Critical);
        assert_eq!(PriorityTier::from_score(MAX_PRIORITY_SCORE), PriorityTier::Critical);
    }

    #[test]
    fn test_tier_weights_increase_with_urgency() {
        let tiers = PriorityTier::descending();
        for i in 1..tiers.len() {
            assert!(tiers[i - 1].weight() > tiers[i].weight());
        }
    }

    #[test]
    fn test_ranks_before_orders_by_score_then_id() {
        assert!(ranks_before(10, 5, 9, 1));
        assert!(ranks_before(10, 1, 10, 2));
        assert!(!ranks_before(10, 2, 10, 1));
        assert!(!ranks_before(9, 1, 10, 5));
    }

    #[test]
    fn test_insertion_index_keeps_processing_order() {
        let env = Env::default();
        let mut queue: Vec<(u64, u32)> = Vec::new(&env);
        for (id, score) in [(1u64, 100u32), (2, 300), (3, 300), (4, 50), (5, 900)] {
            let idx = insertion_index(&queue, id, score);
            queue.insert(idx, (id, score));
        }
        let mut order = Vec::new(&env);
        for (id, _) in queue.iter() {
            order.push_back(id);
        }
        let expected = Vec::from_array(&env, [5u64, 2, 3, 1, 4]);
        assert_eq!(order, expected);
    }

    #[test]
    fn test_sort_invoices_without_priority_preserves_id_order() {
        let env = Env::default();
        let contract_id = env.register(SplitContract, ());
        let client = SplitContractClient::new(&env, &contract_id);
        let ids = Vec::from_array(&env, [7u64, 3, 9]);
        let sorted = client.sort_invoices_by_priority(&ids);
        assert_eq!(sorted, Vec::from_array(&env, [3u64, 7, 9]));
    }

    #[test]
    #[should_panic(expected = "priority score out of range")]
    fn test_tier_for_score_rejects_out_of_range() {
        let env = Env::default();
        let contract_id = env.register(SplitContract, ());
        let client = SplitContractClient::new(&env, &contract_id);
        client.get_priority_tier_for_score(&(MAX_PRIORITY_SCORE + 1));
    }
}
