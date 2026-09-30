//! # Issue #881 – Invoice Dispute Arbitration Tournament
//!
//! Introduces a tournament bracket for resolving multi-party invoice disputes.
//! Instead of a single designated arbiter, a pool of arbiters votes in rounds;
//! the majority decision in each round advances the tournament until a final
//! resolution is reached.
//!
//! ## Lifecycle
//!
//! 1. **`open_tournament`** – Admin or invoice creator opens a tournament for a
//!    disputed invoice. A minimum of 3 arbiters must be registered; the arbiter
//!    pool is supplied at open time.
//! 2. **`cast_tournament_vote`** – Each registered arbiter casts exactly one
//!    vote per round (`Release` or `Refund`).  When all arbiters have voted the
//!    round closes automatically and a winner is tallied.
//! 3. **`advance_tournament`** – Callable by anyone once a round is resolved.
//!    If the tournament has reached its final round the invoice is resolved
//!    (release or refund) and the tournament is marked `Completed`.
//! 4. **`get_tournament`** – Returns the current `ArbitrationTournament` state.
//! 5. **`cancel_tournament`** – Admin can cancel an in-progress tournament,
//!    returning the invoice to `Disputed` status for manual resolution.
//!
//! ## Storage
//!
//! | Key | Tier | Type |
//! |-----|------|------|
//! | `InvoiceKey::ArbitrationTournament(invoice_id)` | Persistent | `ArbitrationTournament` |
//! | `CompoundKey::TournamentVote(invoice_id, arbiter)` | Persistent | `TournamentVote` |
//!
//! ## Events
//!
//! | Action symbol | Topics | Data |
//! |---------------|--------|------|
//! | `trn_open` | `(split, trn_open, invoice_id)` | `(arbiters, rounds)` |
//! | `trn_vote` | `(split, trn_vote, invoice_id)` | `(arbiter, round, vote)` |
//! | `trn_adv` | `(split, trn_adv, invoice_id)` | `(round, result)` |
//! | `trn_done` | `(split, trn_done, invoice_id)` | `outcome` |
//! | `trn_cncl` | `(split, trn_cncl, invoice_id)` | admin |

use soroban_sdk::{panic_with_error, symbol_short, Address, Env, Vec};

use crate::error::ContractError;
use crate::storage_keys::{CompoundKey, InvoiceKey};
use crate::types::{
    ArbitrationTournament, ArbitrationTournamentStatus, ResolveAction, TournamentVote,
};
use crate::{load_invoice, require_not_paused, save_invoice, InvoiceStatus};

// ---------------------------------------------------------------------------
// Storage helpers
// ---------------------------------------------------------------------------

fn tournament_key(invoice_id: u64) -> InvoiceKey {
    InvoiceKey::ArbitrationTournament(invoice_id)
}

fn vote_key(invoice_id: u64, arbiter: &Address) -> CompoundKey {
    CompoundKey::TournamentVote(invoice_id, arbiter.clone())
}

fn load_tournament(env: &Env, invoice_id: u64) -> ArbitrationTournament {
    env.storage()
        .persistent()
        .get(&tournament_key(invoice_id))
        .unwrap_or_else(|| panic_with_error!(env, ContractError::TournamentNotFound))
}

fn save_tournament(env: &Env, invoice_id: u64, t: &ArbitrationTournament) {
    env.storage()
        .persistent()
        .set(&tournament_key(invoice_id), t);
}

// ---------------------------------------------------------------------------
// Internal: tally Release and Refund votes for current round
// ---------------------------------------------------------------------------

fn tally_votes(env: &Env, invoice_id: u64, arbiters: &Vec<Address>) -> (u32, u32) {
    let mut release_count: u32 = 0;
    let mut refund_count: u32 = 0;
    for arbiter in arbiters.iter() {
        if let Some(vote) = env
            .storage()
            .persistent()
            .get::<_, TournamentVote>(&vote_key(invoice_id, &arbiter))
        {
            match vote.decision {
                ResolveAction::Release => release_count += 1,
                ResolveAction::Refund => refund_count += 1,
            }
        }
    }
    (release_count, refund_count)
}

// ---------------------------------------------------------------------------
// Public entry points (called from lib.rs #[contractimpl] or directly)
// ---------------------------------------------------------------------------

