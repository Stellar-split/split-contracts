//! Issue #774: multi-token protocol-fee treasury with per-token accounting.
//!
//! Protocol fees (issue #326) are no longer pushed to the treasury address at
//! release time. They stay in the contract and are credited to a per-token
//! balance; the admin withdraws each token independently to the treasury
//! address configured via `set_protocol_fee`. There was no prior single
//! `treasury_balance: i128` in storage to replace, so nothing is migrated.

use soroban_sdk::{contractimpl, contracttype, symbol_short, token, Address, Env, Symbol, Vec};

use crate::{protocol_fee_key, types::ProtocolFeeConfig, SplitContract};

#[contracttype]
#[derive(Clone)]
pub enum TreasuryKey {
    /// (token) -> i128 accrued protocol fees held by the contract
    Balance(Address),
    /// Vec<Address> of every token that has ever accrued fees
    Tokens,
}

fn balance(env: &Env, token: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&TreasuryKey::Balance(token.clone()))
        .unwrap_or(0)
}

/// Credit `amount` of `token` to the treasury ledger.
pub(crate) fn credit(env: &Env, token: &Address, amount: i128) {
    let new_bal = balance(env, token).checked_add(amount).expect("ArithmeticOverflow");
    env.storage()
        .persistent()
        .set(&TreasuryKey::Balance(token.clone()), &new_bal);
    let mut tokens: Vec<Address> = env
        .storage()
        .persistent()
        .get(&TreasuryKey::Tokens)
        .unwrap_or_else(|| Vec::new(env));
    if !tokens.contains(token) {
        tokens.push_back(token.clone());
        env.storage().persistent().set(&TreasuryKey::Tokens, &tokens);
    }
}

#[contractimpl]
impl SplitContract {
    /// Issue #774: accrued treasury balance for `token`.
    pub fn get_treasury_balance(env: Env, token: Address) -> i128 {
        balance(&env, &token)
    }

    /// Issue #774: tokens with a non-zero treasury balance.
    pub fn list_treasury_tokens(env: Env) -> Vec<Address> {
        let all: Vec<Address> = env
            .storage()
            .persistent()
            .get(&TreasuryKey::Tokens)
            .unwrap_or_else(|| Vec::new(&env));
        let mut out = Vec::new(&env);
        for t in all.iter() {
            if balance(&env, &t) > 0 {
                out.push_back(t);
            }
        }
        out
    }

    /// Issue #774: withdraw `amount` of `token` to the configured treasury
    /// address. Admin only; blocked while the contract is frozen.
    pub fn withdraw_treasury(env: Env, admin: Address, token: Address, amount: i128) {
        crate::freeze_ext::require_not_frozen_global(&env);
        crate::freeze_ext::require_admin(&env, &admin);
        assert!(amount > 0, "InvalidAmount");
        let bal = balance(&env, &token);
        assert!(amount <= bal, "InsufficientTreasuryBalance");
        let cfg: ProtocolFeeConfig = env
            .storage()
            .instance()
            .get(&protocol_fee_key())
            .expect("protocol fee treasury not configured");
        env.storage()
            .persistent()
            .set(&TreasuryKey::Balance(token.clone()), &(bal - amount));
        token::Client::new(&env, &token).transfer(&env.current_contract_address(), &cfg.treasury, &amount);
        env.events().publish(
            (symbol_short!("split"), Symbol::new(&env, "TreasuryWithdrawn")),
            (token, amount, admin),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::ext_test_util::*;
    use soroban_sdk::{testutils::Address as _, token::Client as TokenClient, Address};

    #[test]
    fn independent_per_token_accounting_withdrawal_and_listing() {
        let f = fixture();
        let token_b = f
            .env
            .register_stellar_asset_contract_v2(Address::generate(&f.env))
            .address();
        let treasury = Address::generate(&f.env);
        f.c.set_protocol_fee(&f.admin, &500_u32, &treasury);

        let payer = Address::generate(&f.env);
        mint(&f.env, &f.token, &payer, 1_000);
        mint(&f.env, &token_b, &payer, 2_000);
        assert_eq!(f.c.list_treasury_tokens().len(), 0);

        let (a, _, _) = new_invoice_with_token(&f, &f.token, 1_000);
        pay(&f, &payer, a, 1_000);
        let (b, _, _) = new_invoice_with_token(&f, &token_b, 2_000);
        pay(&f, &payer, b, 2_000);

        assert_eq!(f.c.get_treasury_balance(&f.token), 50);
        assert_eq!(f.c.get_treasury_balance(&token_b), 100);
        assert_eq!(f.c.list_treasury_tokens().len(), 2);

        f.c.withdraw_treasury(&f.admin, &f.token, &50_i128);
        assert_eq!(TokenClient::new(&f.env, &f.token).balance(&treasury), 50);
        assert_eq!(f.c.get_treasury_balance(&f.token), 0);
        assert_eq!(f.c.get_treasury_balance(&token_b), 100);
        let listed = f.c.list_treasury_tokens();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed.get(0).unwrap(), token_b);

        f.c.withdraw_treasury(&f.admin, &token_b, &100_i128);
        assert_eq!(TokenClient::new(&f.env, &token_b).balance(&treasury), 100);
        assert_eq!(f.c.list_treasury_tokens().len(), 0);
    }

    #[test]
    fn over_withdrawal_fails() {
        let f = fixture();
        let treasury = Address::generate(&f.env);
        f.c.set_protocol_fee(&f.admin, &500_u32, &treasury);
        assert!(f.c.try_withdraw_treasury(&f.admin, &f.token, &1_i128).is_err());
    }
}
