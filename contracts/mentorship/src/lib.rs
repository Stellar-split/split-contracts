//! StellarSplit Mentorship — on-chain creator mentorship program.
//!
//! Experienced creators register as mentors with an area of expertise, a
//! mentee capacity and a revenue share. Newer creators request mentorship;
//! once a mentor accepts, the program runs for a fixed number of ledgers.
//! During the term the mentee can route a share of their earnings to the
//! mentor, the mentor logs sessions, and on completion the mentee can rate
//! the mentor. Every state transition emits an event under the `mentor`
//! topic so indexers can follow the program lifecycle.

#![no_std]

mod errors;
mod types;

pub use errors::Error;
pub use types::{MentorProfile, MentorshipProgram, ProgramStatus};

use soroban_sdk::{contract, contractimpl, symbol_short, token, Address, Env, Symbol, Vec};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const BPS_DENOMINATOR: i128 = 10_000;
/// Mentors may take at most 30% of a mentee's earnings.
pub const MAX_REVENUE_SHARE_BPS: u32 = 3_000;
pub const MAX_MENTEES_PER_MENTOR: u32 = 50;
/// Roughly one day at 5s ledgers.
pub const MIN_DURATION_LEDGERS: u32 = 17_280;
/// Roughly one year at 5s ledgers.
pub const MAX_DURATION_LEDGERS: u32 = 6_307_200;
const MAX_RATING: u32 = 5;

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

fn admin_key() -> Symbol {
    symbol_short!("admin")
}

fn counter_key() -> Symbol {
    symbol_short!("prog_ctr")
}

fn mentor_key(mentor: &Address) -> (Symbol, Address) {
    (symbol_short!("mentor"), mentor.clone())
}

fn program_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("program"), id)
}

fn mentor_programs_key(mentor: &Address) -> (Symbol, Address) {
    (symbol_short!("mtr_prgs"), mentor.clone())
}

fn mentee_programs_key(mentee: &Address) -> (Symbol, Address) {
    (symbol_short!("mte_prgs"), mentee.clone())
}

