//! Issue #860: Creator liquidity pool mechanism.
//!
//! Each `(creator, token)` pair owns a pool. Anyone may provide liquidity and
//! receive pro-rata shares. The creator may draw liquidity from the pool (for
//! example to front payouts before an invoice is fully funded) and repays the
//! draw plus an optional fee. Fees raise the value of every share, so providers
//! earn yield proportional to their stake.
//!
//! Share accounting: `pool value = available + outstanding`. Deposits mint
//! `amount * total_shares / value` shares (1:1 for the first deposit);
//! withdrawals burn shares for `shares * value / total_shares` tokens, limited
//! to the currently `available` balance.

use crate::error::ContractError;
use crate::types::CreatorPool;
use crate::{events, require_not_paused, SplitContract, SplitContractArgs, SplitContractClient};
use soroban_sdk::{contractimpl, panic_with_error, symbol_short, token, Address, Env, Symbol};

fn pool_key(creator: &Address, token: &Address) -> (Symbol, Address, Address) {
    (symbol_short!("cl_pool"), creator.clone(), token.clone())
}

fn lp_shares_key(creator: &Address, token: &Address, provider: &Address) -> (Symbol, Address, Address, Address) {
    (symbol_short!("cl_lpsh"), creator.clone(), token.clone(), provider.clone())
}

fn load_pool(env: &Env, creator: &Address, token: &Address) -> CreatorPool {
    env.storage()
        .persistent()
        .get(&pool_key(creator, token))
        .unwrap_or(CreatorPool {
            creator: creator.clone(),
            token: token.clone(),
            available: 0,
            outstanding: 0,
            total_shares: 0,
            fees_earned: 0,
        })
}

fn save_pool(env: &Env, pool: &CreatorPool) {
    env.storage()
        .persistent()
        .set(&pool_key(&pool.creator, &pool.token), pool);
}

fn load_shares(env: &Env, creator: &Address, token: &Address, provider: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&lp_shares_key(creator, token, provider))
        .unwrap_or(0)
}

fn save_shares(env: &Env, creator: &Address, token: &Address, provider: &Address, shares: i128) {
    let key = lp_shares_key(creator, token, provider);
    if shares == 0 {
        env.storage().persistent().remove(&key);
    } else {
        env.storage().persistent().set(&key, &shares);
    }
}

/// `a * b / c` in i128 with overflow checks.
fn mul_div(env: &Env, a: i128, b: i128, c: i128) -> i128 {
    a.checked_mul(b)
        .and_then(|v| v.checked_div(c))
        .unwrap_or_else(|| panic_with_error!(env, ContractError::ArithmeticOverflow))
}

fn pool_value(env: &Env, pool: &CreatorPool) -> i128 {
    pool.available
        .checked_add(pool.outstanding)
        .unwrap_or_else(|| panic_with_error!(env, ContractError::ArithmeticOverflow))
}

fn require_positive(env: &Env, amount: i128) {
    if amount <= 0 {
        panic_with_error!(env, ContractError::ZeroAmountNotAllowed);
    }
}

#[contractimpl]
impl SplitContract {
    /// Deposit `amount` of `token` into `creator`'s liquidity pool.
    /// Returns the number of LP shares minted to `provider`.
    pub fn pool_deposit(
        env: Env,
        provider: Address,
        creator: Address,
        token: Address,
        amount: i128,
    ) -> i128 {
        require_not_paused(&env);
        provider.require_auth();
        require_positive(&env, amount);

        let mut pool = load_pool(&env, &creator, &token);
        let value = pool_value(&env, &pool);
        let shares = if pool.total_shares == 0 || value == 0 {
            amount
        } else {
            mul_div(&env, amount, pool.total_shares, value)
        };
        if shares <= 0 {
            panic_with_error!(&env, ContractError::InvalidAmount);
        }

        token::Client::new(&env, &token).transfer(&provider, &env.current_contract_address(), &amount);

        pool.available += amount;
        pool.total_shares += shares;
        save_pool(&env, &pool);
        let held = load_shares(&env, &creator, &token, &provider);
        save_shares(&env, &creator, &token, &provider, held + shares);

        events::pool_deposited(&env, &creator, &provider, &token, amount, shares);
        shares
    }

