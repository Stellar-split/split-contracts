//! Issue #862: Advanced invoice bundling with smart grouping.
//!
//! A bundle is a creator-owned, named collection of their `Pending` invoices
//! that share a funding token. Bundles give payers and creators one place to
//! track a batch of related invoices:
//!
//! - `create_invoice_bundle` groups an explicit list of invoices.
//! - `smart_bundle_invoices` groups a candidate list automatically. Invoices
//!   are keyed by funding token, optionally by deadline bucket
//!   (`deadline / deadline_window_secs`) and optionally by identical recipient
//!   list; each key becomes one or more bundles of at most `max_bundle_size`.
//!   Ineligible candidates are skipped rather than rejected.
//! - `get_bundle_summary` aggregates funding progress across members.
//! - `plan_bundle_payment` splits a payment amount across outstanding members,
//!   earliest deadline first, so a payer can fund the most urgent invoices.
//!
//! An invoice belongs to at most one live bundle at a time. Bundling does not
//! alter invoice behaviour; payments are still made per invoice.

use crate::*;
use soroban_sdk::xdr::ToXdr;
use soroban_sdk::{contractimpl, contracttype, symbol_short, Address, BytesN, Env, Symbol, Vec};

/// Maximum invoices in a single bundle.
const MAX_BUNDLE_SIZE: u32 = 25;

/// Maximum candidates accepted by one `smart_bundle_invoices` call.
const MAX_SMART_CANDIDATES: u32 = 50;

/// Maximum bundles tracked per creator.
const MAX_BUNDLES_PER_CREATOR: u32 = 100;

/// Bundles must contain at least this many invoices.
const MIN_BUNDLE_SIZE: u32 = 2;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Grouping rules for `smart_bundle_invoices`. Funding token always matches.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BundleCriteria {
    /// Group invoices whose deadlines fall in the same window of this many
    /// seconds. `0` ignores deadlines.
    pub deadline_window_secs: u64,
    /// Only group invoices with an identical recipient list.
    pub match_recipients: bool,
    /// Groups smaller than this are left unbundled (>= 2).
    pub min_bundle_size: u32,
    /// Groups larger than this are split into several bundles (<= 25).
    pub max_bundle_size: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvoiceBundle {
    pub id: u64,
    pub creator: Address,
    pub label: Symbol,
    pub token: Address,
    pub invoice_ids: Vec<u64>,
    pub created_at: u64,
    pub dissolved: bool,
}

/// Funding progress aggregated across a bundle's members.
///
/// Refunded, expired, cancelled and deleted members count as `failed` and are
/// excluded from `total_due`, `total_funded` and `outstanding`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BundleSummary {
    pub bundle_id: u64,
    pub member_count: u32,
    pub pending_count: u32,
    pub released_count: u32,
    pub failed_count: u32,
    pub total_due: i128,
    pub total_funded: i128,
    pub outstanding: i128,
    /// `total_funded * 10_000 / total_due`; 10 000 when nothing is due.
    pub progress_bps: u32,
    /// Earliest deadline among open members; 0 when none are open.
    pub next_deadline: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BundleAllocation {
    pub invoice_id: u64,
    pub amount: i128,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

/// Instance storage: last issued bundle ID.
fn bundle_counter_key() -> Symbol {
    symbol_short!("bdl_ctr")
}

/// Persistent storage: bundle ID → `InvoiceBundle`.
fn bundle_key(bundle_id: u64) -> (Symbol, u64) {
    (symbol_short!("bdl_rec"), bundle_id)
}

/// Persistent storage: invoice ID → bundle ID of its live bundle.
fn invoice_bundle_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("bdl_inv"), invoice_id)
}

/// Persistent storage: creator → `Vec<u64>` of their bundle IDs.
fn creator_bundles_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("bdl_cr"), creator.clone())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn load_bundle(env: &Env, bundle_id: u64) -> InvoiceBundle {
    env.storage()
        .persistent()
        .get(&bundle_key(bundle_id))
        .expect("bundle not found")
}

fn save_bundle(env: &Env, bundle: &InvoiceBundle) {
    env.storage().persistent().set(&bundle_key(bundle.id), bundle);
}

