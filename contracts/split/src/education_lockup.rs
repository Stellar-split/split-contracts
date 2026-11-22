//! Issue #891: Recipient education funding lockup.
//!
//! A funder locks tokens for a recipient with an approved education
//! institution. Funds cannot be withdrawn by the recipient directly: once the
//! release ledger is reached they can only be paid out to the institution. If
//! the lockup is not released before its expiry ledger, the funder may reclaim.

use crate::SplitContract;
#[allow(unused_imports)]
use crate::{SplitContractArgs, SplitContractClient};
use soroban_sdk::{contractimpl, contracttype, symbol_short, token, Address, Env, Vec};

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum EducationKey {
    NextId,
    Lockup(u64),
    /// Lockup ids per recipient: Vec<u64>
    ByRecipient(Address),
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum LockupStatus {
    Locked,
    Released,
    Reclaimed,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct EducationLockup {
    pub id: u64,
    pub funder: Address,
    pub recipient: Address,
    pub institution: Address,
    pub token: Address,
    pub amount: i128,
    pub release_ledger: u32,
    pub expiry_ledger: u32,
    pub status: LockupStatus,
}

fn load_lockup(env: &Env, id: u64) -> EducationLockup {
    env.storage()
        .persistent()
        .get(&EducationKey::Lockup(id))
        .expect("lockup not found")
}

fn save_lockup(env: &Env, l: &EducationLockup) {
    env.storage()
        .persistent()
        .set(&EducationKey::Lockup(l.id), l);
}

#[contractimpl]
impl SplitContract {
    /// Lock `amount` for `recipient`, payable only to `institution` once
    /// `release_ledger` is reached. Returns the lockup id.
    pub fn create_education_lockup(
        env: Env,
        funder: Address,
        recipient: Address,
        institution: Address,
        token: Address,
        amount: i128,
        release_ledger: u32,
        expiry_ledger: u32,
    ) -> u64 {
        funder.require_auth();
        assert!(amount > 0, "amount must be positive");
        assert!(
            release_ledger > env.ledger().sequence(),
            "release ledger must be in the future"
        );
        assert!(expiry_ledger > release_ledger, "expiry must follow release");
        token::Client::new(&env, &token).transfer(
            &funder,
            &env.current_contract_address(),
            &amount,
        );
        let id: u64 = env
            .storage()
            .persistent()
            .get(&EducationKey::NextId)
            .unwrap_or(1u64);
        env.storage().persistent().set(&EducationKey::NextId, &(id + 1));
        let lockup = EducationLockup {
            id,
            funder: funder.clone(),
            recipient: recipient.clone(),
            institution: institution.clone(),
            token,
            amount,
            release_ledger,
            expiry_ledger,
            status: LockupStatus::Locked,
        };
        save_lockup(&env, &lockup);
        let rkey = EducationKey::ByRecipient(recipient.clone());
        let mut ids: Vec<u64> = env.storage().persistent().get(&rkey).unwrap_or(Vec::new(&env));
        ids.push_back(id);
        env.storage().persistent().set(&rkey, &ids);
        env.events().publish(
            (symbol_short!("split"), symbol_short!("edu_lock"), id),
            (funder, recipient, institution, amount, release_ledger),
        );
        id
    }

    /// Recipient authorises payment of the locked funds to the institution.
    /// Only valid between the release and expiry ledgers.
    pub fn release_education_funds(env: Env, recipient: Address, lockup_id: u64) {
        recipient.require_auth();
        let mut l = load_lockup(&env, lockup_id);
        assert!(l.recipient == recipient, "not the lockup recipient");
        assert!(l.status == LockupStatus::Locked, "lockup not active");
        let now = env.ledger().sequence();
        assert!(now >= l.release_ledger, "lockup not yet releasable");
        assert!(now <= l.expiry_ledger, "lockup expired");
        token::Client::new(&env, &l.token).transfer(
            &env.current_contract_address(),
            &l.institution,
            &l.amount,
        );
        l.status = LockupStatus::Released;
        save_lockup(&env, &l);
        env.events().publish(
            (symbol_short!("split"), symbol_short!("edu_rel"), lockup_id),
            (l.institution, l.amount),
        );
    }

    /// Funder reclaims an unreleased lockup after its expiry ledger.
    pub fn reclaim_education_funds(env: Env, funder: Address, lockup_id: u64) {
        funder.require_auth();
        let mut l = load_lockup(&env, lockup_id);
        assert!(l.funder == funder, "not the lockup funder");
        assert!(l.status == LockupStatus::Locked, "lockup not active");
        assert!(env.ledger().sequence() > l.expiry_ledger, "lockup not yet expired");
        token::Client::new(&env, &l.token).transfer(
            &env.current_contract_address(),
            &funder,
            &l.amount,
        );
        l.status = LockupStatus::Reclaimed;
        save_lockup(&env, &l);
        env.events().publish(
            (symbol_short!("split"), symbol_short!("edu_recl"), lockup_id),
            (funder, l.amount),
        );
    }

    /// Fetch a lockup by id.
    pub fn get_education_lockup(env: Env, lockup_id: u64) -> EducationLockup {
        load_lockup(&env, lockup_id)
    }

    /// Lockup ids created for a recipient.
    pub fn get_recipient_education_lockups(env: Env, recipient: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&EducationKey::ByRecipient(recipient))
            .unwrap_or(Vec::new(&env))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger};
    use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};

    struct Ctx {
        env: Env,
        c: SplitContractClient<'static>,
        token: Address,
        funder: Address,
        recipient: Address,
        school: Address,
        id: u64,
    }

    fn setup() -> Ctx {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register(SplitContract, ());
        let c = SplitContractClient::new(&env, &cid);
        let token = env
            .register_stellar_asset_contract_v2(Address::generate(&env))
            .address();
        let (funder, recipient, school) = (
            Address::generate(&env),
            Address::generate(&env),
            Address::generate(&env),
        );
        StellarAssetClient::new(&env, &token).mint(&funder, &1_000);
        let now = env.ledger().sequence();
        let id = c.create_education_lockup(
            &funder, &recipient, &school, &token, &1_000, &(now + 10), &(now + 100),
        );
        Ctx { env, c, token, funder, recipient, school, id }
    }

    #[test]
    fn test_release_pays_institution() {
        let x = setup();
        x.env.ledger().set_sequence_number(x.env.ledger().sequence() + 10);
        x.c.release_education_funds(&x.recipient, &x.id);
        assert_eq!(TokenClient::new(&x.env, &x.token).balance(&x.school), 1_000);
        assert_eq!(x.c.get_education_lockup(&x.id).status, LockupStatus::Released);
        assert_eq!(x.c.get_recipient_education_lockups(&x.recipient).len(), 1);
    }

    #[test]
    #[should_panic(expected = "lockup not yet releasable")]
    fn test_release_before_ledger_rejected() {
        let x = setup();
        x.c.release_education_funds(&x.recipient, &x.id);
    }

    #[test]
    fn test_reclaim_after_expiry() {
        let x = setup();
        x.env.ledger().set_sequence_number(x.env.ledger().sequence() + 101);
        x.c.reclaim_education_funds(&x.funder, &x.id);
        assert_eq!(TokenClient::new(&x.env, &x.token).balance(&x.funder), 1_000);
    }

    #[test]
    #[should_panic(expected = "lockup not yet expired")]
    fn test_reclaim_before_expiry_rejected() {
        let x = setup();
        x.c.reclaim_education_funds(&x.funder, &x.id);
    }

    #[test]
    #[should_panic(expected = "lockup expired")]
    fn test_release_after_expiry_rejected() {
        let x = setup();
        x.env.ledger().set_sequence_number(x.env.ledger().sequence() + 101);
        x.c.release_education_funds(&x.recipient, &x.id);
    }
}
