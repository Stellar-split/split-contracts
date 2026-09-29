//! Invoice insurance pool — automatic refund protection (issue #853).
//!
//! Underwriters deposit liquidity into a per-token pool and receive shares.
//! A payer who has deposited into an escrow invoice can buy refund protection
//! for that deposit by paying a premium (`premium_bps` of the coverage) into
//! the pool; the coverage amount is locked so providers cannot withdraw it.
//!
//! Escrow already refunds payers when an invoice fails to fund. Protection
//! covers the remaining risk: the invoice is released to the creator, who then
//! fails to deliver. When the admin declares such an invoice defaulted, every
//! active, in-term policy on it is refunded from the pool automatically, in the
//! same call — payers do not need to file claims.
//!
//! Policies lapse (and their coverage is unlocked) once their term ends or the
//! invoice is refunded / cancelled through the normal escrow path.

use soroban_sdk::{symbol_short, token, Address, Env, Symbol, Vec};

use crate::errors::Error;
use crate::types::{
    EscrowStatus, InsuranceConfig, InsurancePolicy, InsurancePool, PolicyStatus,
};
use crate::{deposit_of, get_invoice, require_admin, require_initialized};

/// Default premium: 2 % of coverage.
pub(crate) const DEFAULT_PREMIUM_BPS: u32 = 200;
/// Default policy term: 30 days.
pub(crate) const DEFAULT_POLICY_DURATION: u64 = 30 * 24 * 60 * 60;
/// Upper bound on the premium rate: 50 % of coverage.
pub(crate) const MAX_PREMIUM_BPS: u32 = 5_000;
/// Maximum number of policies per invoice (bounds `declare_default` work).
pub(crate) const MAX_POLICIES_PER_INVOICE: u32 = 50;

const BPS_DENOMINATOR: i128 = 10_000;

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Instance storage: insurance configuration.
fn config_key() -> Symbol {
    symbol_short!("ins_cfg")
}

/// Persistent storage: pool state per token.
fn pool_key(token: &Address) -> (Symbol, Address) {
    (symbol_short!("ins_pool"), token.clone())
}

/// Persistent storage: provider share balance per token.
fn shares_key(token: &Address, provider: &Address) -> (Symbol, Address, Address) {
    (symbol_short!("ins_share"), token.clone(), provider.clone())
}

/// Persistent storage: policy per `(invoice, payer)`.
fn policy_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("ins_pol"), invoice_id, payer.clone())
}

/// Persistent storage: list of insured payers per invoice.
fn insured_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("ins_list"), invoice_id)
}

/// Persistent storage: set once an invoice has been declared defaulted.
fn defaulted_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("ins_dflt"), invoice_id)
}

pub(crate) fn get_config(env: &Env) -> InsuranceConfig {
    env.storage()
        .instance()
        .get(&config_key())
        .unwrap_or(InsuranceConfig {
            premium_bps: DEFAULT_PREMIUM_BPS,
            policy_duration: DEFAULT_POLICY_DURATION,
        })
}

pub(crate) fn get_pool(env: &Env, token: &Address) -> InsurancePool {
    env.storage()
        .persistent()
        .get(&pool_key(token))
        .unwrap_or(InsurancePool {
            total_liquidity: 0,
            locked_coverage: 0,
            total_shares: 0,
            premiums_collected: 0,
            claims_paid: 0,
        })
}

fn save_pool(env: &Env, token: &Address, pool: &InsurancePool) {
    env.storage().persistent().set(&pool_key(token), pool);
}

pub(crate) fn get_shares(env: &Env, token: &Address, provider: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&shares_key(token, provider))
        .unwrap_or(0)
}

pub(crate) fn get_policy(env: &Env, invoice_id: u64, payer: &Address) -> Option<InsurancePolicy> {
    env.storage().persistent().get(&policy_key(invoice_id, payer))
}

fn save_policy(env: &Env, policy: &InsurancePolicy) {
    env.storage()
        .persistent()
        .set(&policy_key(policy.invoice_id, &policy.payer), policy);
}

pub(crate) fn is_defaulted(env: &Env, invoice_id: u64) -> bool {
    env.storage()
        .persistent()
        .get(&defaulted_key(invoice_id))
        .unwrap_or(false)
}