fn load_live_bundle_for_owner(env: &Env, caller: &Address, bundle_id: u64) -> InvoiceBundle {
    let bundle = load_bundle(env, bundle_id);
    assert!(bundle.creator == *caller, "only the bundle creator may modify it");
    assert!(!bundle.dissolved, "bundle is dissolved");
    bundle
}

fn invoice_total(invoice: &Invoice) -> i128 {
    let mut total: i128 = 0;
    for amount in invoice.amounts.iter() {
        total = total.checked_add(amount).expect("invoice total overflow");
    }
    total
}

fn invoice_exists(env: &Env, invoice_id: u64) -> bool {
    env.storage().persistent().has(&invoice_key(invoice_id))
        || env.storage().instance().has(&invoice_key(invoice_id))
}

/// Whether `caller` may place `invoice` in a bundle funded with `token`.
/// When `token` is `None` any funding token is accepted.
fn is_bundleable(
    env: &Env,
    invoice_id: u64,
    invoice: &Invoice,
    caller: &Address,
    token: Option<&Address>,
) -> bool {
    let owns = invoice.creator == *caller || invoice.co_creators.contains(caller);
    let token_ok = match token {
        Some(t) => invoice.funding_token == *t,
        None => true,
    };
    owns && token_ok
        && invoice.status == InvoiceStatus::Pending
        && !env.storage().persistent().has(&invoice_bundle_key(invoice_id))
}

/// Grouping key used by smart bundling: (token, deadline bucket, recipients
/// hash). Disabled dimensions are fixed to a constant so they never split.
fn smart_group_key(
    env: &Env,
    invoice: &Invoice,
    criteria: &BundleCriteria,
) -> (Address, u64, BytesN<32>) {
    let bucket = if criteria.deadline_window_secs == 0 {
        0
    } else {
        invoice.deadline / criteria.deadline_window_secs
    };
    let recipients_hash: BytesN<32> = if criteria.match_recipients {
        env.crypto()
            .sha256(&invoice.recipients.clone().to_xdr(env))
            .into()
    } else {
        BytesN::from_array(env, &[0u8; 32])
    };
    (invoice.funding_token.clone(), bucket, recipients_hash)
}

/// Persist a new bundle, index its members and record it for the creator.
fn store_new_bundle(
    env: &Env,
    creator: &Address,
    label: Symbol,
    token: Address,
    invoice_ids: Vec<u64>,
) -> u64 {
    let mut creator_bundles: Vec<u64> = env
        .storage()
        .persistent()
        .get(&creator_bundles_key(creator))
        .unwrap_or_else(|| Vec::new(env));
    assert!(
        creator_bundles.len() < MAX_BUNDLES_PER_CREATOR,
        "creator bundle limit reached"
    );

    let bundle_id: u64 = env
        .storage()
        .instance()
        .get(&bundle_counter_key())
        .unwrap_or(0u64)
        + 1;
    env.storage().instance().set(&bundle_counter_key(), &bundle_id);

    for invoice_id in invoice_ids.iter() {
        env.storage()
            .persistent()
            .set(&invoice_bundle_key(invoice_id), &bundle_id);
    }

    let bundle = InvoiceBundle {
        id: bundle_id,
        creator: creator.clone(),
        label: label.clone(),
        token,
        invoice_ids: invoice_ids.clone(),
        created_at: env.ledger().timestamp(),
        dissolved: false,
    };
    save_bundle(env, &bundle);

    creator_bundles.push_back(bundle_id);
    env.storage()
        .persistent()
        .set(&creator_bundles_key(creator), &creator_bundles);

    events::bundle_created(env, bundle_id, creator, &label, &invoice_ids);
    bundle_id
}

// ---------------------------------------------------------------------------
// Contract entry points
// ---------------------------------------------------------------------------