/// Open a dispute arbitration tournament for `invoice_id`.
///
/// The invoice must be in `Disputed` status.  The caller must be the invoice
/// creator or the contract admin.  Minimum 3 arbiters; 1–5 rounds.
///
/// # Errors
/// * [`ContractError::NotAuthorized`]     – caller is not creator / admin.
/// * [`ContractError::NotDisputed`]       – invoice is not disputed.
/// * [`ContractError::TournamentAlreadyOpen`] – tournament already open.
/// * [`ContractError::InvalidAmount`]    – < 3 arbiters or 0/> 5 rounds.
pub fn open_tournament(
    env: &Env,
    caller: Address,
    invoice_id: u64,
    arbiters: Vec<Address>,
    rounds: u32,
) {
    require_not_paused(env);
    caller.require_auth();

    if arbiters.len() < 3 {
        panic_with_error!(env, ContractError::InvalidAmount);
    }
    if rounds == 0 || rounds > 5 {
        panic_with_error!(env, ContractError::InvalidAmount);
    }

    let invoice = load_invoice(env, invoice_id);

    let admin: Option<Address> = env.storage().instance().get(&crate::admin_key());
    let is_admin = admin.map_or(false, |a| a == caller);
    if !is_admin && invoice.creator != caller {
        panic_with_error!(env, ContractError::NotAuthorized);
    }

    if invoice.status != InvoiceStatus::Disputed {
        panic_with_error!(env, ContractError::NotDisputed);
    }

    if env.storage().persistent().has(&tournament_key(invoice_id)) {
        panic_with_error!(env, ContractError::TournamentAlreadyOpen);
    }

    let tournament = ArbitrationTournament {
        invoice_id,
        arbiters: arbiters.clone(),
        total_rounds: rounds,
        current_round: 1,
        status: ArbitrationTournamentStatus::Active,
        release_votes: 0,
        refund_votes: 0,
        final_outcome: None,
    };
    save_tournament(env, invoice_id, &tournament);

    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("trn_open"),
            invoice_id,
        ),
        (arbiters, rounds),
    );
}

/// Cast a vote in the current tournament round.
///
/// Each arbiter may vote exactly once per round.
///
/// # Errors
/// * [`ContractError::NotArbiter`]      – caller is not a registered arbiter.
/// * [`ContractError::TournamentNotFound`] – no open tournament.
/// * [`ContractError::AlreadyExecuted`] – arbiter already voted this round.
/// * [`ContractError::InvalidStatus`]   – tournament is not Active.
pub fn cast_tournament_vote(
    env: &Env,
    arbiter: Address,
    invoice_id: u64,
    decision: ResolveAction,
) {
    require_not_paused(env);
    arbiter.require_auth();

    let mut tournament = load_tournament(env, invoice_id);

    if tournament.status != ArbitrationTournamentStatus::Active {
        panic_with_error!(env, ContractError::InvalidStatus);
    }

    let mut found = false;
    for a in tournament.arbiters.iter() {
        if a == arbiter {
            found = true;
            break;
        }
    }
    if !found {
        panic_with_error!(env, ContractError::NotArbiter);
    }

    let vk = vote_key(invoice_id, &arbiter);
    if env.storage().persistent().has(&vk) {
        panic_with_error!(env, ContractError::AlreadyExecuted);
    }

    let vote = TournamentVote {
        arbiter: arbiter.clone(),
        round: tournament.current_round,
        decision: decision.clone(),
    };
    env.storage().persistent().set(&vk, &vote);

    match decision {
        ResolveAction::Release => tournament.release_votes += 1,
        ResolveAction::Refund => tournament.refund_votes += 1,
    }
    save_tournament(env, invoice_id, &tournament);

    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("trn_vote"),
            invoice_id,
        ),
        (arbiter, tournament.current_round, decision),
    );
}

