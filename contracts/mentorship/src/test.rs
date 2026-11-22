#![cfg(test)]

extern crate std;

use crate::{
    Error, MentorshipContract, MentorshipContractClient, ProgramStatus, MAX_REVENUE_SHARE_BPS,
    MIN_DURATION_LEDGERS,
};
use soroban_sdk::testutils::{Address as _, Events as _, Ledger};
use soroban_sdk::{symbol_short, token, Address, Env, Symbol, TryFromVal, Val};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup() -> (Env, MentorshipContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MentorshipContract, ());
    let client = MentorshipContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin)
}

fn create_token(env: &Env, holder: &Address, amount: i128) -> Address {
    let admin = Address::generate(env);
    let token_id = env.register_stellar_asset_contract_v2(admin).address();
    token::StellarAssetClient::new(env, &token_id).mint(holder, &amount);
    token_id
}

fn register_default_mentor(env: &Env, client: &MentorshipContractClient) -> Address {
    let mentor = Address::generate(env);
    client.register_mentor(&mentor, &symbol_short!("design"), &1_000, &2);
    mentor
}

fn active_program(env: &Env, client: &MentorshipContractClient) -> (Address, Address, u64) {
    let mentor = register_default_mentor(env, client);
    let mentee = Address::generate(env);
    let id = client.request_mentorship(&mentee, &mentor, &MIN_DURATION_LEDGERS);
    client.accept_mentorship(&mentor, &id);
    (mentor, mentee, id)
}