#[contractimpl]
impl SplitContract {
    /// Bundle an explicit list of the caller's `Pending` invoices. All members
    /// must share a funding token and not already belong to a live bundle.
    pub fn create_invoice_bundle(env: Env, creator: Address, invoice_ids: Vec<u64>, label: Symbol) -> u64 {
        require_not_paused(&env);
        creator.require_auth();
        assert!(
            invoice_ids.len() >= MIN_BUNDLE_SIZE && invoice_ids.len() <= MAX_BUNDLE_SIZE,
            "bundle size out of range"
        );

        let first = load_invoice(&env, invoice_ids.get(0).expect("bundle is empty"));
        let token = first.funding_token.clone();
        for (i, invoice_id) in invoice_ids.iter().enumerate() {
            assert!(
                invoice_ids.first_index_of(invoice_id) == Some(i as u32),
                "duplicate invoice in bundle"
            );
            let invoice = load_invoice(&env, invoice_id);
            assert!(
                is_bundleable(&env, invoice_id, &invoice, &creator, Some(&token)),
                "invoice cannot be bundled"
            );
        }

        store_new_bundle(&env, &creator, label, token, invoice_ids)
    }

    /// Automatically bundle the eligible invoices among `candidate_ids`
    /// according to `criteria`. Returns the IDs of the bundles created (may be
    /// empty when no group reaches `min_bundle_size`).
    pub fn smart_bundle_invoices(
        env: Env,
        creator: Address,
        candidate_ids: Vec<u64>,
        criteria: BundleCriteria,
    ) -> Vec<u64> {
        require_not_paused(&env);
        creator.require_auth();
        assert!(
            candidate_ids.len() <= MAX_SMART_CANDIDATES,
            "too many candidates"
        );
        assert!(
            criteria.min_bundle_size >= MIN_BUNDLE_SIZE
                && criteria.min_bundle_size <= criteria.max_bundle_size
                && criteria.max_bundle_size <= MAX_BUNDLE_SIZE,
            "invalid bundle size criteria"
        );

        // Parallel vectors: group key and the invoice IDs collected under it.
        // A key may appear more than once when a group overflows max size.
        let mut keys: Vec<(Address, u64, BytesN<32>)> = Vec::new(&env);
        let mut groups: Vec<Vec<u64>> = Vec::new(&env);

        for (i, invoice_id) in candidate_ids.iter().enumerate() {
            if candidate_ids.first_index_of(invoice_id) != Some(i as u32) {
                continue;
            }
            if !invoice_exists(&env, invoice_id) {
                continue;
            }
            let invoice = load_invoice(&env, invoice_id);
            if !is_bundleable(&env, invoice_id, &invoice, &creator, None) {
                continue;
            }

            let key = smart_group_key(&env, &invoice, &criteria);
            let mut slot = None;
            for (g, existing) in keys.iter().enumerate() {
                if existing == key
                    && groups.get(g as u32).expect("group exists").len() < criteria.max_bundle_size
                {
                    slot = Some(g as u32);
                    break;
                }
            }
            match slot {
                Some(g) => {
                    let mut group = groups.get(g).expect("group exists");
                    group.push_back(invoice_id);
                    groups.set(g, group);
                }
                None => {
                    keys.push_back(key);
                    let mut group = Vec::new(&env);
                    group.push_back(invoice_id);
                    groups.push_back(group);
                }
            }
        }

        let mut created: Vec<u64> = Vec::new(&env);
        for (g, group) in groups.iter().enumerate() {
            if group.len() < criteria.min_bundle_size {
                continue;
            }
            let (token, _, _) = keys.get(g as u32).expect("key exists");
            let bundle_id = store_new_bundle(&env, &creator, symbol_short!("smart"), token, group);
            created.push_back(bundle_id);
        }

        events::bundles_smart_grouped(&env, &creator, candidate_ids.len(), &created);
        created
    }

    /// Add one of the caller's `Pending` invoices to their live bundle.
    pub fn add_invoice_to_bundle(env: Env, creator: Address, bundle_id: u64, invoice_id: u64) {
        require_not_paused(&env);
        creator.require_auth();
        let mut bundle = load_live_bundle_for_owner(&env, &creator, bundle_id);
        assert!(
            bundle.invoice_ids.len() < MAX_BUNDLE_SIZE,
            "bundle is full"
        );
        let invoice = load_invoice(&env, invoice_id);
        assert!(
            is_bundleable(&env, invoice_id, &invoice, &creator, Some(&bundle.token)),
            "invoice cannot be bundled"
        );

        bundle.invoice_ids.push_back(invoice_id);
        save_bundle(&env, &bundle);
        env.storage()
            .persistent()
            .set(&invoice_bundle_key(invoice_id), &bundle_id);

        events::bundle_member_changed(&env, bundle_id, invoice_id, true);
    }