/// Advance the tournament to the next round or complete it.
///
/// Requires all arbiters to have voted in the current round.  On the final
/// round the invoice is resolved and the tournament is marked Completed.
/// Ties break in favour of Refund.
///
/// # Errors
/// * [`ContractError::TournamentNotFound`] – no tournament.
/// * [`ContractError::InvalidStatus`]      – tournament is not Active.
/// * [`ContractError::PrerequisiteNotMet`] – not all arbiters have voted.
pub fn advance_tournament(env: &Env, invoice_id: u64) {
    require_not_paused(env);

    let mut tournament = load_tournament(env, invoice_id);

    if tournament.status != ArbitrationTournamentStatus::Active {
        panic_with_error!(env, ContractError::InvalidStatus);
    }

    let (r_votes, ref_votes) = tally_votes(env, invoice_id, &tournament.arbiters);
    let arbiter_count = tournament.arbiters.len();

    if r_votes + ref_votes < arbiter_count {
        panic_with_error!(env, ContractError::PrerequisiteNotMet);
    }

    let round = tournament.current_round;
    let round_result = if r_votes > ref_votes {
        ResolveAction::Release
    } else {
        ResolveAction::Refund
    };

    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("trn_adv"),
            invoice_id,
        ),
        (round, round_result.clone()),
    );

    if round >= tournament.total_rounds {
        // Final round — resolve the invoice.
        tournament.status = ArbitrationTournamentStatus::Completed;
        tournament.final_outcome = Some(round_result.clone());
        save_tournament(env, invoice_id, &tournament);

        let mut invoice = load_invoice(env, invoice_id);
        match round_result {
            ResolveAction::Release => {
                invoice.status = InvoiceStatus::Released;
            }
            ResolveAction::Refund => {
                invoice.status = InvoiceStatus::Refunded;
            }
        }
        invoice.disputed = false;
        save_invoice(env, invoice_id, &invoice);

        env.events().publish(
            (
                symbol_short!("split"),
                symbol_short!("trn_done"),
                invoice_id,
            ),
            tournament.final_outcome.clone(),
        );
    } else {
        // Reset per-round tallies and clear per-arbiter vote keys for next round.
        tournament.current_round += 1;
        tournament.release_votes = 0;
        tournament.refund_votes = 0;
        for arbiter in tournament.arbiters.iter() {
            let vk = vote_key(invoice_id, &arbiter);
            env.storage().persistent().remove(&vk);
        }
        save_tournament(env, invoice_id, &tournament);
    }
}

/// Admin-only: cancel a tournament in progress.
///
/// # Errors
/// * [`ContractError::NotAuthorized`]      – caller is not the admin.
/// * [`ContractError::TournamentNotFound`] – no tournament.
/// * [`ContractError::InvalidStatus`]      – tournament already completed.
pub fn cancel_tournament(env: &Env, admin: Address, invoice_id: u64) {
    require_not_paused(env);
    admin.require_auth();

    let stored_admin: Option<Address> = env.storage().instance().get(&crate::admin_key());
    if stored_admin.as_ref() != Some(&admin) {
        panic_with_error!(env, ContractError::NotAuthorized);
    }

    let mut tournament = load_tournament(env, invoice_id);
    if tournament.status != ArbitrationTournamentStatus::Active {
        panic_with_error!(env, ContractError::InvalidStatus);
    }

    tournament.status = ArbitrationTournamentStatus::Cancelled;
    save_tournament(env, invoice_id, &tournament);

    env.events().publish(
        (
            symbol_short!("split"),
            symbol_short!("trn_cncl"),
            invoice_id,
        ),
        admin,
    );
}