/// Tracks the open (requested/active) program for a mentor/mentee pair.
fn open_pair_key(mentor: &Address, mentee: &Address) -> (Symbol, Address, Address) {
    (symbol_short!("open_pair"), mentor.clone(), mentee.clone())
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

fn emit_mentor_registered(env: &Env, mentor: &Address, expertise: &Symbol, share_bps: u32, max_mentees: u32) {
    env.events().publish(
        (symbol_short!("mentor"), symbol_short!("reg"), mentor.clone()),
        (expertise.clone(), share_bps, max_mentees),
    );
}

fn emit_mentor_updated(env: &Env, mentor: &Address, share_bps: u32, max_mentees: u32, available: bool) {
    env.events().publish(
        (symbol_short!("mentor"), symbol_short!("updated"), mentor.clone()),
        (share_bps, max_mentees, available),
    );
}

fn emit_mentor_suspended(env: &Env, mentor: &Address, suspended: bool) {
    env.events().publish(
        (symbol_short!("mentor"), symbol_short!("suspend"), mentor.clone()),
        suspended,
    );
}

fn emit_program_status(env: &Env, action: Symbol, program: &MentorshipProgram) {
    env.events().publish(
        (symbol_short!("mentor"), action, program.id),
        (program.mentor.clone(), program.mentee.clone(), program.status),
    );
}

fn emit_session_logged(env: &Env, id: u64, sessions: u32) {
    env.events().publish(
        (symbol_short!("mentor"), symbol_short!("session"), id),
        sessions,
    );
}

fn emit_earnings_shared(env: &Env, id: u64, token: &Address, gross: i128, share: i128) {
    env.events().publish(
        (symbol_short!("mentor"), symbol_short!("shared"), id),
        (token.clone(), gross, share),
    );
}

fn emit_mentor_rated(env: &Env, id: u64, mentor: &Address, rating: u32) {
    env.events().publish(
        (symbol_short!("mentor"), symbol_short!("rated"), id),
        (mentor.clone(), rating),
    );
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn require_admin(env: &Env, admin: &Address) -> Result<(), Error> {
    let stored: Address = env
        .storage()
        .instance()
        .get(&admin_key())
        .ok_or(Error::NotInitialized)?;
    if stored != *admin {
        return Err(Error::NotAdmin);
    }
    admin.require_auth();
    Ok(())
}

fn load_mentor(env: &Env, mentor: &Address) -> Result<MentorProfile, Error> {
    env.storage()
        .persistent()
        .get(&mentor_key(mentor))
        .ok_or(Error::MentorNotFound)
}

fn save_mentor(env: &Env, profile: &MentorProfile) {
    env.storage()
        .persistent()
        .set(&mentor_key(&profile.mentor), profile);
}

fn load_program(env: &Env, id: u64) -> Result<MentorshipProgram, Error> {
    env.storage()
        .persistent()
        .get(&program_key(id))
        .ok_or(Error::ProgramNotFound)
}

fn save_program(env: &Env, program: &MentorshipProgram) {
    env.storage()
        .persistent()
        .set(&program_key(program.id), program);
}

fn push_id(env: &Env, key: &(Symbol, Address), id: u64) {
    let mut ids: Vec<u64> = env
        .storage()
        .persistent()
        .get(key)
        .unwrap_or_else(|| Vec::new(env));
    ids.push_back(id);
    env.storage().persistent().set(key, &ids);
}

fn validate_terms(share_bps: u32, max_mentees: u32) -> Result<(), Error> {
    if share_bps > MAX_REVENUE_SHARE_BPS {
        return Err(Error::InvalidRevenueShare);
    }
    if max_mentees == 0 || max_mentees > MAX_MENTEES_PER_MENTOR {
        return Err(Error::InvalidCapacity);
    }
    Ok(())
}

/// Closes an active program: frees the mentor slot and clears the pair lock.
fn release_slot(env: &Env, program: &MentorshipProgram, was_active: bool) -> Result<(), Error> {
    if was_active {
        let mut profile = load_mentor(env, &program.mentor)?;
        profile.active_mentees = profile.active_mentees.saturating_sub(1);
        save_mentor(env, &profile);
    }
    env.storage()
        .persistent()
        .remove(&open_pair_key(&program.mentor, &program.mentee));
    Ok(())
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct MentorshipContract;

#[contractimpl]
impl MentorshipContract {
    /// Initialise the contract with an admin who can suspend mentors.
    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&admin_key()) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&admin_key(), &admin);
        env.storage().instance().set(&counter_key(), &0u64);
        Ok(())
    }

    /// Register the caller as a mentor.
    ///
    /// * `revenue_share_bps` — share of mentee earnings (≤ `MAX_REVENUE_SHARE_BPS`).
    /// * `max_mentees` — concurrent mentee capacity (1..=`MAX_MENTEES_PER_MENTOR`).
    pub fn register_mentor(
        env: Env,
        mentor: Address,
        expertise: Symbol,
        revenue_share_bps: u32,
        max_mentees: u32,
    ) -> Result<(), Error> {
        mentor.require_auth();
        if !env.storage().instance().has(&admin_key()) {
            return Err(Error::NotInitialized);
        }
        if env.storage().persistent().has(&mentor_key(&mentor)) {
            return Err(Error::MentorAlreadyRegistered);
        }
        validate_terms(revenue_share_bps, max_mentees)?;

        let profile = MentorProfile {
            mentor: mentor.clone(),
            expertise: expertise.clone(),
            revenue_share_bps,
            max_mentees,
            active_mentees: 0,
            completed_programs: 0,
            rating_sum: 0,
            rating_count: 0,
            available: true,
            suspended: false,
            registered_at: env.ledger().timestamp(),
        };
        save_mentor(&env, &profile);
        emit_mentor_registered(&env, &mentor, &expertise, revenue_share_bps, max_mentees);
        Ok(())
    }

    /// Update a mentor's terms and availability. Terms of programs that are
    /// already active are not affected.
    pub fn update_mentor(
        env: Env,
        mentor: Address,
        revenue_share_bps: u32,
        max_mentees: u32,
        available: bool,
    ) -> Result<(), Error> {
        mentor.require_auth();
        let mut profile = load_mentor(&env, &mentor)?;
        validate_terms(revenue_share_bps, max_mentees)?;
        if max_mentees < profile.active_mentees {
            return Err(Error::InvalidCapacity);
        }
        profile.revenue_share_bps = revenue_share_bps;
        profile.max_mentees = max_mentees;
        profile.available = available;
        save_mentor(&env, &profile);
        emit_mentor_updated(&env, &mentor, revenue_share_bps, max_mentees, available);
        Ok(())
    }

    /// Admin-only: suspend or reinstate a mentor. Suspended mentors cannot
    /// accept new requests; existing programs continue.
    pub fn set_mentor_suspended(
        env: Env,
        admin: Address,
        mentor: Address,
        suspended: bool,
    ) -> Result<(), Error> {
        require_admin(&env, &admin)?;
        let mut profile = load_mentor(&env, &mentor)?;
        profile.suspended = suspended;
        save_mentor(&env, &profile);
        emit_mentor_suspended(&env, &mentor, suspended);
        Ok(())
    }

    /// Mentee requests a mentorship program of `duration_ledgers` with `mentor`.
    /// Returns the new program ID.
    pub fn request_mentorship(
        env: Env,
        mentee: Address,
        mentor: Address,
        duration_ledgers: u32,
    ) -> Result<u64, Error> {
        mentee.require_auth();
        if mentee == mentor {
            return Err(Error::SelfMentorship);
        }
        if !(MIN_DURATION_LEDGERS..=MAX_DURATION_LEDGERS).contains(&duration_ledgers) {
            return Err(Error::InvalidDuration);
        }
        let profile = load_mentor(&env, &mentor)?;
        if !profile.available || profile.suspended {
            return Err(Error::MentorUnavailable);
        }
        let pair_key = open_pair_key(&mentor, &mentee);
        if env.storage().persistent().has(&pair_key) {
            return Err(Error::DuplicateProgram);
        }

        let id: u64 = env
            .storage()
            .instance()
            .get(&counter_key())
            .ok_or(Error::NotInitialized)?;
        let next = id.checked_add(1).ok_or(Error::Overflow)?;
        env.storage().instance().set(&counter_key(), &next);

        let program = MentorshipProgram {
            id,
            mentor: mentor.clone(),
            mentee: mentee.clone(),
            status: ProgramStatus::Requested,
            revenue_share_bps: profile.revenue_share_bps,
            duration_ledgers,
            requested_at: env.ledger().timestamp(),
            start_ledger: 0,
            end_ledger: 0,
            sessions: 0,
            total_shared: 0,
            rated: false,
        };
        save_program(&env, &program);
        env.storage().persistent().set(&pair_key, &id);
        push_id(&env, &mentor_programs_key(&mentor), id);
        push_id(&env, &mentee_programs_key(&mentee), id);

        emit_program_status(&env, symbol_short!("requested"), &program);
        Ok(id)
    }

    /// Mentor accepts a pending request. Locks in the mentor's current
    /// revenue share and starts the program term.
    pub fn accept_mentorship(env: Env, mentor: Address, program_id: u64) -> Result<(), Error> {
        mentor.require_auth();
        let mut program = load_program(&env, program_id)?;
        if program.mentor != mentor {
            return Err(Error::Unauthorized);
        }
        if program.status != ProgramStatus::Requested {
            return Err(Error::InvalidProgramState);
        }
        let mut profile = load_mentor(&env, &mentor)?;
        if profile.suspended {
            return Err(Error::MentorUnavailable);
        }
        if profile.active_mentees >= profile.max_mentees {
            return Err(Error::MentorAtCapacity);
        }

        let start = env.ledger().sequence();
        program.status = ProgramStatus::Active;
        program.revenue_share_bps = profile.revenue_share_bps;
        program.start_ledger = start;
        program.end_ledger = start
            .checked_add(program.duration_ledgers)
            .ok_or(Error::Overflow)?;
        profile.active_mentees += 1;

        save_mentor(&env, &profile);
        save_program(&env, &program);
        emit_program_status(&env, symbol_short!("accepted"), &program);
        Ok(())
    }

    /// Mentor declines a pending request.
    pub fn decline_mentorship(env: Env, mentor: Address, program_id: u64) -> Result<(), Error> {
        mentor.require_auth();
        let mut program = load_program(&env, program_id)?;
        if program.mentor != mentor {
            return Err(Error::Unauthorized);
        }
        if program.status != ProgramStatus::Requested {
            return Err(Error::InvalidProgramState);
        }
        program.status = ProgramStatus::Declined;
        release_slot(&env, &program, false)?;
        save_program(&env, &program);
        emit_program_status(&env, symbol_short!("declined"), &program);
        Ok(())
    }

    /// Mentor records a completed mentoring session on an active program.
    pub fn log_session(env: Env, mentor: Address, program_id: u64) -> Result<u32, Error> {
        mentor.require_auth();
        let mut program = load_program(&env, program_id)?;
        if program.mentor != mentor {
            return Err(Error::Unauthorized);
        }
        if program.status != ProgramStatus::Active {
            return Err(Error::InvalidProgramState);
        }
        program.sessions = program.sessions.checked_add(1).ok_or(Error::Overflow)?;
        save_program(&env, &program);
        emit_session_logged(&env, program_id, program.sessions);
        Ok(program.sessions)
    }

    /// Mentee routes the mentor's revenue share of `gross_amount` earnings.
    ///
    /// Transfers `gross_amount * revenue_share_bps / 10_000` of `token` from
    /// the mentee to the mentor and returns the amount transferred.
    pub fn share_earnings(
        env: Env,
        mentee: Address,
        program_id: u64,
        token: Address,
        gross_amount: i128,
    ) -> Result<i128, Error> {
        mentee.require_auth();
        if gross_amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        let mut program = load_program(&env, program_id)?;
        if program.mentee != mentee {
            return Err(Error::Unauthorized);
        }
        if program.status != ProgramStatus::Active {
            return Err(Error::InvalidProgramState);
        }
        if env.ledger().sequence() > program.end_ledger {
            return Err(Error::ProgramExpired);
        }

        let share = gross_amount
            .checked_mul(program.revenue_share_bps as i128)
            .ok_or(Error::Overflow)?
            / BPS_DENOMINATOR;
        if share > 0 {
            token::Client::new(&env, &token).transfer(&mentee, &program.mentor, &share);
        }
        program.total_shared = program
            .total_shared
            .checked_add(share)
            .ok_or(Error::Overflow)?;
        save_program(&env, &program);
        emit_earnings_shared(&env, program_id, &token, gross_amount, share);
        Ok(share)
    }

    /// Complete an active program.
    ///
    /// The mentee may complete at any time; the mentor only once the term
    /// has ended.
    pub fn complete_program(env: Env, caller: Address, program_id: u64) -> Result<(), Error> {
        caller.require_auth();
        let mut program = load_program(&env, program_id)?;
        if program.status != ProgramStatus::Active {
            return Err(Error::InvalidProgramState);
        }
        if caller == program.mentor {
            if env.ledger().sequence() < program.end_ledger {
                return Err(Error::ProgramNotMatured);
            }
        } else if caller != program.mentee {
            return Err(Error::Unauthorized);
        }

        program.status = ProgramStatus::Completed;
        release_slot(&env, &program, true)?;
        let mut profile = load_mentor(&env, &program.mentor)?;
        profile.completed_programs = profile.completed_programs.saturating_add(1);
        save_mentor(&env, &profile);
        save_program(&env, &program);
        emit_program_status(&env, symbol_short!("completed"), &program);
        Ok(())
    }

    /// Cancel a requested or active program. Either participant may cancel.
    pub fn cancel_program(env: Env, caller: Address, program_id: u64) -> Result<(), Error> {
        caller.require_auth();
        let mut program = load_program(&env, program_id)?;
        if caller != program.mentor && caller != program.mentee {
            return Err(Error::Unauthorized);
        }
        let was_active = match program.status {
            ProgramStatus::Requested => false,
            ProgramStatus::Active => true,
            _ => return Err(Error::InvalidProgramState),
        };
        program.status = ProgramStatus::Cancelled;
        release_slot(&env, &program, was_active)?;
        save_program(&env, &program);
        emit_program_status(&env, symbol_short!("cancelled"), &program);
        Ok(())
    }

    /// Mentee rates the mentor (1..=5) once a program has completed.
    pub fn rate_mentor(env: Env, mentee: Address, program_id: u64, rating: u32) -> Result<(), Error> {
        mentee.require_auth();
        if rating == 0 || rating > MAX_RATING {
            return Err(Error::InvalidRating);
        }
        let mut program = load_program(&env, program_id)?;
        if program.mentee != mentee {
            return Err(Error::Unauthorized);
        }
        if program.status != ProgramStatus::Completed {
            return Err(Error::InvalidProgramState);
        }
        if program.rated {
            return Err(Error::AlreadyRated);
        }
        let mut profile = load_mentor(&env, &program.mentor)?;
        profile.rating_sum = profile.rating_sum.checked_add(rating).ok_or(Error::Overflow)?;
        profile.rating_count = profile.rating_count.checked_add(1).ok_or(Error::Overflow)?;
        program.rated = true;
        save_mentor(&env, &profile);
        save_program(&env, &program);
        emit_mentor_rated(&env, program_id, &program.mentor, rating);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Views
    // -----------------------------------------------------------------------

    pub fn get_mentor(env: Env, mentor: Address) -> Option<MentorProfile> {
        env.storage().persistent().get(&mentor_key(&mentor))
    }

    pub fn get_program(env: Env, program_id: u64) -> Option<MentorshipProgram> {
        env.storage().persistent().get(&program_key(program_id))
    }

    pub fn get_mentor_programs(env: Env, mentor: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&mentor_programs_key(&mentor))
            .unwrap_or_else(|| Vec::new(&env))
    }

    pub fn get_mentee_programs(env: Env, mentee: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&mentee_programs_key(&mentee))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Average rating scaled by 100 (e.g. 450 == 4.50). Returns 0 when unrated.
    pub fn get_mentor_rating(env: Env, mentor: Address) -> u32 {
        match env.storage().persistent().get::<_, MentorProfile>(&mentor_key(&mentor)) {
            Some(p) if p.rating_count > 0 => p.rating_sum * 100 / p.rating_count,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod test;