    /// Remove an invoice from the caller's live bundle. A bundle must keep at
    /// least two members; dissolve it instead to release the last ones.
    pub fn remove_invoice_from_bundle(env: Env, creator: Address, bundle_id: u64, invoice_id: u64) {
        creator.require_auth();
        let mut bundle = load_live_bundle_for_owner(&env, &creator, bundle_id);
        let idx = bundle
            .invoice_ids
            .first_index_of(invoice_id)
            .expect("invoice not in bundle");
        assert!(
            bundle.invoice_ids.len() > MIN_BUNDLE_SIZE,
            "bundle must keep at least 2 invoices"
        );

        bundle.invoice_ids.remove(idx);
        save_bundle(&env, &bundle);
        env.storage()
            .persistent()
            .remove(&invoice_bundle_key(invoice_id));

        events::bundle_member_changed(&env, bundle_id, invoice_id, false);
    }

    /// Dissolve the caller's bundle, freeing every member for re-bundling.
    pub fn dissolve_invoice_bundle(env: Env, creator: Address, bundle_id: u64) {
        creator.require_auth();
        let mut bundle = load_live_bundle_for_owner(&env, &creator, bundle_id);
        for invoice_id in bundle.invoice_ids.iter() {
            env.storage()
                .persistent()
                .remove(&invoice_bundle_key(invoice_id));
        }
        bundle.dissolved = true;
        save_bundle(&env, &bundle);

        events::bundle_dissolved(&env, bundle_id, &creator);
    }

    /// Return a bundle by ID.
    pub fn get_invoice_bundle(env: Env, bundle_id: u64) -> InvoiceBundle {
        load_bundle(&env, bundle_id)
    }

    /// Return the live bundle an invoice belongs to, if any.
    pub fn get_invoice_bundle_id(env: Env, invoice_id: u64) -> Option<u64> {
        env.storage()
            .persistent()
            .get(&invoice_bundle_key(invoice_id))
    }

    /// Return every bundle ID created by `creator`, including dissolved ones.
    pub fn get_creator_bundles(env: Env, creator: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&creator_bundles_key(&creator))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Aggregate funding progress across a bundle's members.
    pub fn get_bundle_summary(env: Env, bundle_id: u64) -> BundleSummary {
        let bundle = load_bundle(&env, bundle_id);
        let mut summary = BundleSummary {
            bundle_id,
            member_count: bundle.invoice_ids.len(),
            pending_count: 0,
            released_count: 0,
            failed_count: 0,
            total_due: 0,
            total_funded: 0,
            outstanding: 0,
            progress_bps: 10_000,
            next_deadline: 0,
        };

        for invoice_id in bundle.invoice_ids.iter() {
            let invoice = load_invoice(&env, invoice_id);
            let total = invoice_total(&invoice);
            match invoice.status {
                InvoiceStatus::Released | InvoiceStatus::Finalised => {
                    summary.released_count += 1;
                    summary.total_due += total;
                    summary.total_funded += total;
                }
                InvoiceStatus::Pending
                | InvoiceStatus::Disputed
                | InvoiceStatus::PartiallyReleased => {
                    summary.pending_count += 1;
                    let funded = invoice.funded.clamp(0, total);
                    summary.total_due += total;
                    summary.total_funded += funded;
                    summary.outstanding += total - funded;
                    if summary.next_deadline == 0 || invoice.deadline < summary.next_deadline {
                        summary.next_deadline = invoice.deadline;
                    }
                }
                InvoiceStatus::Refunded
                | InvoiceStatus::Expired
                | InvoiceStatus::Cancelled
                | InvoiceStatus::Deleted => {
                    summary.failed_count += 1;
                }
            }
        }

        if summary.total_due > 0 {
            summary.progress_bps = (summary.total_funded * 10_000 / summary.total_due) as u32;
        }
        summary
    }

