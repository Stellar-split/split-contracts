//! Issue #858: Recipient earnings insurance.
//!
//! A recipient can insure their expected earnings from a pending invoice by
//! paying a premium into a per-token insurance pool. If the invoice ends
//! without paying out (refunded, expired or cancelled) the recipient claims
//! their coverage from the pool. If the invoice is released, anyone may settle
//! the policy, which frees the reserved capital for new policies.
//!
//! Solvency: coverage is reserved against pool capital when a policy is bought,
//! so a policy can only be sold when `capital - reserved >= coverage`.

use crate::error::ContractError;
use crate::types::{EarningsInsurancePool, EarningsPolicy, EarningsPolicyStatus, InvoiceStatus};
use crate::{
    events, load_invoice, require_admin, require_not_paused, SplitContract, SplitContractArgs,
    SplitContractClient,
};
use soroban_sdk::{contractimpl, panic_with_error, symbol_short, token, Address, Env, Symbol};

/// Upper bound on the premium rate (50%).
pub const MAX_EARNINGS_PREMIUM_BPS: u32 = 5_000;

fn pool_key(token: &Address) -> (Symbol, Address) {
    (symbol_short!("ei_pool"), token.clone())
}

fn policy_key(invoice_id: u64, recipient: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("ei_pol"), invoice_id, recipient.clone())
}

fn load_pool(env: &Env, token: &Address) -> Option<EarningsInsurancePool> {
    env.storage().persistent().get(&pool_key(token))
}

fn require_pool(env: &Env, token: &Address) -> EarningsInsurancePool {
    load_pool(env, token)
        .unwrap_or_else(|| panic_with_error!(env, ContractError::InsuranceNotConfigured))
}

fn save_pool(env: &Env, pool: &EarningsInsurancePool) {
    env.storage().persistent().set(&pool_key(&pool.token), pool);
}

fn require_policy(env: &Env, invoice_id: u64, recipient: &Address) -> EarningsPolicy {
    env.storage()
        .persistent()
        .get(&policy_key(invoice_id, recipient))
        .unwrap_or_else(|| panic_with_error!(env, ContractError::PolicyNotFound))
}

fn save_policy(env: &Env, policy: &EarningsPolicy) {
    env.storage()
        .persistent()
        .set(&policy_key(policy.invoice_id, &policy.recipient), policy);
}

fn require_is_admin(env: &Env, admin: &Address) {
    if require_admin(env) != *admin {
        panic_with_error!(env, ContractError::NotAuthorized);
    }
}

/// Premium for `coverage` at `premium_bps`, rounded up so tiny policies are never free.
fn premium_for(env: &Env, coverage: i128, premium_bps: u32) -> i128 {
    coverage
        .checked_mul(premium_bps as i128)
        .map(|v| (v + 9_999) / 10_000)
        .unwrap_or_else(|| panic_with_error!(env, ContractError::ArithmeticOverflow))
}

fn is_failed(status: &InvoiceStatus) -> bool {
    matches!(
        status,
        InvoiceStatus::Refunded | InvoiceStatus::Expired | InvoiceStatus::Cancelled
    )
}

fn is_paid_out(status: &InvoiceStatus) -> bool {
    matches!(status, InvoiceStatus::Released | InvoiceStatus::Finalised)
}

#[contractimpl]
impl SplitContract {
    /// Admin: enable earnings insurance for `token` and set the premium rate.
    pub fn configure_earnings_insurance(env: Env, admin: Address, token: Address, premium_bps: u32) {
        require_is_admin(&env, &admin);
        if premium_bps > MAX_EARNINGS_PREMIUM_BPS {
            panic_with_error!(&env, ContractError::InvalidFeeConfig);
        }
        let pool = load_pool(&env, &token).unwrap_or(EarningsInsurancePool {
            token: token.clone(),
            premium_bps,
            capital: 0,
            reserved: 0,
        });
        save_pool(&env, &EarningsInsurancePool { premium_bps, ..pool });
        events::earnings_insurance_configured(&env, &token, premium_bps);
    }

    /// Add underwriting capital to the insurance pool for `token`.
    pub fn fund_earnings_insurance(env: Env, funder: Address, token: Address, amount: i128) {
        require_not_paused(&env);
        funder.require_auth();
        if amount <= 0 {
            panic_with_error!(&env, ContractError::ZeroAmountNotAllowed);
        }
        let mut pool = require_pool(&env, &token);
        token::Client::new(&env, &token).transfer(&funder, &env.current_contract_address(), &amount);
        pool.capital += amount;
        save_pool(&env, &pool);
        events::earnings_insurance_funded(&env, &token, &funder, amount, pool.capital);
    }

    /// Admin: withdraw unreserved capital from the insurance pool.
    pub fn withdraw_earnings_insurance(
        env: Env,
        admin: Address,
        token: Address,
        amount: i128,
        to: Address,
    ) {
        require_is_admin(&env, &admin);
        if amount <= 0 {
            panic_with_error!(&env, ContractError::ZeroAmountNotAllowed);
        }
        let mut pool = require_pool(&env, &token);
        if amount > pool.capital - pool.reserved {
            panic_with_error!(&env, ContractError::InsuranceCapacityExceeded);
        }
        pool.capital -= amount;
        save_pool(&env, &pool);
        token::Client::new(&env, &token).transfer(&env.current_contract_address(), &to, &amount);
        events::earnings_insurance_withdrawn(&env, &token, &to, amount, pool.capital);
    }