    /// Burn `shares` of `provider`'s stake in `creator`'s pool and return the
    /// underlying tokens. Returns the token amount paid out.
    pub fn pool_withdraw(
        env: Env,
        provider: Address,
        creator: Address,
        token: Address,
        shares: i128,
    ) -> i128 {
        require_not_paused(&env);
        provider.require_auth();
        require_positive(&env, shares);

        let held = load_shares(&env, &creator, &token, &provider);
        if held < shares {
            panic_with_error!(&env, ContractError::InsufficientPoolShares);
        }
        let mut pool = load_pool(&env, &creator, &token);
        let amount = mul_div(&env, shares, pool_value(&env, &pool), pool.total_shares);
        if amount > pool.available {
            panic_with_error!(&env, ContractError::PoolInsufficientLiquidity);
        }

        pool.available -= amount;
        pool.total_shares -= shares;
        save_pool(&env, &pool);
        save_shares(&env, &creator, &token, &provider, held - shares);

        if amount > 0 {
            token::Client::new(&env, &token).transfer(&env.current_contract_address(), &provider, &amount);
        }

        events::pool_withdrawn(&env, &creator, &provider, &token, amount, shares);
        amount
    }

    /// Creator draws `amount` of liquidity from their own pool.
    pub fn pool_draw(env: Env, creator: Address, token: Address, amount: i128) {
        require_not_paused(&env);
        creator.require_auth();
        require_positive(&env, amount);

        let mut pool = load_pool(&env, &creator, &token);
        if amount > pool.available {
            panic_with_error!(&env, ContractError::PoolInsufficientLiquidity);
        }
        pool.available -= amount;
        pool.outstanding += amount;
        save_pool(&env, &pool);

        token::Client::new(&env, &token).transfer(&env.current_contract_address(), &creator, &amount);

        events::pool_drawn(&env, &creator, &token, amount, pool.outstanding);
    }

    /// Creator repays `principal` of an outstanding draw plus `fee`.
    /// The fee is distributed to LPs implicitly by increasing pool value.
    pub fn pool_repay(env: Env, creator: Address, token: Address, principal: i128, fee: i128) {
        require_not_paused(&env);
        creator.require_auth();
        if principal < 0 || fee < 0 || principal + fee == 0 {
            panic_with_error!(&env, ContractError::InvalidAmount);
        }

        let mut pool = load_pool(&env, &creator, &token);
        if principal > pool.outstanding {
            panic_with_error!(&env, ContractError::RepaymentExceedsDebt);
        }
        if fee > 0 && pool.total_shares == 0 {
            // Nobody to receive the fee — reject rather than strand tokens.
            panic_with_error!(&env, ContractError::InvalidAmount);
        }
        let total = principal
            .checked_add(fee)
            .unwrap_or_else(|| panic_with_error!(&env, ContractError::ArithmeticOverflow));

        token::Client::new(&env, &token).transfer(&creator, &env.current_contract_address(), &total);

        pool.outstanding -= principal;
        pool.available += total;
        pool.fees_earned += fee;
        save_pool(&env, &pool);

        events::pool_repaid(&env, &creator, &token, principal, fee, pool.outstanding);
    }

    /// Current state of `creator`'s pool for `token`.
    pub fn get_creator_pool(env: Env, creator: Address, token: Address) -> CreatorPool {
        load_pool(&env, &creator, &token)
    }

    /// LP shares held by `provider` in `creator`'s pool.
    pub fn get_pool_shares(env: Env, creator: Address, token: Address, provider: Address) -> i128 {
        load_shares(&env, &creator, &token, &provider)
    }

    /// Token value of `provider`'s stake at the current share price.
    pub fn get_pool_position_value(
        env: Env,
        creator: Address,
        token: Address,
        provider: Address,
    ) -> i128 {
        let pool = load_pool(&env, &creator, &token);
        if pool.total_shares == 0 {
            return 0;
        }
        let shares = load_shares(&env, &creator, &token, &provider);
        mul_div(&env, shares, pool_value(&env, &pool), pool.total_shares)
    }
}