    /// Split `amount` across the bundle's `Pending` members, earliest deadline
    /// first (ties by position in the bundle), never exceeding any member's
    /// outstanding balance. Unallocated remainder is simply not returned.
    pub fn plan_bundle_payment(env: Env, bundle_id: u64, amount: i128) -> Vec<BundleAllocation> {
        assert!(amount > 0, "amount must be positive");
        let bundle = load_bundle(&env, bundle_id);

        // Insertion-sort open members by deadline: (deadline, invoice_id, need).
        let mut queue: Vec<(u64, u64, i128)> = Vec::new(&env);
        for invoice_id in bundle.invoice_ids.iter() {
            let invoice = load_invoice(&env, invoice_id);
            if invoice.status != InvoiceStatus::Pending {
                continue;
            }
            let total = invoice_total(&invoice);
            let need = total - invoice.funded.clamp(0, total);
            if need == 0 {
                continue;
            }
            let mut pos = queue.len();
            while pos > 0 && queue.get(pos - 1).expect("queue entry").0 > invoice.deadline {
                pos -= 1;
            }
            queue.insert(pos, (invoice.deadline, invoice_id, need));
        }

        let mut remaining = amount;
        let mut plan: Vec<BundleAllocation> = Vec::new(&env);
        for (_, invoice_id, need) in queue.iter() {
            if remaining == 0 {
                break;
            }
            let share = need.min(remaining);
            remaining -= share;
            plan.push_back(BundleAllocation {
                invoice_id,
                amount: share,
            });
        }
        plan
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
    use soroban_sdk::token::StellarAssetClient;
    use soroban_sdk::vec;

    struct Ctx {
        env: Env,
        contract_id: Address,
        token_id: Address,
        creator: Address,
        recipient: Address,
    }

    fn ctx() -> Ctx {
        let (env, contract_id, token_id) = setup_initialized();
        env.ledger().set_timestamp(1_000);
        let creator = Address::generate(&env);
        let recipient = Address::generate(&env);
        Ctx {
            env,
            contract_id,
            token_id,
            creator,
            recipient,
        }
    }

    fn invoice(t: &Ctx, amount: i128, deadline: u64) -> u64 {
        let c = client(&t.env, &t.contract_id);
        make_invoice(&t.env, &c, &t.creator, &t.recipient, amount, &t.token_id, deadline)
    }

    fn invoice_to(t: &Ctx, recipient: &Address, amount: i128, deadline: u64) -> u64 {
        let c = client(&t.env, &t.contract_id);
        make_invoice(&t.env, &c, &t.creator, recipient, amount, &t.token_id, deadline)
    }

    fn criteria(window: u64, match_recipients: bool, min: u32, max: u32) -> BundleCriteria {
        BundleCriteria {
            deadline_window_secs: window,
            match_recipients,
            min_bundle_size: min,
            max_bundle_size: max,
        }
    }

    #[test]
    fn create_bundle_indexes_members() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        let a = invoice(&t, 100, 5_000);
        let b = invoice(&t, 200, 6_000);

        let id = c.create_invoice_bundle(&t.creator, &vec![&t.env, a, b], &symbol_short!("q3"));
        assert!(!t.env.events().all().is_empty());

        let bundle = c.get_invoice_bundle(&id);
        assert_eq!(bundle.invoice_ids, vec![&t.env, a, b]);
        assert_eq!(bundle.token, t.token_id);
        assert_eq!(c.get_invoice_bundle_id(&a), Some(id));
        assert_eq!(c.get_creator_bundles(&t.creator), vec![&t.env, id]);
    }

    #[test]
    #[should_panic(expected = "bundle size out of range")]
    fn create_rejects_single_invoice() {
        let t = ctx();
        let a = invoice(&t, 100, 5_000);
        client(&t.env, &t.contract_id).create_invoice_bundle(&t.creator, &vec![&t.env, a], &symbol_short!("x"));
    }

    #[test]
    #[should_panic(expected = "duplicate invoice in bundle")]
    fn create_rejects_duplicates() {
        let t = ctx();
        let a = invoice(&t, 100, 5_000);
        client(&t.env, &t.contract_id).create_invoice_bundle(&t.creator, &vec![&t.env, a, a], &symbol_short!("x"));
    }

