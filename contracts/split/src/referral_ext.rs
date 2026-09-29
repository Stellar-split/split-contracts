//! Issue #766: referral rewards for invoice payments.
//!
//! The creator opts an invoice in with `set_referral_fee_bps` (max 1000 = 10%).
//! A payer using `pay_with_referrer` pays the normal `amount` (credited to the
//! invoice exactly as in `pay`) plus a reward of `amount * bps / 10000`, which
//! the contract holds in the referrer's claimable balance until
//! `claim_referral_rewards`.

use super::*;
use soroban_sdk::{contractimpl, symbol_short, token, Address, Env, Symbol};

const MAX_REFERRAL_FEE_BPS: u32 = 1_000;

fn fee_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("ref_fee"), id)
}

fn referral_balance_key(referrer: &Address) -> (Symbol, Address) {
    (symbol_short!("ref_bal"), referrer.clone())
}

fn referral_token_key(referrer: &Address) -> (Symbol, Address) {
    (symbol_short!("ref_tok"), referrer.clone())
}

#[contractimpl]
impl SplitContract {
    /// Configure the referral fee for an invoice (creator only, before any payment).
    pub fn set_referral_fee_bps(env: Env, creator: Address, invoice_id: u64, fee_bps: u32) {
        require_not_paused(&env);
        creator.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        assert!(invoice.creator == creator, "only creator can set referral fee");
        assert!(invoice.funded == 0, "cannot change referral fee after payment");
        assert!(fee_bps <= MAX_REFERRAL_FEE_BPS, "referral_fee_bps exceeds 1000");
        env.storage().persistent().set(&fee_key(invoice_id), &fee_bps);
    }

    pub fn get_referral_fee_bps(env: Env, invoice_id: u64) -> u32 {
        env.storage().persistent().get(&fee_key(invoice_id)).unwrap_or(0)
    }

    /// Like `pay`, optionally crediting a referrer. With `referrer == None`
    /// (or a zero fee) no reward is charged.
    pub fn pay_with_referrer(
        env: Env,
        payer: Address,
        invoice_id: u64,
        amount: i128,
        nonce: u64,
        referrer: Option<Address>,
    ) {
        require_fn_not_paused(&env, &symbol_short!("pay"));
        require_not_frozen(&env);
        payer.require_auth();
        Self::enforce_invoice_rate_limit(&env, invoice_id, &payer);
        Self::_pay(&env, &payer, invoice_id, amount, nonce, false, None, None, false);

        let Some(referrer) = referrer else { return };
        assert!(referrer != payer, "cannot refer yourself");
        let bps: u32 = env.storage().persistent().get(&fee_key(invoice_id)).unwrap_or(0);
        let reward = amount * bps as i128 / 10_000;
        if reward <= 0 {
            return;
        }
        let invoice = load_invoice(&env, invoice_id);
        let tok = funding_token_for(&invoice);
        let bal_key = referral_balance_key(&referrer);
        let tok_key = referral_token_key(&referrer);
        let bal: i128 = env.storage().persistent().get(&bal_key).unwrap_or(0);
        if bal > 0 {
            let stored: Address = env.storage().persistent().get(&tok_key).expect("token");
            assert!(stored == tok, "ReferralTokenMismatch");
        }
        token::Client::new(&env, &tok).transfer(&payer, &env.current_contract_address(), &reward);
        env.storage().persistent().set(&bal_key, &(bal + reward));
        env.storage().persistent().set(&tok_key, &tok);
        env.events().publish(
            (symbol_short!("split"), symbol_short!("ref_rwd"), invoice_id),
            (payer, referrer, reward),
        );
    }

    /// Transfer all accumulated referral rewards to `referrer`; returns the amount.
    pub fn claim_referral_rewards(env: Env, referrer: Address) -> i128 {
        require_not_frozen(&env);
        referrer.require_auth();
        let bal_key = referral_balance_key(&referrer);
        let amount: i128 = env.storage().persistent().get(&bal_key).unwrap_or(0);
        assert!(amount > 0, "no referral rewards");
        let tok: Address = env
            .storage()
            .persistent()
            .get(&referral_token_key(&referrer))
            .expect("token");
        env.storage().persistent().set(&bal_key, &0i128);
        token::Client::new(&env, &tok).transfer(&env.current_contract_address(), &referrer, &amount);
        env.events().publish(
            (symbol_short!("split"), symbol_short!("ref_clm")),
            (referrer, amount),
        );
        amount
    }

    pub fn get_referral_balance(env: Env, referrer: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&referral_balance_key(&referrer))
            .unwrap_or(0)
    }
}