fn free_liquidity(pool: &InsurancePool) -> i128 {
    pool.total_liquidity - pool.locked_coverage
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Topics: `(insurance, config)` — Data: `(premium_bps, policy_duration)`
fn emit_config(env: &Env, cfg: &InsuranceConfig) {
    env.events().publish(
        (symbol_short!("insurance"), symbol_short!("config")),
        (cfg.premium_bps, cfg.policy_duration),
    );
}

/// Topics: `(insurance, lp_add, token)` — Data: `(provider, amount, shares)`
fn emit_lp_add(env: &Env, token: &Address, provider: &Address, amount: i128, shares: i128) {
    env.events().publish(
        (symbol_short!("insurance"), symbol_short!("lp_add"), token.clone()),
        (provider.clone(), amount, shares),
    );
}

/// Topics: `(insurance, lp_remove, token)` — Data: `(provider, amount, shares)`
fn emit_lp_remove(env: &Env, token: &Address, provider: &Address, amount: i128, shares: i128) {
    env.events().publish(
        (symbol_short!("insurance"), symbol_short!("lp_remove"), token.clone()),
        (provider.clone(), amount, shares),
    );
}

/// Topics: `(insurance, policy, invoice_id)` — Data: `(payer, coverage, premium, expires_at)`
fn emit_policy(env: &Env, p: &InsurancePolicy) {
    env.events().publish(
        (symbol_short!("insurance"), symbol_short!("policy"), p.invoice_id),
        (p.payer.clone(), p.coverage, p.premium, p.expires_at),
    );
}

/// Topics: `(insurance, claimed, invoice_id)` — Data: `(payer, amount)`
fn emit_claimed(env: &Env, invoice_id: u64, payer: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("insurance"), symbol_short!("claimed"), invoice_id),
        (payer.clone(), amount),
    );
}

/// Topics: `(insurance, expired, invoice_id)` — Data: `(payer, coverage)`
fn emit_expired(env: &Env, invoice_id: u64, payer: &Address, coverage: i128) {
    env.events().publish(
        (symbol_short!("insurance"), symbol_short!("expired"), invoice_id),
        (payer.clone(), coverage),
    );
}

/// Topics: `(insurance, default, invoice_id)` — Data: `(total_paid, policies_paid)`
fn emit_default(env: &Env, invoice_id: u64, total_paid: i128, count: u32) {
    env.events().publish(
        (symbol_short!("insurance"), symbol_short!("default"), invoice_id),
        (total_paid, count),
    );
}

// ---------------------------------------------------------------------------
// Entry points (wrapped by the contract impl in lib.rs)
// ---------------------------------------------------------------------------

pub(crate) fn set_config(env: &Env, premium_bps: u32, policy_duration: u64) -> Result<(), Error> {
    require_initialized(env)?;
    require_admin(env);
    if premium_bps == 0 || premium_bps > MAX_PREMIUM_BPS || policy_duration == 0 {
        return Err(Error::InvalidConfig);
    }
    let cfg = InsuranceConfig {
        premium_bps,
        policy_duration,
    };
    env.storage().instance().set(&config_key(), &cfg);
    emit_config(env, &cfg);
    Ok(())
}

pub(crate) fn provide(
    env: &Env,
    provider: Address,
    token: Address,
    amount: i128,
) -> Result<i128, Error> {
    require_initialized(env)?;
    if amount <= 0 {
        return Err(Error::InvalidAmount);
    }
    provider.require_auth();

    let mut pool = get_pool(env, &token);
    let shares = if pool.total_shares == 0 {
        amount
    } else if pool.total_liquidity == 0 {
        return Err(Error::InsufficientPoolLiquidity);
    } else {
        amount
            .checked_mul(pool.total_shares)
            .expect("share mint overflow")
            / pool.total_liquidity
    };
    if shares <= 0 {
        return Err(Error::InvalidAmount);
    }

    token::Client::new(env, &token).transfer(&provider, &env.current_contract_address(), &amount);

    pool.total_liquidity += amount;
    pool.total_shares += shares;
    save_pool(env, &token, &pool);
    let balance = get_shares(env, &token, &provider) + shares;
    env.storage()
        .persistent()
        .set(&shares_key(&token, &provider), &balance);

    emit_lp_add(env, &token, &provider, amount, shares);
    Ok(shares)
}

pub(crate) fn withdraw(
    env: &Env,
    provider: Address,
    token: Address,
    shares: i128,
) -> Result<i128, Error> {
    if shares <= 0 {
        return Err(Error::InvalidAmount);
    }
    provider.require_auth();

    let balance = get_shares(env, &token, &provider);
    if balance < shares {
        return Err(Error::InsufficientShares);
    }
    let mut pool = get_pool(env, &token);
    let amount = shares
        .checked_mul(pool.total_liquidity)
        .expect("share redeem overflow")
        / pool.total_shares;
    if amount > free_liquidity(&pool) {
        return Err(Error::InsufficientPoolLiquidity);
    }

    pool.total_liquidity -= amount;
    pool.total_shares -= shares;
    save_pool(env, &token, &pool);
    env.storage()
        .persistent()
        .set(&shares_key(&token, &provider), &(balance - shares));

    if amount > 0 {
        token::Client::new(env, &token).transfer(
            &env.current_contract_address(),
            &provider,
            &amount,
        );
    }
    emit_lp_remove(env, &token, &provider, amount, shares);
    Ok(amount)
}

