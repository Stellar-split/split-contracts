//! Issue #178: SplitFactory — deploy and registry for SplitContract instances.
//!
//! The factory deploys new `SplitContract` instances with shared configuration
//! and maintains a persistent registry of all deployed contract addresses.

#![allow(dead_code)]

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Symbol, Vec};

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

fn factory_admin_key() -> Symbol {
    symbol_short!("fct_adm")
}

fn deployments_key() -> Symbol {
    symbol_short!("deploys")
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Shared configuration passed to every deployed `SplitContract` instance.
#[contracttype]
#[derive(Clone, Debug)]
pub struct FactoryConfig {
    /// Initial admin for the newly deployed SplitContract.
    pub admin: Address,
    /// USDC (or base) token address.
    pub usdc_token: Address,
    /// Treasury address that receives platform fees.
    pub treasury: Address,
    /// Per-invoice creation fee in token units.
    pub creation_fee: i128,
    /// Platform fee in basis points (0–10 000).
    pub platform_fee_bps: u32,
    /// Optional governance contract address.
    pub governance_contract: Option<Address>,
    /// Maximum cancellation rate in basis points.
    pub max_cancel_bps: u32,
    /// Maximum invoice creations per `rate_window`.
    pub rate_limit: u32,
    /// Rate-limit window in seconds.
    pub rate_window: u64,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct SplitFactory;

#[contractimpl]
impl SplitFactory {
    /// Initialise the factory by recording its admin.
    /// Can only be called once.
    pub fn initialize(env: Env, admin: Address) {
        assert!(
            !env.storage().instance().has(&factory_admin_key()),
            "already initialized"
        );
        env.storage().instance().set(&factory_admin_key(), &admin);
    }

    /// Deploy a new `SplitContract` instance with the supplied configuration.
    ///
    /// Requires factory-admin authentication.
    ///
    /// * `wasm_hash`  — SHA-256 hash of the previously uploaded `SplitContract` WASM.
    /// * `salt`       — 32-byte uniqueness salt; determines the deployed address.
    /// * `config`     — Shared configuration forwarded to `SplitContract::initialize`.
    ///
    /// Returns the `Address` of the newly deployed contract, which is also
    /// appended to the persistent deployments registry.
    pub fn deploy(
        env: Env,
        wasm_hash: BytesN<32>,
        salt: BytesN<32>,
        config: FactoryConfig,
    ) -> Address {
        // Require factory admin auth.
        let admin: Address = env
            .storage()
            .instance()
            .get(&factory_admin_key())
            .expect("factory not initialized");
        admin.require_auth();

        // Deploy the SplitContract WASM, passing constructor arguments that
        // map to SplitContract::initialize.
        let init_args = (
            config.admin.clone(),
            config.creation_fee,
            config.treasury.clone(),
            config.usdc_token.clone(),
            config.platform_fee_bps,
            config.governance_contract.clone(),
            config.max_cancel_bps,
            config.rate_limit,
            config.rate_window,
        );

        let deployed_address = env
            .deployer()
            .with_address(env.current_contract_address(), salt)
            .deploy_v2(wasm_hash, init_args);

        // Append to registry.
        let mut deployments: Vec<Address> = env
            .storage()
            .persistent()
            .get(&deployments_key())
            .unwrap_or_else(|| Vec::new(&env));
        deployments.push_back(deployed_address.clone());
        env.storage()
            .persistent()
            .set(&deployments_key(), &deployments);

        deployed_address
    }

    /// Return all deployed SplitContract addresses in the order they were deployed.
    pub fn get_deployments(env: Env) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&deployments_key())
            .unwrap_or_else(|| Vec::new(&env))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::Address as _, BytesN, Env};

    fn make_config(env: &Env) -> FactoryConfig {
        let token_admin = soroban_sdk::Address::generate(env);
        let token = env
            .register_stellar_asset_contract_v2(token_admin)
            .address();
        let treasury = soroban_sdk::Address::generate(env);
        let admin = soroban_sdk::Address::generate(env);
        FactoryConfig {
            admin,
            usdc_token: token,
            treasury,
            creation_fee: 0,
            platform_fee_bps: 0,
            governance_contract: None,
            max_cancel_bps: 5_000,
            rate_limit: 0,
            rate_window: 0,
        }
    }

    /// Minimal valid Soroban WASM (WebAssembly module with no exports).
    /// Used as a stand-in for the SplitContract WASM to test the factory
    /// registry without requiring a pre-compiled artifact.
    const STUB_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, // magic: \0asm
        0x01, 0x00, 0x00, 0x00, // version: 1
    ];

    #[test]
    fn test_factory_deploy_two_contracts_registry() {
        let env = Env::default();
        env.mock_all_auths();

        // Upload stub WASM — stand-in for the SplitContract WASM.
        let wasm_hash = env
            .deployer()
            .upload_contract_wasm(STUB_WASM);

        // Register and initialise the factory.
        let factory_admin = soroban_sdk::Address::generate(&env);
        let factory_id = env.register(SplitFactory, ());
        let factory_client = SplitFactoryClient::new(&env, &factory_id);
        factory_client.initialize(&factory_admin);

        // Deploy first instance.
        let salt1 = BytesN::from_array(&env, &[1u8; 32]);
        let config1 = make_config(&env);
        let addr1 = factory_client.deploy(&wasm_hash, &salt1, &config1);

        // Deploy second instance with a different salt.
        let salt2 = BytesN::from_array(&env, &[2u8; 32]);
        let config2 = make_config(&env);
        let addr2 = factory_client.deploy(&wasm_hash, &salt2, &config2);

        // Both addresses must differ.
        assert_ne!(addr1, addr2);

        // Registry must contain exactly the two deployed addresses, in order.
        let deployments = factory_client.get_deployments();
        assert_eq!(deployments.len(), 2);
        assert_eq!(deployments.get_unchecked(0), addr1);
        assert_eq!(deployments.get_unchecked(1), addr2);
    }

    #[test]
    fn test_factory_get_deployments_empty_before_any_deploy() {
        let env = Env::default();
        env.mock_all_auths();

        let factory_admin = soroban_sdk::Address::generate(&env);
        let factory_id = env.register(SplitFactory, ());
        let factory_client = SplitFactoryClient::new(&env, &factory_id);
        factory_client.initialize(&factory_admin);

        assert_eq!(factory_client.get_deployments().len(), 0);
    }
}
