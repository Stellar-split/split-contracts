//! Advance payment deposits with refund capability.
//!
//! Allows payers (depositors) to place advance payment deposits with the
//! contract for a designated recipient (and optional invoice). Recipients can
//! apply deposits (in full or in part) toward settlement, while depositors or
//! recipients can trigger full or partial refunds. Every stroop is accounted
//! for by the invariant `applied_amount + refunded_amount + remaining_amount == amount`.

use crate::SplitContract;
#[allow(unused_imports)]
use crate::{SplitContractArgs, SplitContractClient};
use soroban_sdk::{contractimpl, contracttype, symbol_short, token, Address, Env, Vec};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DepositKey {
    NextId,
    Deposit(u64),
    /// Deposit IDs indexed by depositor: Vec<u64>
    ByDepositor(Address),
    /// Deposit IDs indexed by recipient: Vec<u64>
    ByRecipient(Address),
    /// Deposit IDs indexed by invoice ID: Vec<u64>
    ByInvoice(u64),
    /// Aggregate deposit accounting summary
    Summary,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DepositStatus {
    /// Deposit is active with full balance available.
    Active,
    /// Part of the deposit has been applied; remaining balance is still held.
    PartiallyApplied,
    /// Entire deposit has been applied to the recipient.
    Applied,
    /// Part of the deposit has been refunded; remaining balance is still held.
    PartiallyRefunded,
    /// Entire remaining deposit has been refunded to the depositor.
    Refunded,
    /// Deposit passed its expiry ledger and remaining funds were reclaimed.
    Expired,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdvanceDeposit {
    pub id: u64,
    pub depositor: Address,
    pub recipient: Address,
    pub token: Address,
    pub amount: i128,
    pub applied_amount: i128,
    pub refunded_amount: i128,
    pub remaining_amount: i128,
    pub invoice_id: Option<u64>,
    pub created_ledger: u32,
    pub expiry_ledger: u32,
    pub status: DepositStatus,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepositSummary {
    pub total_deposits: u64,
    pub total_deposited: i128,
    pub total_applied: i128,
    pub total_refunded: i128,
    pub total_held: i128,
}

fn load_deposit(env: &Env, id: u64) -> AdvanceDeposit {
    env.storage()
        .persistent()
        .get(&DepositKey::Deposit(id))
        .expect("deposit not found")
}

fn save_deposit(env: &Env, deposit: &AdvanceDeposit) {
    env.storage()
        .persistent()
        .set(&DepositKey::Deposit(deposit.id), deposit);
}

fn load_summary(env: &Env) -> DepositSummary {
    env.storage()
        .persistent()
        .get(&DepositKey::Summary)
        .unwrap_or(DepositSummary {
            total_deposits: 0,
            total_deposited: 0,
            total_applied: 0,
            total_refunded: 0,
            total_held: 0,
        })
}

fn save_summary(env: &Env, summary: &DepositSummary) {
    env.storage()
        .persistent()
        .set(&DepositKey::Summary, summary);
}

fn push_index(env: &Env, key: &DepositKey, id: u64) {
    let mut ids: Vec<u64> = env
        .storage()
        .persistent()
        .get(key)
        .unwrap_or(Vec::new(env));
    ids.push_back(id);
    env.storage().persistent().set(key, &ids);
}

fn is_terminal(status: &DepositStatus) -> bool {
    matches!(
        status,
        DepositStatus::Applied | DepositStatus::Refunded | DepositStatus::Expired
    )
}

#[contractimpl]
impl SplitContract {
    /// Create an advance payment deposit held by the contract for `recipient`.
    ///
    /// Pass `expiry_ledger = 0` for a non-expiring deposit, or a future ledger
    /// sequence after which the depositor can reclaim any unapplied balance.
    /// Returns the newly assigned deposit ID.
    pub fn create_advance_deposit(
        env: Env,
        depositor: Address,
        recipient: Address,
        token: Address,
        amount: i128,
        invoice_id: Option<u64>,
        expiry_ledger: u32,
    ) -> u64 {
        depositor.require_auth();
        assert!(amount > 0, "deposit amount must be positive");
        let now = env.ledger().sequence();
        if expiry_ledger != 0 {
            assert!(
                expiry_ledger > now,
                "expiry ledger must be in the future"
            );
        }

        token::Client::new(&env, &token).transfer(
            &depositor,
            &env.current_contract_address(),
            &amount,
        );

        let id: u64 = env
            .storage()
            .persistent()
            .get(&DepositKey::NextId)
            .unwrap_or(1u64);
        env.storage()
            .persistent()
            .set(&DepositKey::NextId, &(id + 1));

        let deposit = AdvanceDeposit {
            id,
            depositor: depositor.clone(),
            recipient: recipient.clone(),
            token,
            amount,
            applied_amount: 0,
            refunded_amount: 0,
            remaining_amount: amount,
            invoice_id,
            created_ledger: now,
            expiry_ledger,
            status: DepositStatus::Active,
        };
        save_deposit(&env, &deposit);

        push_index(&env, &DepositKey::ByDepositor(depositor.clone()), id);
        push_index(&env, &DepositKey::ByRecipient(recipient.clone()), id);
        if let Some(inv_id) = invoice_id {
            push_index(&env, &DepositKey::ByInvoice(inv_id), id);
        }

        let mut summary = load_summary(&env);
        summary.total_deposits = summary
            .total_deposits
            .checked_add(1)
            .expect("deposit count overflow");
        summary.total_deposited = summary
            .total_deposited
            .checked_add(amount)
            .expect("deposit total overflow");
        summary.total_held = summary
            .total_held
            .checked_add(amount)
            .expect("held total overflow");
        save_summary(&env, &summary);

        env.events().publish(
            (symbol_short!("split"), symbol_short!("dep_new"), id),
            (depositor, recipient, amount, expiry_ledger),
        );

        id
    }

    /// Apply `amount` from an advance deposit to its designated `recipient`.
    /// Supports both partial and full application.
    pub fn apply_advance_deposit(
        env: Env,
        recipient: Address,
        deposit_id: u64,
        amount: i128,
    ) -> AdvanceDeposit {
        recipient.require_auth();
        assert!(amount > 0, "apply amount must be positive");

        let mut deposit = load_deposit(&env, deposit_id);
        assert!(deposit.recipient == recipient, "not the deposit recipient");
        assert!(!is_terminal(&deposit.status), "deposit is already closed");
        if deposit.expiry_ledger != 0 {
            assert!(
                env.ledger().sequence() <= deposit.expiry_ledger,
                "deposit has expired"
            );
        }
        assert!(
            amount <= deposit.remaining_amount,
            "apply amount exceeds remaining deposit"
        );

        token::Client::new(&env, &deposit.token).transfer(
            &env.current_contract_address(),
            &recipient,
            &amount,
        );

        deposit.applied_amount = deposit
            .applied_amount
            .checked_add(amount)
            .expect("applied overflow");
        deposit.remaining_amount = deposit
            .remaining_amount
            .checked_sub(amount)
            .expect("remaining underflow");
        deposit.status = if deposit.remaining_amount == 0 {
            DepositStatus::Applied
        } else {
            DepositStatus::PartiallyApplied
        };
        save_deposit(&env, &deposit);

        let mut summary = load_summary(&env);
        summary.total_applied = summary
            .total_applied
            .checked_add(amount)
            .expect("total applied overflow");
        summary.total_held = summary
            .total_held
            .checked_sub(amount)
            .expect("total held underflow");
        save_summary(&env, &summary);

        env.events().publish(
            (symbol_short!("split"), symbol_short!("dep_app"), deposit_id),
            (recipient, amount, deposit.remaining_amount),
        );

        deposit
    }

    /// Refund `amount` from an advance deposit back to its original `depositor`.
    ///
    /// Authorization:
    /// - The `recipient` may refund any remaining balance at any time.
    /// - The `depositor` may refund if the deposit has not yet been applied
    ///   (`applied_amount == 0`), or if the deposit has passed `expiry_ledger`.
    pub fn refund_advance_deposit(
        env: Env,
        caller: Address,
        deposit_id: u64,
        amount: i128,
    ) -> AdvanceDeposit {
        caller.require_auth();
        assert!(amount > 0, "refund amount must be positive");

        let mut deposit = load_deposit(&env, deposit_id);
        assert!(!is_terminal(&deposit.status), "deposit is already closed");
        assert!(
            amount <= deposit.remaining_amount,
            "refund amount exceeds remaining deposit"
        );

        let now = env.ledger().sequence();
        let is_expired = deposit.expiry_ledger != 0 && now > deposit.expiry_ledger;

        let authorized = if caller == deposit.recipient {
            true
        } else if caller == deposit.depositor {
            is_expired || deposit.applied_amount == 0
        } else {
            false
        };
        assert!(authorized, "not authorized to refund deposit");

        token::Client::new(&env, &deposit.token).transfer(
            &env.current_contract_address(),
            &deposit.depositor,
            &amount,
        );

        deposit.refunded_amount = deposit
            .refunded_amount
            .checked_add(amount)
            .expect("refunded overflow");
        deposit.remaining_amount = deposit
            .remaining_amount
            .checked_sub(amount)
            .expect("remaining underflow");
        deposit.status = if deposit.remaining_amount == 0 {
            if is_expired {
                DepositStatus::Expired
            } else {
                DepositStatus::Refunded
            }
        } else {
            DepositStatus::PartiallyRefunded
        };
        save_deposit(&env, &deposit);

        let mut summary = load_summary(&env);
        summary.total_refunded = summary
            .total_refunded
            .checked_add(amount)
            .expect("total refunded overflow");
        summary.total_held = summary
            .total_held
            .checked_sub(amount)
            .expect("total held underflow");
        save_summary(&env, &summary);

        env.events().publish(
            (symbol_short!("split"), symbol_short!("dep_ref"), deposit_id),
            (deposit.depositor.clone(), amount, deposit.remaining_amount),
        );

        deposit
    }

    /// Convenience helper to refund the entire remaining balance of a deposit.
    pub fn refund_full_advance_deposit(
        env: Env,
        caller: Address,
        deposit_id: u64,
    ) -> AdvanceDeposit {
        let deposit = load_deposit(&env, deposit_id);
        let remaining = deposit.remaining_amount;
        Self::refund_advance_deposit(env, caller, deposit_id, remaining)
    }

    /// Fetch a deposit record by ID (O(1)).
    pub fn get_advance_deposit(env: Env, deposit_id: u64) -> AdvanceDeposit {
        load_deposit(&env, deposit_id)
    }

    /// Fetch only the current status of a deposit by ID (O(1)).
    pub fn get_advance_deposit_status(env: Env, deposit_id: u64) -> DepositStatus {
        load_deposit(&env, deposit_id).status
    }

    /// Fetch all deposit IDs created by `depositor` (O(1) index lookup).
    pub fn get_depositor_deposits(env: Env, depositor: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&DepositKey::ByDepositor(depositor))
            .unwrap_or(Vec::new(&env))
    }

    /// Fetch all deposit IDs addressed to `recipient` (O(1) index lookup).
    pub fn get_recipient_deposits(env: Env, recipient: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&DepositKey::ByRecipient(recipient))
            .unwrap_or(Vec::new(&env))
    }

    /// Fetch all deposit IDs linked to `invoice_id` (O(1) index lookup).
    pub fn get_invoice_deposits(env: Env, invoice_id: u64) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&DepositKey::ByInvoice(invoice_id))
            .unwrap_or(Vec::new(&env))
    }

    /// Fetch aggregate deposit accounting summary (O(1)).
    pub fn get_deposit_summary(env: Env) -> DepositSummary {
        load_summary(&env)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger as _};
    use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
    use soroban_sdk::Env;

    fn setup() -> (Env, SplitContractClient<'static>, Address, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().with_mut(|l| l.sequence_number = 100);
        let contract_id = env.register(SplitContract, ());
        let client = SplitContractClient::new(&env, &contract_id);

        let token_admin = Address::generate(&env);
        let token_id = env.register_stellar_asset_contract_v2(token_admin).address();
        let depositor = Address::generate(&env);
        let recipient = Address::generate(&env);
        StellarAssetClient::new(&env, &token_id).mint(&depositor, &10_000);

        (env, client, token_id, depositor, recipient)
    }

    #[test]
    fn test_deposit_and_full_apply() {
        let (env, client, token, depositor, recipient) = setup();
        let tok = TokenClient::new(&env, &token);

        let id = client.create_advance_deposit(&depositor, &recipient, &token, &1_000, &Some(42), &500);
        assert_eq!(id, 1);
        assert_eq!(tok.balance(&depositor), 9_000);

        let dep = client.get_advance_deposit(&id);
        assert_eq!(dep.status, DepositStatus::Active);
        assert_eq!(dep.remaining_amount, 1_000);
        assert_eq!(dep.invoice_id, Some(42));

        let updated = client.apply_advance_deposit(&recipient, &id, &1_000);
        assert_eq!(updated.status, DepositStatus::Applied);
        assert_eq!(updated.applied_amount, 1_000);
        assert_eq!(updated.remaining_amount, 0);
        assert_eq!(tok.balance(&recipient), 1_000);

        let summary = client.get_deposit_summary();
        assert_eq!(summary.total_deposits, 1);
        assert_eq!(summary.total_deposited, 1_000);
        assert_eq!(summary.total_applied, 1_000);
        assert_eq!(summary.total_refunded, 0);
        assert_eq!(summary.total_held, 0);
    }

    #[test]
    fn test_partial_apply_and_recipient_refund_no_lost_funds() {
        let (env, client, token, depositor, recipient) = setup();
        let tok = TokenClient::new(&env, &token);

        let id = client.create_advance_deposit(&depositor, &recipient, &token, &1_000, &None, &0);

        // Recipient applies 400
        let d1 = client.apply_advance_deposit(&recipient, &id, &400);
        assert_eq!(d1.status, DepositStatus::PartiallyApplied);
        assert_eq!(d1.applied_amount, 400);
        assert_eq!(d1.remaining_amount, 600);

        // Recipient refunds 250
        let d2 = client.refund_advance_deposit(&recipient, &id, &250);
        assert_eq!(d2.status, DepositStatus::PartiallyRefunded);
        assert_eq!(d2.refunded_amount, 250);
        assert_eq!(d2.remaining_amount, 350);

        // Recipient refunds remaining 350 via full refund helper
        let d3 = client.refund_full_advance_deposit(&recipient, &id);
        assert_eq!(d3.status, DepositStatus::Refunded);
        assert_eq!(d3.applied_amount, 400);
        assert_eq!(d3.refunded_amount, 600);
        assert_eq!(d3.remaining_amount, 0);

        // Conservation of funds: no lost deposits
        assert_eq!(tok.balance(&depositor), 9_600);
        assert_eq!(tok.balance(&recipient), 400);
        assert_eq!(tok.balance(&depositor) + tok.balance(&recipient), 10_000);

        let summary = client.get_deposit_summary();
        assert_eq!(summary.total_held, 0);
        assert_eq!(summary.total_applied + summary.total_refunded, summary.total_deposited);
    }

    #[test]
    fn test_depositor_self_refund_before_application() {
        let (env, client, token, depositor, recipient) = setup();
        let tok = TokenClient::new(&env, &token);

        let id = client.create_advance_deposit(&depositor, &recipient, &token, &500, &None, &0);
        let refunded = client.refund_full_advance_deposit(&depositor, &id);
        assert_eq!(refunded.status, DepositStatus::Refunded);
        assert_eq!(refunded.refunded_amount, 500);
        assert_eq!(refunded.remaining_amount, 0);
        assert_eq!(tok.balance(&depositor), 10_000);
    }

    #[test]
    fn test_depositor_reclaim_after_expiry() {
        let (env, client, token, depositor, recipient) = setup();
        let tok = TokenClient::new(&env, &token);

        let id = client.create_advance_deposit(&depositor, &recipient, &token, &1_000, &None, &200);
        // Recipient applies 300 before expiry
        client.apply_advance_deposit(&recipient, &id, &300);

        // Advance past expiry
        env.ledger().with_mut(|l| l.sequence_number = 201);
        let reclaimed = client.refund_full_advance_deposit(&depositor, &id);
        assert_eq!(reclaimed.status, DepositStatus::Expired);
        assert_eq!(reclaimed.applied_amount, 300);
        assert_eq!(reclaimed.refunded_amount, 700);
        assert_eq!(reclaimed.remaining_amount, 0);
        assert_eq!(tok.balance(&depositor), 9_700);
        assert_eq!(tok.balance(&recipient), 300);
    }

    #[test]
    fn test_efficient_queries_and_indexes() {
        let (env, client, token, depositor, recipient) = setup();
        let other_recipient = Address::generate(&env);

        let id1 = client.create_advance_deposit(&depositor, &recipient, &token, &200, &Some(7), &0);
        let id2 = client.create_advance_deposit(&depositor, &other_recipient, &token, &300, &Some(7), &0);
        let id3 = client.create_advance_deposit(&depositor, &recipient, &token, &500, &Some(8), &0);

        assert_eq!(client.get_depositor_deposits(&depositor).len(), 3);
        assert_eq!(client.get_recipient_deposits(&recipient).len(), 2);
        assert_eq!(client.get_recipient_deposits(&other_recipient).len(), 1);
        assert_eq!(client.get_invoice_deposits(&7).len(), 2);
        assert_eq!(client.get_invoice_deposits(&8).len(), 1);
        assert_eq!(client.get_advance_deposit_status(&id1), DepositStatus::Active);
        assert_eq!(client.get_advance_deposit_status(&id2), DepositStatus::Active);
        assert_eq!(client.get_advance_deposit_status(&id3), DepositStatus::Active);

        let summary = client.get_deposit_summary();
        assert_eq!(summary.total_deposits, 3);
        assert_eq!(summary.total_deposited, 1_000);
        assert_eq!(summary.total_held, 1_000);
    }

    #[test]
    #[should_panic(expected = "not authorized to refund deposit")]
    fn test_depositor_cannot_refund_partially_applied_before_expiry() {
        let (_env, client, token, depositor, recipient) = setup();
        let id = client.create_advance_deposit(&depositor, &recipient, &token, &1_000, &None, &500);
        client.apply_advance_deposit(&recipient, &id, &200);
        // Depositor cannot unilaterally pull back remaining funds before expiry once partially applied
        client.refund_advance_deposit(&depositor, &id, &800);
    }

    #[test]
    #[should_panic(expected = "deposit has expired")]
    fn test_cannot_apply_expired_deposit() {
        let (env, client, token, depositor, recipient) = setup();
        let id = client.create_advance_deposit(&depositor, &recipient, &token, &500, &None, &150);
        env.ledger().with_mut(|l| l.sequence_number = 151);
        client.apply_advance_deposit(&recipient, &id, &500);
    }

    #[test]
    #[should_panic(expected = "deposit is already closed")]
    fn test_cannot_double_refund() {
        let (_env, client, token, depositor, recipient) = setup();
        let id = client.create_advance_deposit(&depositor, &recipient, &token, &500, &None, &0);
        client.refund_full_advance_deposit(&depositor, &id);
        client.refund_advance_deposit(&depositor, &id, &1);
    }

    #[test]
    #[should_panic(expected = "deposit amount must be positive")]
    fn test_zero_deposit_rejected() {
        let (_env, client, token, depositor, recipient) = setup();
        client.create_advance_deposit(&depositor, &recipient, &token, &0, &None, &0);
    }
}