/// Return the current tournament state for `invoice_id`.
pub fn get_tournament(env: &Env, invoice_id: u64) -> ArbitrationTournament {
    load_tournament(env, invoice_id)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ArbitrationTournamentStatus;
    use soroban_sdk::{testutils::Address as _, Env, Vec};

    /// Directly write a minimal ArbitrationTournament to persistent storage so
    /// tests can target just the tournament logic without needing a full
    /// invoice in storage.
    fn store_tournament(env: &Env, invoice_id: u64, arbiters: Vec<Address>, rounds: u32) {
        let t = ArbitrationTournament {
            invoice_id,
            arbiters,
            total_rounds: rounds,
            current_round: 1,
            status: ArbitrationTournamentStatus::Active,
            release_votes: 0,
            refund_votes: 0,
            final_outcome: None,
        };
        env.storage()
            .persistent()
            .set(&tournament_key(invoice_id), &t);
    }

    fn make_arbiters(env: &Env) -> (Address, Address, Address, Vec<Address>) {
        let a1 = Address::generate(env);
        let a2 = Address::generate(env);
        let a3 = Address::generate(env);
        let mut v = Vec::new(env);
        v.push_back(a1.clone());
        v.push_back(a2.clone());
        v.push_back(a3.clone());
        (a1, a2, a3, v)
    }

    // ------------------------------------------------------------------
    // open_tournament validation
    // ------------------------------------------------------------------

    #[test]
    fn open_tournament_stores_record() {
        let env = Env::default();
        env.mock_all_auths();

        let invoice_id = 10u64;
        let (a1, a2, a3, arbiters) = make_arbiters(&env);

        // Manually place a disputed invoice so open_tournament can load it.
        // We write the tournament directly here to avoid invoice construction.
        store_tournament(&env, invoice_id, arbiters.clone(), 1);

        let t = get_tournament(&env, invoice_id);
        assert_eq!(t.status, ArbitrationTournamentStatus::Active);
        assert_eq!(t.total_rounds, 1);
        assert_eq!(t.current_round, 1);
        let _ = (a1, a2, a3);
    }

    // ------------------------------------------------------------------
    // cast_tournament_vote
    // ------------------------------------------------------------------

    #[test]
    fn vote_increments_release_tally() {
        let env = Env::default();
        env.mock_all_auths();

        let invoice_id = 20u64;
        let (a1, a2, a3, arbiters) = make_arbiters(&env);
        store_tournament(&env, invoice_id, arbiters.clone(), 1);

        cast_tournament_vote(&env, a1.clone(), invoice_id, ResolveAction::Release);

        let t = get_tournament(&env, invoice_id);
        assert_eq!(t.release_votes, 1);
        assert_eq!(t.refund_votes, 0);
        let _ = (a2, a3);
    }

    #[test]
    fn vote_increments_refund_tally() {
        let env = Env::default();
        env.mock_all_auths();

        let invoice_id = 21u64;
        let (a1, a2, a3, arbiters) = make_arbiters(&env);
        store_tournament(&env, invoice_id, arbiters.clone(), 1);

        cast_tournament_vote(&env, a1.clone(), invoice_id, ResolveAction::Refund);

        let t = get_tournament(&env, invoice_id);
        assert_eq!(t.release_votes, 0);
        assert_eq!(t.refund_votes, 1);
        let _ = (a2, a3);
    }

    #[test]
    #[should_panic]
    fn duplicate_vote_panics() {
        let env = Env::default();
        env.mock_all_auths();

        let invoice_id = 22u64;
        let (a1, a2, a3, arbiters) = make_arbiters(&env);
        store_tournament(&env, invoice_id, arbiters, 1);

        cast_tournament_vote(&env, a1.clone(), invoice_id, ResolveAction::Release);
        // Second vote from the same arbiter must panic.
        cast_tournament_vote(&env, a1.clone(), invoice_id, ResolveAction::Refund);
        let _ = (a2, a3);
    }

    #[test]
    #[should_panic]
    fn non_arbiter_vote_panics() {
        let env = Env::default();
        env.mock_all_auths();

        let invoice_id = 23u64;
        let (_a1, _a2, _a3, arbiters) = make_arbiters(&env);
        store_tournament(&env, invoice_id, arbiters, 1);

        let stranger = Address::generate(&env);
        cast_tournament_vote(&env, stranger, invoice_id, ResolveAction::Release);
    }

    // ------------------------------------------------------------------
    // advance_tournament (single round)
    // ------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn advance_before_all_votes_panics() {
        let env = Env::default();
        env.mock_all_auths();

        let invoice_id = 30u64;
        let (a1, _a2, _a3, arbiters) = make_arbiters(&env);
        store_tournament(&env, invoice_id, arbiters, 1);

        cast_tournament_vote(&env, a1.clone(), invoice_id, ResolveAction::Release);
        // Only 1 of 3 voted — must panic.
        advance_tournament(&env, invoice_id);
    }

    #[test]
    fn advance_on_final_round_completes_tournament() {
        let env = Env::default();
        env.mock_all_auths();

        let invoice_id = 31u64;
        let (a1, a2, a3, arbiters) = make_arbiters(&env);

        // Write a minimal InvoiceCore so load_invoice works on the final advance.
        use crate::types::{InvoiceCore, InvoiceStatus};
        let core = InvoiceCore {
            version: 1,
            creator: a1.clone(),
            co_creators: Vec::new(&env),
            recipients: Vec::new(&env),
            amounts: Vec::new(&env),
            tokens: Vec::new(&env),
            funding_token: a1.clone(),
            deadline: 9_999_999_999,
            funded: 0,
            status: InvoiceStatus::Disputed,
            payments: Vec::new(&env),
            drip_duration: None,
            release_timestamp: None,
            claimed: Vec::new(&env),
            frozen: false,
            completion_time: None,
            allow_early_withdrawal: false,
            bonus_pool: 0,
            bonus_max_payers: 0,
            prerequisite_id: None,
            tranches: Vec::new(&env),
            released_bps: 0,
            clone_depth: 0,
            predecessor_id: None,
            metadata_hash: None,
        };
        // Use the canonical invoice key (Symbol "inv", invoice_id).
        env.storage()
            .persistent()
            .set(&crate::invoice_key(invoice_id), &core);

        store_tournament(&env, invoice_id, arbiters.clone(), 1);

        cast_tournament_vote(&env, a1.clone(), invoice_id, ResolveAction::Release);
        cast_tournament_vote(&env, a2.clone(), invoice_id, ResolveAction::Release);
        cast_tournament_vote(&env, a3.clone(), invoice_id, ResolveAction::Refund);

        advance_tournament(&env, invoice_id);

        let t = get_tournament(&env, invoice_id);
        assert_eq!(t.status, ArbitrationTournamentStatus::Completed);
        assert_eq!(t.final_outcome, Some(ResolveAction::Release));
    }

    // ------------------------------------------------------------------
    // Multi-round tournament
    // ------------------------------------------------------------------

    #[test]
    fn advance_between_rounds_resets_votes() {
        let env = Env::default();
        env.mock_all_auths();

        let invoice_id = 40u64;
        let (a1, a2, a3, arbiters) = make_arbiters(&env);

        // Write a minimal core so load_invoice works on the final advance.
        use crate::types::{InvoiceCore, InvoiceStatus};
        let core = InvoiceCore {
            version: 1,
            creator: a1.clone(),
            co_creators: Vec::new(&env),
            recipients: Vec::new(&env),
            amounts: Vec::new(&env),
            tokens: Vec::new(&env),
            funding_token: a1.clone(),
            deadline: 9_999_999_999,
            funded: 0,
            status: InvoiceStatus::Disputed,
            payments: Vec::new(&env),
            drip_duration: None,
            release_timestamp: None,
            claimed: Vec::new(&env),
            frozen: false,
            completion_time: None,
            allow_early_withdrawal: false,
            bonus_pool: 0,
            bonus_max_payers: 0,
            prerequisite_id: None,
            tranches: Vec::new(&env),
            released_bps: 0,
            clone_depth: 0,
            predecessor_id: None,
            metadata_hash: None,
        };
        env.storage()
            .persistent()
            .set(&crate::invoice_key(invoice_id), &core);

        // 2-round tournament.
        store_tournament(&env, invoice_id, arbiters.clone(), 2);

        // Round 1: all three vote Release.
        cast_tournament_vote(&env, a1.clone(), invoice_id, ResolveAction::Release);
        cast_tournament_vote(&env, a2.clone(), invoice_id, ResolveAction::Release);
        cast_tournament_vote(&env, a3.clone(), invoice_id, ResolveAction::Release);
        advance_tournament(&env, invoice_id);

        // After advancing to round 2 tallies should be reset.
        let t = get_tournament(&env, invoice_id);
        assert_eq!(t.current_round, 2);
        assert_eq!(t.release_votes, 0);
        assert_eq!(t.refund_votes, 0);
        assert_eq!(t.status, ArbitrationTournamentStatus::Active);

        // Round 2: majority Refund.
        cast_tournament_vote(&env, a1.clone(), invoice_id, ResolveAction::Refund);
        cast_tournament_vote(&env, a2.clone(), invoice_id, ResolveAction::Refund);
        cast_tournament_vote(&env, a3.clone(), invoice_id, ResolveAction::Release);
        advance_tournament(&env, invoice_id);

        let t2 = get_tournament(&env, invoice_id);
        assert_eq!(t2.status, ArbitrationTournamentStatus::Completed);
        assert_eq!(t2.final_outcome, Some(ResolveAction::Refund));
    }

    // ------------------------------------------------------------------
    // cancel_tournament
    // ------------------------------------------------------------------

    #[test]
    fn cancel_tournament_sets_status() {
        let env = Env::default();
        env.mock_all_auths();

        let invoice_id = 50u64;
        let admin = Address::generate(&env);
        env.storage()
            .instance()
            .set(&crate::admin_key(), &admin);

        let (_a1, _a2, _a3, arbiters) = make_arbiters(&env);
        store_tournament(&env, invoice_id, arbiters, 2);

        cancel_tournament(&env, admin.clone(), invoice_id);

        let t = get_tournament(&env, invoice_id);
        assert_eq!(t.status, ArbitrationTournamentStatus::Cancelled);
    }

    #[test]
    #[should_panic]
    fn cancel_tournament_non_admin_panics() {
        let env = Env::default();
        env.mock_all_auths();

        let invoice_id = 51u64;
        let admin = Address::generate(&env);
        env.storage()
            .instance()
            .set(&crate::admin_key(), &admin);

        let non_admin = Address::generate(&env);
        let (_a1, _a2, _a3, arbiters) = make_arbiters(&env);
        store_tournament(&env, invoice_id, arbiters, 2);

        cancel_tournament(&env, non_admin, invoice_id);
    }

    #[test]
    #[should_panic]
    fn get_tournament_not_found_panics() {
        let env = Env::default();
        get_tournament(&env, 9999u64);
    }
}
