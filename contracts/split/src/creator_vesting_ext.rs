//! Issue #878: Creator vesting contracts.
//!
//! A creator can attach a linear vesting schedule to an invoice. Vesting is
//! time-based: the total amount linearly vests between `start_at` and `end_at`
//! with an optional cliff at `cliff_at`. No tokens are claimable before
//! `cliff_at`; after the cliff the full linearly-vested portion is claimable.
//!
//! The creator calls `claim_vested` to receive tokens that have vested since
//! the last claim. The contract holds no extra tokens — this tracks how much
//! of an already-released invoice amount the creator has claimed.
//!
//! Storage: own persistent key `VestingKey`.

use crate::error::ContractError;
use crate::types::VestingSchedule;
use crate::{events, load_invoice, require_not_paused, SplitContract, SplitContractArgs, SplitContractClient};
use soroban_sdk::{contractimpl, contracttype, panic_with_error, Address, Env, Symbol};

// ---------------------------------------------------------------------------
// Storage key
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VestingKey {
    /// Per-invoice vesting schedule.
    Schedule(u64),
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn load_schedule(env: &Env, invoice_id: u64) -> VestingSchedule {
    env.storage()
        .persistent()
        .get(&VestingKey::Schedule(invoice_id))
        .unwrap_or_else(|| panic_with_error!(env, ContractError::VestingNotFound))
}

/// Compute how much of `total_amount` has vested by `now`.
///
/// Returns 0 before `start_at`, partial amount during vesting, and
/// `total_amount` after `end_at`. The cliff blocks any claims before
/// `cliff_at` even if vesting has technically started.
fn compute_vested(schedule: &VestingSchedule, now: u64) -> i128 {
    if now < schedule.cliff_at {
        return 0;
    }
    if now >= schedule.end_at {
        return schedule.total_amount;
    }
    if now < schedule.start_at {
        return 0;
    }
    let elapsed = (now - schedule.start_at) as i128;
    let duration = (schedule.end_at - schedule.start_at) as i128;
    if duration == 0 {
        return schedule.total_amount;
    }
    schedule.total_amount * elapsed / duration
}

// ---------------------------------------------------------------------------
// Contract entry points
// ---------------------------------------------------------------------------

#[contractimpl]
impl SplitContract {
    /// Attach a vesting schedule to `invoice_id` (creator only).
    ///
    /// - `total_amount`: the total token amount subject to vesting.
    /// - `start_at`: Unix timestamp when linear vesting begins.
    /// - `cliff_at`: Unix timestamp before which nothing is claimable
    ///   (must be ≥ `start_at`).
    /// - `end_at`: Unix timestamp when the full amount is vested (must be
    ///   > `start_at`).
    ///
    /// The invoice must exist. Overwrites any existing schedule.
    pub fn create_vesting_schedule(
        env: Env,
        creator: Address,
        invoice_id: u64,
        total_amount: i128,
        start_at: u64,
        cliff_at: u64,
        end_at: u64,
    ) {
        require_not_paused(&env);
        creator.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        if invoice.creator != creator {
            panic_with_error!(&env, ContractError::NotAuthorized);
        }
        if total_amount <= 0 {
            panic_with_error!(&env, ContractError::InvalidAmount);
        }
        if cliff_at < start_at || end_at <= start_at {
            panic_with_error!(&env, ContractError::InvalidAmount);
        }
        let schedule = VestingSchedule {
            invoice_id,
            creator: creator.clone(),
            total_amount,
            released_amount: 0,
            start_at,
            cliff_at,
            end_at,
        };
        env.storage()
            .persistent()
            .set(&VestingKey::Schedule(invoice_id), &schedule);
        events::vesting_schedule_created(&env, invoice_id, &creator, total_amount, start_at, cliff_at, end_at);
    }

    /// Claim the currently vested but unclaimed tokens (creator only).
    ///
    /// Panics with `VestingNotFound` if no schedule exists, and
    /// `VestingCliffNotReached` if called before the cliff, and
    /// `VestingAlreadyComplete` if everything has already been claimed.
    pub fn claim_vested(env: Env, creator: Address, invoice_id: u64) -> i128 {
        require_not_paused(&env);
        creator.require_auth();
        let mut schedule = load_schedule(&env, invoice_id);
        if schedule.creator != creator {
            panic_with_error!(&env, ContractError::NotAuthorized);
        }
        let now = env.ledger().timestamp();
        if now < schedule.cliff_at {
            panic_with_error!(&env, ContractError::VestingCliffNotReached);
        }
        if schedule.released_amount >= schedule.total_amount {
            panic_with_error!(&env, ContractError::VestingAlreadyComplete);
        }
        let vested = compute_vested(&schedule, now);
        let claimable = vested - schedule.released_amount;
        if claimable <= 0 {
            panic_with_error!(&env, ContractError::VestingCliffNotReached);
        }
        schedule.released_amount += claimable;
        env.storage()
            .persistent()
            .set(&VestingKey::Schedule(invoice_id), &schedule);
        events::vesting_claimed(&env, invoice_id, &creator, claimable, schedule.released_amount);
        claimable
    }

    /// Return the vesting schedule for `invoice_id`, or `None` if none exists.
    pub fn get_vesting_schedule(env: Env, invoice_id: u64) -> Option<VestingSchedule> {
        env.storage()
            .persistent()
            .get(&VestingKey::Schedule(invoice_id))
    }

    /// Return how much of the vesting schedule has vested by the current
    /// ledger timestamp (regardless of what has already been claimed).
    pub fn get_vested_amount(env: Env, invoice_id: u64) -> i128 {
        let schedule: VestingSchedule = env
            .storage()
            .persistent()
            .get(&VestingKey::Schedule(invoice_id))
            .unwrap_or_else(|| panic_with_error!(&env, ContractError::VestingNotFound));
        compute_vested(&schedule, env.ledger().timestamp())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};
    use soroban_sdk::{vec, Env};

    fn setup() -> (Env, SplitContractClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, SplitContract);
        let client = SplitContractClient::new(&env, &contract_id);
        client.initialize(
            &Address::generate(&env),
            &0_i128,
            &0_u32,
            &Address::generate(&env),
        );
        (env, client)
    }

    #[test]
    fn test_create_and_get_vesting_schedule() {
        let (env, client) = setup();
        let creator = Address::generate(&env);
        let recipient = Address::generate(&env);
        let token = Address::generate(&env);
        let invoice_id = client.create_invoice(
            &creator,
            &vec![&env, recipient.clone()],
            &vec![&env, 1000_i128],
            &token,
            &(env.ledger().timestamp() + 10_000),
        );
        client.create_vesting_schedule(
            &creator,
            &invoice_id,
            &1000_i128,
            &100_u64,
            &200_u64,
            &1100_u64,
        );
        let schedule = client.get_vesting_schedule(&invoice_id).unwrap();
        assert_eq!(schedule.total_amount, 1000_i128);
        assert_eq!(schedule.released_amount, 0_i128);
        assert_eq!(schedule.cliff_at, 200_u64);
    }

    #[test]
    fn test_get_vesting_schedule_none() {
        let (env, client) = setup();
        let creator = Address::generate(&env);
        let recipient = Address::generate(&env);
        let token = Address::generate(&env);
        let invoice_id = client.create_invoice(
            &creator,
            &vec![&env, recipient],
            &vec![&env, 500_i128],
            &token,
            &(env.ledger().timestamp() + 10_000),
        );
        assert!(client.get_vesting_schedule(&invoice_id).is_none());
    }

    #[test]
    #[should_panic]
    fn test_only_creator_can_create_schedule() {
        let (env, client) = setup();
        let creator = Address::generate(&env);
        let other = Address::generate(&env);
        let recipient = Address::generate(&env);
        let token = Address::generate(&env);
        let invoice_id = client.create_invoice(
            &creator,
            &vec![&env, recipient],
            &vec![&env, 500_i128],
            &token,
            &(env.ledger().timestamp() + 10_000),
        );
        client.create_vesting_schedule(&other, &invoice_id, &500_i128, &100_u64, &100_u64, &1100_u64);
    }

    #[test]
    #[should_panic]
    fn test_claim_before_cliff_panics() {
        let (env, client) = setup();
        let creator = Address::generate(&env);
        let recipient = Address::generate(&env);
        let token = Address::generate(&env);
        // Set ledger time well before cliff
        env.ledger().set(LedgerInfo {
            timestamp: 50,
            ..env.ledger().get()
        });
        let invoice_id = client.create_invoice(
            &creator,
            &vec![&env, recipient],
            &vec![&env, 1000_i128],
            &token,
            &(env.ledger().timestamp() + 10_000),
        );
        client.create_vesting_schedule(
            &creator,
            &invoice_id,
            &1000_i128,
            &100_u64,
            &500_u64,
            &2000_u64,
        );
        // Still before cliff
        client.claim_vested(&creator, &invoice_id);
    }

    #[test]
    #[should_panic]
    fn test_get_vested_amount_no_schedule_panics() {
        let (env, client) = setup();
        let creator = Address::generate(&env);
        let recipient = Address::generate(&env);
        let token = Address::generate(&env);
        let invoice_id = client.create_invoice(
            &creator,
            &vec![&env, recipient],
            &vec![&env, 500_i128],
            &token,
            &(env.ledger().timestamp() + 10_000),
        );
        client.get_vested_amount(&invoice_id);
    }

    #[test]
    #[should_panic]
    fn test_invalid_cliff_before_start_panics() {
        let (env, client) = setup();
        let creator = Address::generate(&env);
        let recipient = Address::generate(&env);
        let token = Address::generate(&env);
        let invoice_id = client.create_invoice(
            &creator,
            &vec![&env, recipient],
            &vec![&env, 500_i128],
            &token,
            &(env.ledger().timestamp() + 10_000),
        );
        // cliff_at < start_at — invalid
        client.create_vesting_schedule(
            &creator,
            &invoice_id,
            &500_i128,
            &500_u64,
            &100_u64, // cliff before start
            &2000_u64,
        );
    }
}
