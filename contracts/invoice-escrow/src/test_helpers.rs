//! Shared fixtures for the feature test modules.

use crate::{InvoiceEscrowContract, InvoiceEscrowContractClient};
use soroban_sdk::testutils::{Address as _, Events, Ledger};
use soroban_sdk::{token, Address, Env, IntoVal, Symbol, Val, Vec};

pub struct Ctx<'a> {
    pub env: Env,
    pub client: InvoiceEscrowContractClient<'a>,
    pub admin: Address,
    pub token: Address,
}

impl Ctx<'_> {
    pub fn mint(&self, to: &Address, amount: i128) {
        token::StellarAssetClient::new(&self.env, &self.token).mint(to, &amount);
    }

    pub fn balance(&self, who: &Address) -> i128 {
        token::Client::new(&self.env, &self.token).balance(who)
    }

    pub fn contract_balance(&self) -> i128 {
        self.balance(&self.client.address)
    }

    pub fn user(&self) -> Address {
        Address::generate(&self.env)
    }

    /// A user pre-funded with `amount` tokens.
    pub fn funded_user(&self, amount: i128) -> Address {
        let who = self.user();
        self.mint(&who, amount);
        who
    }

    pub fn set_time(&self, timestamp: u64) {
        self.env.ledger().with_mut(|l| l.timestamp = timestamp);
    }

    /// Create an escrow invoice owned by `creator` with deadline 1 000.
    pub fn invoice(&self, creator: &Address, total: i128) -> u64 {
        self.client
            .create_invoice(creator, &self.token, &total, &1_000u64)
    }

    /// `true` if the most recent invocation emitted an event with exactly
    /// the topics `(a, b, id)`.
    pub fn has_event(&self, a: Symbol, b: Symbol, id: u64) -> bool {
        let expected: Vec<Val> = (a, b, id).into_val(&self.env);
        self.env
            .events()
            .all()
            .iter()
            .any(|(_, topics, _)| topics == expected)
    }

    /// `true` if the most recent invocation emitted an event with exactly
    /// the two topics `(a, b)`.
    pub fn has_event2(&self, a: Symbol, b: Symbol) -> bool {
        let expected: Vec<Val> = (a, b).into_val(&self.env);
        self.env
            .events()
            .all()
            .iter()
            .any(|(_, topics, _)| topics == expected)
    }
}

/// Register and initialize the contract with a Stellar asset token.
pub fn setup<'a>() -> Ctx<'a> {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(InvoiceEscrowContract, ());
    let client = InvoiceEscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    Ctx {
        env,
        client,
        admin,
        token,
    }
}
