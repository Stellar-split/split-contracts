#![no_std]

use soroban_sdk::{contract, contractimpl, symbol_short, Address, Env, Symbol, Vec, i128};

mod events;
mod storage;
mod types;

pub use events::*;
pub use storage::*;
pub use types::*;

#[contract]
pub struct CarbonCreditContract;

#[contractimpl]
impl CarbonCreditContract {
    pub fn initialize(env: Env, admin: Address) {
        admin.require_auth();
        storage::set_admin(&env, &admin);
        emit_initialized(&env, admin);
    }

    pub fn register_invoice_carbon(
        env: Env,
        invoice_id: Symbol,
        credits: i128,
        source: Symbol,
    ) -> Result<(), Error> {
        let admin: Address = storage::get_admin(&env)?;
        admin.require_auth();

        assert!(credits > 0, "credits must be positive");

        let record = CarbonRecord {
            invoice_id: invoice_id.clone(),
            credits,
            source,
            timestamp: env.ledger().timestamp(),
            verified: false,
        };

        storage::store_carbon_record(&env, &invoice_id, &record);
        emit_carbon_registered(&env, invoice_id, credits);

        Ok(())
    }

    pub fn link_invoice_credits(
        env: Env,
        invoice_id: Symbol,
        wallet: Address,
    ) -> Result<i128, Error> {
        let record = storage::get_carbon_record(&env, &invoice_id)
            .ok_or(Error::NotFound)?;

        storage::link_wallet_credits(&env, &wallet, record.credits);
        emit_invoice_linked(&env, invoice_id, wallet, record.credits);

        Ok(record.credits)
    }

    pub fn get_wallet_total_credits(env: Env, wallet: Address) -> i128 {
        storage::get_wallet_credits(&env, &wallet).unwrap_or(0)
    }

    pub fn verify_carbon_record(env: Env, invoice_id: Symbol) -> Result<(), Error> {
        let admin: Address = storage::get_admin(&env)?;
        admin.require_auth();

        let mut record = storage::get_carbon_record(&env, &invoice_id)
            .ok_or(Error::NotFound)?;

        record.verified = true;
        storage::store_carbon_record(&env, &invoice_id, &record);
        emit_carbon_verified(&env, invoice_id);

        Ok(())
    }

    pub fn get_carbon_record(env: Env, invoice_id: Symbol) -> Result<CarbonRecord, Error> {
        storage::get_carbon_record(&env, &invoice_id)
            .ok_or(Error::NotFound)
    }

    pub fn list_wallet_credits(env: Env, wallet: Address, limit: u32) -> Vec<CarbonRecord> {
        storage::list_wallet_records(&env, &wallet, limit)
    }

    pub fn redeem_credits(
        env: Env,
        wallet: Address,
        amount: i128,
    ) -> Result<(), Error> {
        wallet.require_auth();

        let current = storage::get_wallet_credits(&env, &wallet)
            .ok_or(Error::InsufficientFunds)?;

        assert!(amount <= current, "insufficient credits");

        let remaining = current - amount;
        storage::update_wallet_credits(&env, &wallet, remaining);
        emit_credits_redeemed(&env, wallet, amount);

        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum Error {
    NotFound,
    InsufficientFunds,
    Unauthorized,
    InvalidAmount,
}

impl From<Error> for soroban_sdk::Error {
    fn from(err: Error) -> Self {
        match err {
            Error::NotFound => soroban_sdk::Error::from_contract_error(1),
            Error::InsufficientFunds => soroban_sdk::Error::from_contract_error(2),
            Error::Unauthorized => soroban_sdk::Error::from_contract_error(3),
            Error::InvalidAmount => soroban_sdk::Error::from_contract_error(4),
        }
    }
}
