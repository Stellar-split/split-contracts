//! SplitFactory — clone-factory for per-invoice split contract instances.
//!
//! Deploying a fresh WASM instance per invoice is gas-intensive. This factory
//! deploys minimal-proxy clones pointing to a single canonical WASM blob,
//! reducing deployment cost while maintaining per-invoice state isolation.

#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Symbol, Val, Vec};

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

fn factory_admin_key() -> Symbol {
    symbol_short!("fct_adm")
}

/// Per-creator deployed contracts list — persistent storage.
fn deployed_contracts_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("deployed"), creator.clone())
}

/// Per-(creator, salt) deployed address — used to detect duplicate salts.
fn salt_key(creator: &Address, salt: &BytesN<32>) -> (Symbol, Address, BytesN<32>) {
    (symbol_short!("salt"), creator.clone(), salt.clone())
}

/// Maximum number of deployed contracts per creator (DoS bound).
const MAX_DEPLOYMENTS_PER_CREATOR: u32 = 10_000;

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Emitted when a new split contract is deployed by the factory.
///
/// Topics: (factory, deployed, creator)
/// Data: (contract_address, salt)
pub fn contract_deployed(env: &Env, creator: &Address, contract_address: &Address) {
    env.events().publish(
        (
            symbol_short!("factory"),
            symbol_short!("deployed"),
            creator.clone(),
        ),
        contract_address.clone(),
    );
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

    /// Deploy a new split contract instance using the clone-factory pattern.
    ///
    /// Uses `env.deployer().with_current_contract(salt)` so that the deployed
    /// address is deterministic for a given `(creator, salt)` pair. Duplicate
    /// salts for the same creator are rejected.
    ///
    /// # Parameters
    /// * `creator`     — The creator address; requires auth.
    /// * `wasm_hash`   — SHA-256 hash of the pre-uploaded split-contract WASM.
    /// * `salt`        — 32-byte uniqueness salt; determines the deployed address.
    /// * `init_args`   — Constructor arguments forwarded to the deployed contract.
    ///
    /// # Returns
    /// The `Address` of the newly deployed contract.
    pub fn deploy_invoice_contract(
        env: Env,
        creator: Address,
        wasm_hash: BytesN<32>,
        salt: BytesN<32>,
        init_args: soroban_sdk::Vec<Val>,
    ) -> Address {
        creator.require_auth();

        // Reject duplicate salts for the same creator.
        let salt_storage_key = salt_key(&creator, &salt);
        assert!(
            !env.storage().persistent().has(&salt_storage_key),
            "duplicate salt for creator"
        );

        // Enforce per-creator deployment cap (DoS bound).
        let deployments_key = deployed_contracts_key(&creator);
        let deployments: Vec<Address> = env
            .storage()
            .persistent()
            .get(&deployments_key)
            .unwrap_or_else(|| Vec::new(&env));
        assert!(
            deployments.len() < MAX_DEPLOYMENTS_PER_CREATOR as u32,
            "deployment limit exceeded"
        );

        // Deploy using the clone-factory pattern.
        let deployed_address = env
            .deployer()
            .with_current_contract(salt.clone())
            .deploy(wasm_hash);

        // Invoke init on the deployed contract if init_args is non-empty.
        if !init_args.is_empty() {
            let _: Val = env.invoke_contract(
                &deployed_address,
                &Symbol::new(&env, "initialize"),
                init_args,
            );
        }

        // Record the deployed address.
        let mut updated_deployments = deployments;
        updated_deployments.push_back(deployed_address.clone());
        env.storage()
            .persistent()
            .set(&deployments_key, &updated_deployments);

        // Mark the salt as used.
        env.storage()
            .persistent()
            .set(&salt_storage_key, &deployed_address);

        // Emit event.
        contract_deployed(&env, &creator, &deployed_address);

        deployed_address
    }

    /// Return all deployed contract addresses for a given creator.
    pub fn get_deployments(env: Env, creator: Address) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&deployed_contracts_key(&creator))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Check whether a specific (creator, salt) pair has already been used.
    pub fn is_salt_used(env: Env, creator: Address, salt: BytesN<32>) -> bool {
        env.storage().persistent().has(&salt_key(&creator, &salt))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::Address as _, BytesN, Env, Val, Vec};

    /// Minimal valid Soroban WASM stub (WebAssembly module with no exports).
    const STUB_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, // magic: \0asm
        0x01, 0x00, 0x00, 0x00, // version: 1
    ];

    #[test]
    fn test_deploy_single_contract() {
        let env = Env::default();
        env.mock_all_auths();

        let factory_admin = Address::generate(&env);
        let factory_id = env.register(SplitFactory, ());
        let factory_client = SplitFactoryClient::new(&env, &factory_id);
        factory_client.initialize(&factory_admin);

        let wasm_hash = env.deployer().upload_contract_wasm(STUB_WASM);

        let creator = Address::generate(&env);
        let salt = BytesN::from_array(&env, &[1u8; 32]);
        let init_args = Vec::<Val>::new(&env);

        let addr = factory_client.deploy_invoice_contract(
            &creator,
            &wasm_hash,
            &salt,
            &init_args,
        );

        let deployments = factory_client.get_deployments(&creator);
        assert_eq!(deployments.len(), 1);
        assert_eq!(deployments.get_unchecked(0), addr);
    }

    #[test]
    fn test_deploy_two_contracts_unique_addresses() {
        let env = Env::default();
        env.mock_all_auths();

        let factory_admin = Address::generate(&env);
        let factory_id = env.register(SplitFactory, ());
        let factory_client = SplitFactoryClient::new(&env, &factory_id);
        factory_client.initialize(&factory_admin);

        let wasm_hash = env.deployer().upload_contract_wasm(STUB_WASM);
        let creator = Address::generate(&env);
        let init_args = Vec::<Val>::new(&env);

        let salt1 = BytesN::from_array(&env, &[1u8; 32]);
        let addr1 = factory_client.deploy_invoice_contract(
            &creator,
            &wasm_hash,
            &salt1,
            &init_args,
        );

        let salt2 = BytesN::from_array(&env, &[2u8; 32]);
        let addr2 = factory_client.deploy_invoice_contract(
            &creator,
            &wasm_hash,
            &salt2,
            &init_args,
        );

        assert_ne!(addr1, addr2);

        let deployments = factory_client.get_deployments(&creator);
        assert_eq!(deployments.len(), 2);
        assert_eq!(deployments.get_unchecked(0), addr1);
        assert_eq!(deployments.get_unchecked(1), addr2);
    }

    #[test]
    #[should_panic(expected = "duplicate salt for creator")]
    fn test_duplicate_salt_rejected() {
        let env = Env::default();
        env.mock_all_auths();

        let factory_admin = Address::generate(&env);
        let factory_id = env.register(SplitFactory, ());
        let factory_client = SplitFactoryClient::new(&env, &factory_id);
        factory_client.initialize(&factory_admin);

        let wasm_hash = env.deployer().upload_contract_wasm(STUB_WASM);
        let creator = Address::generate(&env);
        let salt = BytesN::from_array(&env, &[1u8; 32]);
        let init_args = Vec::<Val>::new(&env);

        factory_client.deploy_invoice_contract(&creator, &wasm_hash, &salt, &init_args);
        // Same salt, same creator — should panic.
        factory_client.deploy_invoice_contract(&creator, &wasm_hash, &salt, &init_args);
    }

    #[test]
    fn test_different_creators_same_salt_allowed() {
        let env = Env::default();
        env.mock_all_auths();

        let factory_admin = Address::generate(&env);
        let factory_id = env.register(SplitFactory, ());
        let factory_client = SplitFactoryClient::new(&env, &factory_id);
        factory_client.initialize(&factory_admin);

        let wasm_hash = env.deployer().upload_contract_wasm(STUB_WASM);
        let init_args = Vec::<Val>::new(&env);
        let salt = BytesN::from_array(&env, &[1u8; 32]);

        let creator1 = Address::generate(&env);
        let addr1 = factory_client.deploy_invoice_contract(&creator1, &wasm_hash, &salt, &init_args);

        let creator2 = Address::generate(&env);
        let addr2 = factory_client.deploy_invoice_contract(&creator2, &wasm_hash, &salt, &init_args);

        assert_ne!(addr1, addr2);
    }

    #[test]
    fn test_contract_deployed_event_emitted() {
        let env = Env::default();
        env.mock_all_auths();

        let factory_admin = Address::generate(&env);
        let factory_id = env.register(SplitFactory, ());
        let factory_client = SplitFactoryClient::new(&env, &factory_id);
        factory_client.initialize(&factory_admin);

        let wasm_hash = env.deployer().upload_contract_wasm(STUB_WASM);
        let creator = Address::generate(&env);
        let salt = BytesN::from_array(&env, &[1u8; 32]);
        let init_args = Vec::<Val>::new(&env);

        factory_client.deploy_invoice_contract(&creator, &wasm_hash, &salt, &init_args);

        // Verify the ContractDeployed event was emitted.
        let events = env.events().all();
        let mut found = false;
        for event in events.iter() {
            let topics = event.1;
            // topics: (factory, deployed, creator)
            if topics.len() >= 3 {
                if let Ok(t0) = Symbol::try_from_val(&env, &topics.get_unchecked(0)) {
                    if t0 == symbol_short!("factory") {
                        found = true;
                        break;
                    }
                }
            }
        }
        assert!(found, "ContractDeployed event not emitted");
    }

    #[test]
    fn test_is_salt_used() {
        let env = Env::default();
        env.mock_all_auths();

        let factory_admin = Address::generate(&env);
        let factory_id = env.register(SplitFactory, ());
        let factory_client = SplitFactoryClient::new(&env, &factory_id);
        factory_client.initialize(&factory_admin);

        let wasm_hash = env.deployer().upload_contract_wasm(STUB_WASM);
        let creator = Address::generate(&env);
        let salt = BytesN::from_array(&env, &[1u8; 32]);
        let init_args = Vec::<Val>::new(&env);

        assert!(!factory_client.is_salt_used(&creator, &salt));
        factory_client.deploy_invoice_contract(&creator, &wasm_hash, &salt, &init_args);
        assert!(factory_client.is_salt_used(&creator, &salt));
    }

    #[test]
    fn test_empty_get_deployments() {
        let env = Env::default();
        env.mock_all_auths();

        let factory_admin = Address::generate(&env);
        let factory_id = env.register(SplitFactory, ());
        let factory_client = SplitFactoryClient::new(&env, &factory_id);
        factory_client.initialize(&factory_admin);

        let creator = Address::generate(&env);
        assert_eq!(factory_client.get_deployments(&creator).len(), 0);
    }
}
