//! Issue #777: DAO treasury governance. Token holders vote on allocations of
//! the contract's token balance. Owns its own key enum (no shared `StorageKey`
//! slots consumed).
//!
//! Vote weight is the voter's token balance when the vote is cast: Soroban
//! tokens expose no historical balances, so `snapshot_ledger` is recorded on
//! the proposal for indexers but weight is read live.

use crate::error::ContractError;
use soroban_sdk::{contracttype, symbol_short, token, Address, Bytes, Env, Vec};

#[contracttype]
#[derive(Clone)]
pub enum GovKey {
    /// Instance: voting period in ledgers.
    VotingPeriod,
    /// Instance: quorum in bps of `TotalSupply`.
    QuorumBps,
    /// Instance: token supply used as quorum denominator.
    TotalSupply,
    /// Instance: proposal counter.
    Counter,
    Proposal(u64),
    Voted(u64, Address),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Allocation {
    pub recipient: Address,
    pub amount: i128,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProposalStatus {
    Active,
    Executed,
    Rejected,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Proposal {
    pub id: u64,
    pub proposer: Address,
    pub description: Bytes,
    pub allocations: Vec<Allocation>,
    pub snapshot_ledger: u32,
    pub end_ledger: u32,
    pub approve_votes: i128,
    pub reject_votes: i128,
    pub status: ProposalStatus,
}

pub fn set_config(env: &Env, voting_period: u32, quorum_bps: u32, total_supply: i128) {
    if quorum_bps > 10_000 || total_supply <= 0 {
        env.panic_with_error(ContractError::InvalidAllocation);
    }
    let s = env.storage().instance();
    s.set(&GovKey::VotingPeriod, &voting_period);
    s.set(&GovKey::QuorumBps, &quorum_bps);
    s.set(&GovKey::TotalSupply, &total_supply);
}

fn cfg<T: soroban_sdk::TryFromVal<Env, soroban_sdk::Val>>(env: &Env, k: GovKey) -> T {
    match env.storage().instance().get(&k) {
        Some(v) => v,
        None => env.panic_with_error(ContractError::GovNotConfigured),
    }
}

fn load(env: &Env, id: u64) -> Proposal {
    match env.storage().persistent().get(&GovKey::Proposal(id)) {
        Some(p) => p,
        None => env.panic_with_error(ContractError::ProposalNotFound),
    }
}

pub fn get_proposal(env: &Env, id: u64) -> Proposal {
    load(env, id)
}

pub fn create_proposal(
    env: &Env,
    proposer: Address,
    description: Bytes,
    allocations: Vec<Allocation>,
) -> u64 {
    proposer.require_auth();
    let period: u32 = cfg(env, GovKey::VotingPeriod);
    if allocations.is_empty() || allocations.iter().any(|a| a.amount <= 0) {
        env.panic_with_error(ContractError::InvalidAllocation);
    }
    let id: u64 = env.storage().instance().get(&GovKey::Counter).unwrap_or(0u64) + 1;
    env.storage().instance().set(&GovKey::Counter, &id);
    let now = env.ledger().sequence();
    let p = Proposal {
        id,
        proposer: proposer.clone(),
        description,
        allocations,
        snapshot_ledger: now,
        end_ledger: now + period,
        approve_votes: 0,
        reject_votes: 0,
        status: ProposalStatus::Active,
    };
    env.storage().persistent().set(&GovKey::Proposal(id), &p);
    env.events()
        .publish((symbol_short!("split"), symbol_short!("prop_new"), id), (proposer, p.end_ledger));
    id
}

pub fn vote(env: &Env, token_addr: &Address, id: u64, voter: Address, approve: bool) {
    voter.require_auth();
    let mut p = load(env, id);
    if p.status != ProposalStatus::Active {
        env.panic_with_error(ContractError::ProposalFinalized);
    }
    if env.ledger().sequence() > p.end_ledger {
        env.panic_with_error(ContractError::VotingClosed);
    }
    let vk = GovKey::Voted(id, voter.clone());
    if env.storage().persistent().has(&vk) {
        env.panic_with_error(ContractError::AlreadyVoted);
    }
    let weight = token::Client::new(env, token_addr).balance(&voter);
    if weight <= 0 {
        env.panic_with_error(ContractError::NoVotingPower);
    }
    if approve {
        p.approve_votes += weight;
    } else {
        p.reject_votes += weight;
    }
    env.storage().persistent().set(&vk, &true);
    env.storage().persistent().set(&GovKey::Proposal(id), &p);
    env.events()
        .publish((symbol_short!("split"), symbol_short!("vote"), id), (voter, approve, weight));
}

pub fn execute(env: &Env, token_addr: &Address, id: u64) {
    let mut p = load(env, id);
    if p.status != ProposalStatus::Active {
        env.panic_with_error(ContractError::ProposalFinalized);
    }
    if env.ledger().sequence() <= p.end_ledger {
        env.panic_with_error(ContractError::VotingOpen);
    }
    let quorum_bps: u32 = cfg(env, GovKey::QuorumBps);
    let supply: i128 = cfg(env, GovKey::TotalSupply);
    let turnout = p.approve_votes + p.reject_votes;
    let quorum_met = turnout * 10_000 >= supply * quorum_bps as i128;
    if quorum_met && p.approve_votes > p.reject_votes {
        let client = token::Client::new(env, token_addr);
        let me = env.current_contract_address();
        for a in p.allocations.iter() {
            client.transfer(&me, &a.recipient, &a.amount);
        }
        p.status = ProposalStatus::Executed;
        env.events()
            .publish((symbol_short!("split"), symbol_short!("prop_exe"), id), p.approve_votes);
    } else {
        p.status = ProposalStatus::Rejected;
        env.events()
            .publish((symbol_short!("split"), symbol_short!("prop_rej"), id), quorum_met);
    }
    env.storage().persistent().set(&GovKey::Proposal(id), &p);
}