    /// Recipient buys `coverage` on their earnings from `invoice_id`.
    /// Coverage is denominated in the invoice's funding token and may not
    /// exceed the recipient's allocated amount. Returns the premium charged.
    pub fn buy_earnings_insurance(env: Env, recipient: Address, invoice_id: u64, coverage: i128) -> i128 {
        require_not_paused(&env);
        recipient.require_auth();
        if coverage <= 0 {
            panic_with_error!(&env, ContractError::ZeroAmountNotAllowed);
        }
        if env.storage().persistent().has(&policy_key(invoice_id, &recipient)) {
            panic_with_error!(&env, ContractError::PolicyAlreadyExists);
        }

        let invoice = load_invoice(&env, invoice_id);
        if invoice.status != InvoiceStatus::Pending {
            panic_with_error!(&env, ContractError::InvalidStatus);
        }
        if env.ledger().timestamp() > invoice.deadline {
            panic_with_error!(&env, ContractError::DeadlinePassed);
        }
        let idx = invoice
            .recipients
            .iter()
            .position(|r| r == recipient)
            .unwrap_or_else(|| panic_with_error!(&env, ContractError::RecipientNotFound));
        let allocated = invoice.amounts.get(idx as u32).unwrap_or(0);
        if coverage > allocated {
            panic_with_error!(&env, ContractError::InvalidAmount);
        }

        let token = invoice.funding_token.clone();
        let mut pool = require_pool(&env, &token);
        if pool.capital - pool.reserved < coverage {
            panic_with_error!(&env, ContractError::InsuranceCapacityExceeded);
        }
        let premium = premium_for(&env, coverage, pool.premium_bps);
        if premium > 0 {
            token::Client::new(&env, &token).transfer(&recipient, &env.current_contract_address(), &premium);
        }
        pool.capital += premium;
        pool.reserved += coverage;
        save_pool(&env, &pool);

        save_policy(
            &env,
            &EarningsPolicy {
                invoice_id,
                recipient: recipient.clone(),
                token,
                coverage,
                premium,
                status: EarningsPolicyStatus::Active,
            },
        );
        events::earnings_policy_bought(&env, invoice_id, &recipient, coverage, premium);
        premium
    }

    /// Recipient claims their coverage after the invoice failed to pay out.
    /// Returns the amount paid.
    pub fn claim_earnings_insurance(env: Env, recipient: Address, invoice_id: u64) -> i128 {
        require_not_paused(&env);
        recipient.require_auth();
        let mut policy = require_policy(&env, invoice_id, &recipient);
        if policy.status != EarningsPolicyStatus::Active {
            panic_with_error!(&env, ContractError::PolicyNotClaimable);
        }
        let invoice = load_invoice(&env, invoice_id);
        if !is_failed(&invoice.status) {
            panic_with_error!(&env, ContractError::PolicyNotClaimable);
        }

        let mut pool = require_pool(&env, &policy.token);
        pool.capital -= policy.coverage;
        pool.reserved -= policy.coverage;
        save_pool(&env, &pool);
        policy.status = EarningsPolicyStatus::Claimed;
        save_policy(&env, &policy);

        token::Client::new(&env, &policy.token).transfer(
            &env.current_contract_address(),
            &recipient,
            &policy.coverage,
        );
        events::earnings_policy_claimed(&env, invoice_id, &recipient, policy.coverage);
        policy.coverage
    }

    /// Permissionless: close an active policy whose invoice was released,
    /// freeing the reserved coverage. The premium stays in the pool.
    pub fn settle_earnings_insurance(env: Env, invoice_id: u64, recipient: Address) {
        let mut policy = require_policy(&env, invoice_id, &recipient);
        if policy.status != EarningsPolicyStatus::Active {
            panic_with_error!(&env, ContractError::PolicyNotClaimable);
        }
        let invoice = load_invoice(&env, invoice_id);
        if !is_paid_out(&invoice.status) {
            panic_with_error!(&env, ContractError::PolicyNotClaimable);
        }

        let mut pool = require_pool(&env, &policy.token);
        pool.reserved -= policy.coverage;
        save_pool(&env, &pool);
        policy.status = EarningsPolicyStatus::Settled;
        save_policy(&env, &policy);

        events::earnings_policy_settled(&env, invoice_id, &recipient, policy.coverage);
    }

    /// Quote the premium for `coverage` of `token` at the current rate.
    pub fn quote_earnings_premium(env: Env, token: Address, coverage: i128) -> i128 {
        let pool = require_pool(&env, &token);
        premium_for(&env, coverage, pool.premium_bps)
    }

    pub fn get_earnings_policy(env: Env, invoice_id: u64, recipient: Address) -> Option<EarningsPolicy> {
        env.storage().persistent().get(&policy_key(invoice_id, &recipient))
    }

    pub fn get_earnings_insurance_pool(env: Env, token: Address) -> Option<EarningsInsurancePool> {
        load_pool(&env, &token)
    }
}