/// Returns true if an event with topics `(mentor, action, ..)` was emitted.
fn has_event(env: &Env, action: Symbol) -> bool {
    for event in env.events().all().events().iter() {
        let soroban_sdk::xdr::ContractEventBody::V0(body) = &event.body;
        let topics: &[soroban_sdk::xdr::ScVal] = body.topics.as_ref();
        if topics.len() < 2 {
            continue;
        }
        let t0 = Val::try_from_val(env, &topics[0]).ok();
        let t1 = Val::try_from_val(env, &topics[1]).ok();
        if let (Some(t0), Some(t1)) = (t0, t1) {
            let s0 = Symbol::try_from_val(env, &t0).ok();
            let s1 = Symbol::try_from_val(env, &t1).ok();
            if s0 == Some(symbol_short!("mentor")) && s1 == Some(action.clone()) {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// initialize
// ---------------------------------------------------------------------------

#[test]
fn test_initialize_twice_fails() {
    let (_env, client, admin) = setup();
    assert_eq!(client.try_initialize(&admin), Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn test_register_before_initialize_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let client = MentorshipContractClient::new(&env, &env.register(MentorshipContract, ()));
    let mentor = Address::generate(&env);
    assert_eq!(
        client.try_register_mentor(&mentor, &symbol_short!("music"), &500, &1),
        Err(Ok(Error::NotInitialized))
    );
}

// ---------------------------------------------------------------------------
// Mentor registration
// ---------------------------------------------------------------------------

#[test]
fn test_register_mentor_success() {
    let (env, client, _admin) = setup();
    let mentor = register_default_mentor(&env, &client);
    assert!(has_event(&env, symbol_short!("reg")));

    let profile = client.get_mentor(&mentor).unwrap();
    assert_eq!(profile.expertise, symbol_short!("design"));
    assert_eq!(profile.revenue_share_bps, 1_000);
    assert_eq!(profile.max_mentees, 2);
    assert_eq!(profile.active_mentees, 0);
    assert!(profile.available);
    assert!(!profile.suspended);
}

#[test]
fn test_register_mentor_twice_fails() {
    let (env, client, _admin) = setup();
    let mentor = register_default_mentor(&env, &client);
    assert_eq!(
        client.try_register_mentor(&mentor, &symbol_short!("design"), &1_000, &2),
        Err(Ok(Error::MentorAlreadyRegistered))
    );
}

#[test]
fn test_register_mentor_share_too_high_fails() {
    let (env, client, _admin) = setup();
    let mentor = Address::generate(&env);
    assert_eq!(
        client.try_register_mentor(&mentor, &symbol_short!("dev"), &(MAX_REVENUE_SHARE_BPS + 1), &1),
        Err(Ok(Error::InvalidRevenueShare))
    );
}

#[test]
fn test_register_mentor_zero_capacity_fails() {
    let (env, client, _admin) = setup();
    let mentor = Address::generate(&env);
    assert_eq!(
        client.try_register_mentor(&mentor, &symbol_short!("dev"), &100, &0),
        Err(Ok(Error::InvalidCapacity))
    );
}

#[test]
fn test_update_mentor_changes_terms() {
    let (env, client, _admin) = setup();
    let mentor = register_default_mentor(&env, &client);
    client.update_mentor(&mentor, &2_000, &5, &false);
    assert!(has_event(&env, symbol_short!("updated")));

    let profile = client.get_mentor(&mentor).unwrap();
    assert_eq!(profile.revenue_share_bps, 2_000);
    assert_eq!(profile.max_mentees, 5);
    assert!(!profile.available);
}

#[test]
fn test_update_mentor_capacity_below_active_fails() {
    let (env, client, _admin) = setup();
    let (mentor, _mentee, _id) = active_program(&env, &client);
    let mentee2 = Address::generate(&env);
    let id2 = client.request_mentorship(&mentee2, &mentor, &MIN_DURATION_LEDGERS);
    client.accept_mentorship(&mentor, &id2);
    assert_eq!(
        client.try_update_mentor(&mentor, &1_000, &1, &true),
        Err(Ok(Error::InvalidCapacity))
    );
}

#[test]
fn test_admin_suspends_mentor() {
    let (env, client, admin) = setup();
    let mentor = register_default_mentor(&env, &client);
    client.set_mentor_suspended(&admin, &mentor, &true);
    assert!(has_event(&env, symbol_short!("suspend")));
    assert!(client.get_mentor(&mentor).unwrap().suspended);

    let mentee = Address::generate(&env);
    assert_eq!(
        client.try_request_mentorship(&mentee, &mentor, &MIN_DURATION_LEDGERS),
        Err(Ok(Error::MentorUnavailable))
    );
}

#[test]
fn test_non_admin_cannot_suspend() {
    let (env, client, _admin) = setup();
    let mentor = register_default_mentor(&env, &client);
    let rando = Address::generate(&env);
    assert_eq!(
        client.try_set_mentor_suspended(&rando, &mentor, &true),
        Err(Ok(Error::NotAdmin))
    );
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

#[test]
fn test_request_mentorship_success() {
    let (env, client, _admin) = setup();
    let mentor = register_default_mentor(&env, &client);
    let mentee = Address::generate(&env);
    let id = client.request_mentorship(&mentee, &mentor, &MIN_DURATION_LEDGERS);
    assert_eq!(id, 0);
    assert!(has_event(&env, symbol_short!("requested")));

    let program = client.get_program(&id).unwrap();
    assert_eq!(program.status, ProgramStatus::Requested);
    assert_eq!(program.mentor, mentor);
    assert_eq!(program.mentee, mentee);
    assert_eq!(client.get_mentor_programs(&mentor).len(), 1);
    assert_eq!(client.get_mentee_programs(&mentee).len(), 1);
}

#[test]
fn test_request_self_mentorship_fails() {
    let (env, client, _admin) = setup();
    let mentor = register_default_mentor(&env, &client);
    assert_eq!(
        client.try_request_mentorship(&mentor, &mentor, &MIN_DURATION_LEDGERS),
        Err(Ok(Error::SelfMentorship))
    );
}

#[test]
fn test_request_unknown_mentor_fails() {
    let (env, client, _admin) = setup();
    let mentee = Address::generate(&env);
    let nobody = Address::generate(&env);
    assert_eq!(
        client.try_request_mentorship(&mentee, &nobody, &MIN_DURATION_LEDGERS),
        Err(Ok(Error::MentorNotFound))
    );
}

#[test]
fn test_request_invalid_duration_fails() {
    let (env, client, _admin) = setup();
    let mentor = register_default_mentor(&env, &client);
    let mentee = Address::generate(&env);
    assert_eq!(
        client.try_request_mentorship(&mentee, &mentor, &(MIN_DURATION_LEDGERS - 1)),
        Err(Ok(Error::InvalidDuration))
    );
}

#[test]
fn test_duplicate_open_request_fails() {
    let (env, client, _admin) = setup();
    let mentor = register_default_mentor(&env, &client);
    let mentee = Address::generate(&env);
    client.request_mentorship(&mentee, &mentor, &MIN_DURATION_LEDGERS);
    assert_eq!(
        client.try_request_mentorship(&mentee, &mentor, &MIN_DURATION_LEDGERS),
        Err(Ok(Error::DuplicateProgram))
    );
}

#[test]
fn test_request_unavailable_mentor_fails() {
    let (env, client, _admin) = setup();
    let mentor = register_default_mentor(&env, &client);
    client.update_mentor(&mentor, &1_000, &2, &false);
    let mentee = Address::generate(&env);
    assert_eq!(
        client.try_request_mentorship(&mentee, &mentor, &MIN_DURATION_LEDGERS),
        Err(Ok(Error::MentorUnavailable))
    );
}

// ---------------------------------------------------------------------------
// Accept / decline
// ---------------------------------------------------------------------------

#[test]
fn test_accept_starts_program() {
    let (env, client, _admin) = setup();
    let (mentor, _mentee, id) = active_program(&env, &client);
    assert!(has_event(&env, symbol_short!("accepted")));

    let program = client.get_program(&id).unwrap();
    assert_eq!(program.status, ProgramStatus::Active);
    assert_eq!(program.end_ledger, program.start_ledger + MIN_DURATION_LEDGERS);
    assert_eq!(program.revenue_share_bps, 1_000);
    assert_eq!(client.get_mentor(&mentor).unwrap().active_mentees, 1);
}

#[test]
fn test_accept_by_other_address_fails() {
    let (env, client, _admin) = setup();
    let mentor = register_default_mentor(&env, &client);
    let mentee = Address::generate(&env);
    let id = client.request_mentorship(&mentee, &mentor, &MIN_DURATION_LEDGERS);
    let rando = Address::generate(&env);
    assert_eq!(client.try_accept_mentorship(&rando, &id), Err(Ok(Error::Unauthorized)));
}

#[test]
fn test_accept_twice_fails() {
    let (env, client, _admin) = setup();
    let (mentor, _mentee, id) = active_program(&env, &client);
    assert_eq!(
        client.try_accept_mentorship(&mentor, &id),
        Err(Ok(Error::InvalidProgramState))
    );
}

#[test]
fn test_accept_at_capacity_fails() {
    let (env, client, _admin) = setup();
    let mentor = Address::generate(&env);
    client.register_mentor(&mentor, &symbol_short!("art"), &500, &1);
    let mentee_a = Address::generate(&env);
    let mentee_b = Address::generate(&env);
    let a = client.request_mentorship(&mentee_a, &mentor, &MIN_DURATION_LEDGERS);
    let b = client.request_mentorship(&mentee_b, &mentor, &MIN_DURATION_LEDGERS);
    client.accept_mentorship(&mentor, &a);
    assert_eq!(
        client.try_accept_mentorship(&mentor, &b),
        Err(Ok(Error::MentorAtCapacity))
    );
}

#[test]
fn test_decline_frees_pair_for_new_request() {
    let (env, client, _admin) = setup();
    let mentor = register_default_mentor(&env, &client);
    let mentee = Address::generate(&env);
    let id = client.request_mentorship(&mentee, &mentor, &MIN_DURATION_LEDGERS);
    client.decline_mentorship(&mentor, &id);
    assert!(has_event(&env, symbol_short!("declined")));
    assert_eq!(client.get_program(&id).unwrap().status, ProgramStatus::Declined);

    let id2 = client.request_mentorship(&mentee, &mentor, &MIN_DURATION_LEDGERS);
    assert_eq!(id2, id + 1);
}

// ---------------------------------------------------------------------------
// Sessions & revenue share
// ---------------------------------------------------------------------------

#[test]
fn test_log_session_increments() {
    let (env, client, _admin) = setup();
    let (mentor, _mentee, id) = active_program(&env, &client);
    assert_eq!(client.log_session(&mentor, &id), 1);
    assert_eq!(client.log_session(&mentor, &id), 2);
    assert!(has_event(&env, symbol_short!("session")));
}

#[test]
fn test_log_session_on_requested_fails() {
    let (env, client, _admin) = setup();
    let mentor = register_default_mentor(&env, &client);
    let mentee = Address::generate(&env);
    let id = client.request_mentorship(&mentee, &mentor, &MIN_DURATION_LEDGERS);
    assert_eq!(
        client.try_log_session(&mentor, &id),
        Err(Ok(Error::InvalidProgramState))
    );
}

#[test]
fn test_share_earnings_transfers_share() {
    let (env, client, _admin) = setup();
    let (mentor, mentee, id) = active_program(&env, &client);
    let token_id = create_token(&env, &mentee, 10_000);

    let share = client.share_earnings(&mentee, &id, &token_id, &5_000);
    assert_eq!(share, 500);
    assert!(has_event(&env, symbol_short!("shared")));

    let tok = token::Client::new(&env, &token_id);
    assert_eq!(tok.balance(&mentor), 500);
    assert_eq!(tok.balance(&mentee), 9_500);
    assert_eq!(client.get_program(&id).unwrap().total_shared, 500);
}

#[test]
fn test_share_earnings_zero_amount_fails() {
    let (env, client, _admin) = setup();
    let (_mentor, mentee, id) = active_program(&env, &client);
    let token_id = create_token(&env, &mentee, 100);
    assert_eq!(
        client.try_share_earnings(&mentee, &id, &token_id, &0),
        Err(Ok(Error::InvalidAmount))
    );
}

#[test]
fn test_share_earnings_after_term_fails() {
    let (env, client, _admin) = setup();
    let (_mentor, mentee, id) = active_program(&env, &client);
    let token_id = create_token(&env, &mentee, 10_000);
    let end = client.get_program(&id).unwrap().end_ledger;
    env.ledger().with_mut(|l| l.sequence_number = end + 1);
    assert_eq!(
        client.try_share_earnings(&mentee, &id, &token_id, &1_000),
        Err(Ok(Error::ProgramExpired))
    );
}

#[test]
fn test_share_earnings_by_non_mentee_fails() {
    let (env, client, _admin) = setup();
    let (mentor, _mentee, id) = active_program(&env, &client);
    let token_id = create_token(&env, &mentor, 10_000);
    assert_eq!(
        client.try_share_earnings(&mentor, &id, &token_id, &1_000),
        Err(Ok(Error::Unauthorized))
    );
}

// ---------------------------------------------------------------------------
// Completion, cancellation, rating
// ---------------------------------------------------------------------------

#[test]
fn test_mentee_completes_program() {
    let (env, client, _admin) = setup();
    let (mentor, mentee, id) = active_program(&env, &client);
    client.complete_program(&mentee, &id);
    assert!(has_event(&env, symbol_short!("completed")));

    assert_eq!(client.get_program(&id).unwrap().status, ProgramStatus::Completed);
    let profile = client.get_mentor(&mentor).unwrap();
    assert_eq!(profile.active_mentees, 0);
    assert_eq!(profile.completed_programs, 1);
}

#[test]
fn test_mentor_cannot_complete_before_term() {
    let (env, client, _admin) = setup();
    let (mentor, _mentee, id) = active_program(&env, &client);
    assert_eq!(
        client.try_complete_program(&mentor, &id),
        Err(Ok(Error::ProgramNotMatured))
    );
}

#[test]
fn test_mentor_completes_after_term() {
    let (env, client, _admin) = setup();
    let (mentor, _mentee, id) = active_program(&env, &client);
    let end = client.get_program(&id).unwrap().end_ledger;
    env.ledger().with_mut(|l| l.sequence_number = end);
    client.complete_program(&mentor, &id);
    assert_eq!(client.get_program(&id).unwrap().status, ProgramStatus::Completed);
}

#[test]
fn test_cancel_active_program_frees_slot() {
    let (env, client, _admin) = setup();
    let (mentor, mentee, id) = active_program(&env, &client);
    client.cancel_program(&mentee, &id);
    assert!(has_event(&env, symbol_short!("cancelled")));
    assert_eq!(client.get_program(&id).unwrap().status, ProgramStatus::Cancelled);
    assert_eq!(client.get_mentor(&mentor).unwrap().active_mentees, 0);
}

#[test]
fn test_cancel_by_outsider_fails() {
    let (env, client, _admin) = setup();
    let (_mentor, _mentee, id) = active_program(&env, &client);
    let rando = Address::generate(&env);
    assert_eq!(client.try_cancel_program(&rando, &id), Err(Ok(Error::Unauthorized)));
}

#[test]
fn test_cancel_completed_program_fails() {
    let (env, client, _admin) = setup();
    let (_mentor, mentee, id) = active_program(&env, &client);
    client.complete_program(&mentee, &id);
    assert_eq!(
        client.try_cancel_program(&mentee, &id),
        Err(Ok(Error::InvalidProgramState))
    );
}

#[test]
fn test_rate_mentor_updates_average() {
    let (env, client, _admin) = setup();
    let (mentor, mentee, id) = active_program(&env, &client);
    client.complete_program(&mentee, &id);
    client.rate_mentor(&mentee, &id, &4);
    assert!(has_event(&env, symbol_short!("rated")));

    let mentee2 = Address::generate(&env);
    let id2 = client.request_mentorship(&mentee2, &mentor, &MIN_DURATION_LEDGERS);
    client.accept_mentorship(&mentor, &id2);
    client.complete_program(&mentee2, &id2);
    client.rate_mentor(&mentee2, &id2, &5);

    assert_eq!(client.get_mentor_rating(&mentor), 450);
}

#[test]
fn test_rate_twice_fails() {
    let (env, client, _admin) = setup();
    let (_mentor, mentee, id) = active_program(&env, &client);
    client.complete_program(&mentee, &id);
    client.rate_mentor(&mentee, &id, &5);
    assert_eq!(client.try_rate_mentor(&mentee, &id, &5), Err(Ok(Error::AlreadyRated)));
}

#[test]
fn test_rate_before_completion_fails() {
    let (env, client, _admin) = setup();
    let (_mentor, mentee, id) = active_program(&env, &client);
    assert_eq!(
        client.try_rate_mentor(&mentee, &id, &5),
        Err(Ok(Error::InvalidProgramState))
    );
}

#[test]
fn test_rate_out_of_range_fails() {
    let (env, client, _admin) = setup();
    let (_mentor, mentee, id) = active_program(&env, &client);
    client.complete_program(&mentee, &id);
    assert_eq!(client.try_rate_mentor(&mentee, &id, &6), Err(Ok(Error::InvalidRating)));
    assert_eq!(client.try_rate_mentor(&mentee, &id, &0), Err(Ok(Error::InvalidRating)));
}

#[test]
fn test_unknown_program_returns_none() {
    let (_env, client, _admin) = setup();
    assert!(client.get_program(&42).is_none());
}