pub(crate) fn buy(env: &Env, payer: Address, invoice_id: u64) -> Result<i128, Error> {
    payer.require_auth();
    let invoice = get_invoice(env, invoice_id)?;
    if invoice.status != EscrowStatus::Pending && invoice.status != EscrowStatus::Active {
        return Err(Error::InvalidStatus);
    }
    if get_policy(env, invoice_id, &payer).is_some() {
        return Err(Error::PolicyAlreadyExists);
    }
    let coverage = deposit_of(env, invoice_id, &payer);
    if coverage <= 0 {
        return Err(Error::NoDepositToInsure);
    }
    let mut insured: Vec<Address> = env
        .storage()
        .persistent()
        .get(&insured_key(invoice_id))
        .unwrap_or_else(|| Vec::new(env));
    if insured.len() >= MAX_POLICIES_PER_INVOICE {
        return Err(Error::CapacityReached);
    }

    let cfg = get_config(env);
    let mut pool = get_pool(env, &invoice.token);
    if coverage > free_liquidity(&pool) {
        return Err(Error::InsufficientPoolLiquidity);
    }
    let premium = (coverage
        .checked_mul(cfg.premium_bps as i128)
        .expect("premium overflow")
        / BPS_DENOMINATOR)
        .max(1);

    token::Client::new(env, &invoice.token).transfer(
        &payer,
        &env.current_contract_address(),
        &premium,
    );

    pool.total_liquidity += premium;
    pool.locked_coverage += coverage;
    pool.premiums_collected += premium;
    save_pool(env, &invoice.token, &pool);

    let policy = InsurancePolicy {
        payer: payer.clone(),
        invoice_id,
        token: invoice.token.clone(),
        coverage,
        premium,
        expires_at: env
            .ledger()
            .timestamp()
            .saturating_add(cfg.policy_duration),
        status: PolicyStatus::Active,
    };
    save_policy(env, &policy);
    insured.push_back(payer);
    env.storage()
        .persistent()
        .set(&insured_key(invoice_id), &insured);

    emit_policy(env, &policy);
    Ok(premium)
}

pub(crate) fn declare_default(env: &Env, invoice_id: u64) -> Result<i128, Error> {
    require_initialized(env)?;
    require_admin(env);
    let invoice = get_invoice(env, invoice_id)?;
    if invoice.status != EscrowStatus::Released {
        return Err(Error::InvalidStatus);
    }
    if is_defaulted(env, invoice_id) {
        return Err(Error::InvoiceAlreadyDefaulted);
    }
    env.storage()
        .persistent()
        .set(&defaulted_key(invoice_id), &true);

    let insured: Vec<Address> = env
        .storage()
        .persistent()
        .get(&insured_key(invoice_id))
        .unwrap_or_else(|| Vec::new(env));
    let token_client = token::Client::new(env, &invoice.token);
    let contract = env.current_contract_address();
    let now = env.ledger().timestamp();
    let mut pool = get_pool(env, &invoice.token);
    let mut total_paid: i128 = 0;
    let mut count: u32 = 0;

    for payer in insured.iter() {
        let mut policy = match get_policy(env, invoice_id, &payer) {
            Some(p) if p.status == PolicyStatus::Active => p,
            _ => continue,
        };
        pool.locked_coverage -= policy.coverage;
        if now > policy.expires_at {
            policy.status = PolicyStatus::Expired;
            emit_expired(env, invoice_id, &payer, policy.coverage);
        } else {
            pool.total_liquidity -= policy.coverage;
            pool.claims_paid += policy.coverage;
            policy.status = PolicyStatus::Claimed;
            token_client.transfer(&contract, &payer, &policy.coverage);
            emit_claimed(env, invoice_id, &payer, policy.coverage);
            total_paid += policy.coverage;
            count += 1;
        }
        save_policy(env, &policy);
    }
    save_pool(env, &invoice.token, &pool);

    emit_default(env, invoice_id, total_paid, count);
    Ok(total_paid)
}

pub(crate) fn expire(env: &Env, invoice_id: u64, payer: Address) -> Result<(), Error> {
    let mut policy = get_policy(env, invoice_id, &payer).ok_or(Error::PolicyNotFound)?;
    if policy.status != PolicyStatus::Active {
        return Err(Error::PolicyNotActive);
    }
    let invoice = get_invoice(env, invoice_id)?;
    let lapsed = env.ledger().timestamp() > policy.expires_at
        || invoice.status == EscrowStatus::Refunded
        || invoice.status == EscrowStatus::Cancelled;
    if !lapsed {
        return Err(Error::InvalidStatus);
    }

    let mut pool = get_pool(env, &policy.token);
    pool.locked_coverage -= policy.coverage;
    save_pool(env, &policy.token, &pool);
    policy.status = PolicyStatus::Expired;
    save_policy(env, &policy);
    emit_expired(env, invoice_id, &payer, policy.coverage);
    Ok(())
}