    #[test]
    #[should_panic(expected = "invoice cannot be bundled")]
    fn create_rejects_foreign_invoice() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        let a = invoice(&t, 100, 5_000);
        let other = Address::generate(&t.env);
        let b = make_invoice(&t.env, &c, &other, &t.recipient, 100, &t.token_id, 5_000);
        c.create_invoice_bundle(&t.creator, &vec![&t.env, a, b], &symbol_short!("x"));
    }

    #[test]
    #[should_panic(expected = "invoice cannot be bundled")]
    fn invoice_cannot_join_two_bundles() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        let a = invoice(&t, 100, 5_000);
        let b = invoice(&t, 100, 5_000);
        let d = invoice(&t, 100, 5_000);
        c.create_invoice_bundle(&t.creator, &vec![&t.env, a, b], &symbol_short!("x"));
        c.create_invoice_bundle(&t.creator, &vec![&t.env, b, d], &symbol_short!("y"));
    }

    #[test]
    #[should_panic(expected = "invoice cannot be bundled")]
    fn create_rejects_mixed_tokens() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        let a = invoice(&t, 100, 5_000);
        let other_token = t
            .env
            .register_stellar_asset_contract_v2(Address::generate(&t.env))
            .address();
        let b = make_invoice(&t.env, &c, &t.creator, &t.recipient, 100, &other_token, 5_000);
        c.create_invoice_bundle(&t.creator, &vec![&t.env, a, b], &symbol_short!("x"));
    }

    #[test]
    fn smart_groups_by_deadline_window() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        // Window of 10_000s: buckets [0, 10_000) and [10_000, 20_000).
        let a = invoice(&t, 100, 2_000);
        let b = invoice(&t, 100, 9_000);
        let d = invoice(&t, 100, 12_000);
        let e = invoice(&t, 100, 15_000);
        let lone = invoice(&t, 100, 25_000);

        let created = c.smart_bundle_invoices(
            &t.creator,
            &vec![&t.env, a, d, b, e, lone],
            &criteria(10_000, false, 2, 25),
        );

        assert_eq!(created.len(), 2);
        assert_eq!(c.get_invoice_bundle(&created.get(0).unwrap()).invoice_ids, vec![&t.env, a, b]);
        assert_eq!(c.get_invoice_bundle(&created.get(1).unwrap()).invoice_ids, vec![&t.env, d, e]);
        assert_eq!(c.get_invoice_bundle_id(&lone), None);
    }

    #[test]
    fn smart_groups_by_recipient_list() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        let other = Address::generate(&t.env);
        let a = invoice(&t, 100, 5_000);
        let b = invoice_to(&t, &other, 100, 5_000);
        let d = invoice(&t, 100, 5_000);
        let e = invoice_to(&t, &other, 100, 5_000);

        let created = c.smart_bundle_invoices(&t.creator, &vec![&t.env, a, b, d, e], &criteria(0, true, 2, 25));

        assert_eq!(created.len(), 2);
        assert_eq!(c.get_invoice_bundle(&created.get(0).unwrap()).invoice_ids, vec![&t.env, a, d]);
        assert_eq!(c.get_invoice_bundle(&created.get(1).unwrap()).invoice_ids, vec![&t.env, b, e]);
    }

    #[test]
    fn smart_splits_oversized_groups_and_skips_ineligible() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        let ids: Vec<u64> = vec![
            &t.env,
            invoice(&t, 100, 5_000),
            invoice(&t, 100, 5_000),
            invoice(&t, 100, 5_000),
            invoice(&t, 100, 5_000),
            invoice(&t, 100, 5_000),
        ];
        let mut candidates = ids.clone();
        candidates.push_back(9_999); // nonexistent
        candidates.push_back(ids.get(0).unwrap()); // duplicate

        let created = c.smart_bundle_invoices(&t.creator, &candidates, &criteria(0, false, 2, 2));

        // 5 eligible invoices in groups of 2 → [2, 2, 1]; the lone one is skipped.
        assert_eq!(created.len(), 2);
        assert_eq!(c.get_invoice_bundle_id(&ids.get(4).unwrap()), None);
    }

    #[test]
    #[should_panic(expected = "invalid bundle size criteria")]
    fn smart_rejects_bad_criteria() {
        let t = ctx();
        client(&t.env, &t.contract_id).smart_bundle_invoices(&t.creator, &Vec::new(&t.env), &criteria(0, false, 1, 25));
    }

    #[test]
    fn add_remove_and_dissolve() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        let a = invoice(&t, 100, 5_000);
        let b = invoice(&t, 100, 5_000);
        let d = invoice(&t, 100, 5_000);
        let id = c.create_invoice_bundle(&t.creator, &vec![&t.env, a, b], &symbol_short!("x"));

        c.add_invoice_to_bundle(&t.creator, &id, &d);
        assert_eq!(c.get_invoice_bundle(&id).invoice_ids.len(), 3);

        c.remove_invoice_from_bundle(&t.creator, &id, &a);
        assert_eq!(c.get_invoice_bundle(&id).invoice_ids, vec![&t.env, b, d]);
        assert_eq!(c.get_invoice_bundle_id(&a), None);

        c.dissolve_invoice_bundle(&t.creator, &id);
        assert!(c.get_invoice_bundle(&id).dissolved);
        assert_eq!(c.get_invoice_bundle_id(&b), None);

        // Freed invoices can be re-bundled.
        c.create_invoice_bundle(&t.creator, &vec![&t.env, a, b], &symbol_short!("y"));
    }

    #[test]
    #[should_panic(expected = "bundle must keep at least 2 invoices")]
    fn remove_below_minimum_panics() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        let a = invoice(&t, 100, 5_000);
        let b = invoice(&t, 100, 5_000);
        let id = c.create_invoice_bundle(&t.creator, &vec![&t.env, a, b], &symbol_short!("x"));
        c.remove_invoice_from_bundle(&t.creator, &id, &a);
    }

    #[test]
    #[should_panic(expected = "only the bundle creator may modify it")]
    fn non_owner_cannot_dissolve() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        let a = invoice(&t, 100, 5_000);
        let b = invoice(&t, 100, 5_000);
        let id = c.create_invoice_bundle(&t.creator, &vec![&t.env, a, b], &symbol_short!("x"));
        c.dissolve_invoice_bundle(&Address::generate(&t.env), &id);
    }

    #[test]
    fn summary_tracks_progress_across_members() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        let a = invoice(&t, 100, 5_000);
        let b = invoice(&t, 300, 4_000);
        let d = invoice(&t, 50, 6_000);
        let id = c.create_invoice_bundle(&t.creator, &vec![&t.env, a, b, d], &symbol_short!("x"));

        let payer = Address::generate(&t.env);
        StellarAssetClient::new(&t.env, &t.token_id).mint(&payer, &1_000);
        c.pay(&payer, &a, &100_i128, &0_u64, &false, &false, &None); // releases a
        c.pay(&payer, &b, &150_i128, &0_u64, &false, &false, &None); // half of b
        c.cancel_invoice(&t.creator, &d);

        let s = c.get_bundle_summary(&id);
        assert_eq!(s.member_count, 3);
        assert_eq!(s.released_count, 1);
        assert_eq!(s.pending_count, 1);
        assert_eq!(s.failed_count, 1);
        assert_eq!(s.total_due, 400);
        assert_eq!(s.total_funded, 250);
        assert_eq!(s.outstanding, 150);
        assert_eq!(s.progress_bps, 6_250);
        assert_eq!(s.next_deadline, 4_000);
    }

    #[test]
    fn payment_plan_fills_earliest_deadline_first() {
        let t = ctx();
        let c = client(&t.env, &t.contract_id);
        let late = invoice(&t, 100, 9_000);
        let early = invoice(&t, 100, 3_000);
        let mid = invoice(&t, 100, 6_000);
        let id = c.create_invoice_bundle(&t.creator, &vec![&t.env, late, early, mid], &symbol_short!("x"));

        let plan = c.plan_bundle_payment(&id, &250);
        assert_eq!(
            plan,
            vec![
                &t.env,
                BundleAllocation { invoice_id: early, amount: 100 },
                BundleAllocation { invoice_id: mid, amount: 100 },
                BundleAllocation { invoice_id: late, amount: 50 },
            ]
        );

        // Over-allocation is capped at what is outstanding.
        assert_eq!(c.plan_bundle_payment(&id, &10_000).len(), 3);
    }
}
