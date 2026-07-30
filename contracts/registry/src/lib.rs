//! StellarSplit Registry — on-chain index of all deployed split contract instances.
//!
//! Maintains a searchable map from creator address to their list of deployed
//! split contract addresses so frontends can discover active invoices.

#![no_std]

use soroban_sdk::{contract, contractimpl, symbol_short, Address, Env, Symbol, Vec};

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

fn registry_admin_key() -> Symbol {
    symbol_short!("reg_adm")
}

/// Per-creator list of registered contract addresses — persistent storage.
fn creator_contracts_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("cr_ctrs"), creator.clone())
}

/// Per-(creator, contract) flag to detect duplicate registrations — persistent.
fn registration_key(creator: &Address, contract: &Address) -> (Symbol, Address, Address) {
    (symbol_short!("reg_chk"), creator.clone(), contract.clone())
}

/// Maximum contracts per creator to bound storage growth and prevent DoS.
const MAX_CONTRACTS_PER_CREATOR: u32 = 5_000;

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Emitted when a contract is registered for a creator.
///
/// Topics: (registry, registered, creator)
/// Data: contract_address
pub fn contract_registered(env: &Env, creator: &Address, contract: &Address) {
    env.events().publish(
        (
            symbol_short!("registry"),
            symbol_short!("registered"),
            creator.clone(),
        ),
        contract.clone(),
    );
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct Registry;

#[contractimpl]
impl Registry {
    /// Initialise the registry by recording its admin.
    /// Can only be called once.
    pub fn initialize(env: Env, admin: Address) {
        assert!(
            !env.storage().instance().has(&registry_admin_key()),
            "already initialized"
        );
        env.storage().instance().set(&registry_admin_key(), &admin);
    }

    /// Register a deployed contract address under a creator.
    ///
    /// Appends `contract` to the creator's list. Rejects duplicate
    /// registrations for the same (creator, contract) pair.
    ///
    /// # Panics
    /// * `"registry limit exceeded"` — the creator already has
    ///   [`MAX_CONTRACTS_PER_CREATOR`] registered contracts.
    /// * `"already registered"` — this contract address is already
    ///   in the creator's list.
    pub fn register(env: Env, creator: Address, contract: Address) {
        creator.require_auth();

        let contracts_key = creator_contracts_key(&creator);
        let registration_check_key = registration_key(&creator, &contract);

        // Reject duplicates.
        assert!(
            !env.storage().persistent().has(&registration_check_key),
            "already registered"
        );

        // Enforce per-creator limit.
        let mut contracts: Vec<Address> = env
            .storage()
            .persistent()
            .get(&contracts_key)
            .unwrap_or_else(|| Vec::new(&env));
        assert!(
            contracts.len() < MAX_CONTRACTS_PER_CREATOR as u32,
            "registry limit exceeded"
        );

        // Append and persist.
        contracts.push_back(contract.clone());
        env.storage()
            .persistent()
            .set(&contracts_key, &contracts);

        // Mark as registered.
        env.storage()
            .persistent()
            .set(&registration_check_key, &true);

        // Emit event.
        contract_registered(&env, &creator, &contract);
    }

    /// Return all registered contract addresses for a creator.
    pub fn get_contracts(env: Env, creator: Address) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&creator_contracts_key(&creator))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Return the number of registered contracts for a creator.
    pub fn get_contract_count(env: Env, creator: Address) -> u32 {
        let contracts: Vec<Address> = env
            .storage()
            .persistent()
            .get(&creator_contracts_key(&creator))
            .unwrap_or_else(|| Vec::new(&env));
        contracts.len()
    }

    /// Check whether a specific (creator, contract) pair is already registered.
    pub fn is_registered(env: Env, creator: Address, contract: Address) -> bool {
        env.storage()
            .persistent()
            .has(&registration_key(&creator, &contract))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Events as _},
        Address, Env, Symbol, Vec,
    };

    #[test]
    fn test_register_and_get_contracts() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let registry_id = env.register(Registry, ());
        let registry_client = RegistryClient::new(&env, &registry_id);
        registry_client.initialize(&admin);

        let creator = Address::generate(&env);
        let contract_addr = Address::generate(&env);

        registry_client.register(&creator, &contract_addr);

        let contracts = registry_client.get_contracts(&creator);
        assert_eq!(contracts.len(), 1);
        assert_eq!(contracts.get_unchecked(0), contract_addr);
        assert_eq!(registry_client.get_contract_count(&creator), 1);
    }

    #[test]
    fn test_multiple_creators() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let registry_id = env.register(Registry, ());
        let registry_client = RegistryClient::new(&env, &registry_id);
        registry_client.initialize(&admin);

        let c1 = Address::generate(&env);
        let c2 = Address::generate(&env);
        let addr1 = Address::generate(&env);
        let addr2 = Address::generate(&env);
        let addr3 = Address::generate(&env);

        registry_client.register(&c1, &addr1);
        registry_client.register(&c1, &addr2);
        registry_client.register(&c2, &addr3);

        let c1_contracts = registry_client.get_contracts(&c1);
        assert_eq!(c1_contracts.len(), 2);
        assert_eq!(c1_contracts.get_unchecked(0), addr1);
        assert_eq!(c1_contracts.get_unchecked(1), addr2);

        let c2_contracts = registry_client.get_contracts(&c2);
        assert_eq!(c2_contracts.len(), 1);
        assert_eq!(c2_contracts.get_unchecked(0), addr3);
    }

    #[test]
    #[should_panic(expected = "already registered")]
    fn test_duplicate_registration_rejected() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let registry_id = env.register(Registry, ());
        let registry_client = RegistryClient::new(&env, &registry_id);
        registry_client.initialize(&admin);

        let creator = Address::generate(&env);
        let contract_addr = Address::generate(&env);

        registry_client.register(&creator, &contract_addr);
        registry_client.register(&creator, &contract_addr); // should panic
    }

    #[test]
    fn test_contract_registered_event_emitted() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let registry_id = env.register(Registry, ());
        let registry_client = RegistryClient::new(&env, &registry_id);
        registry_client.initialize(&admin);

        let creator = Address::generate(&env);
        let contract_addr = Address::generate(&env);

        registry_client.register(&creator, &contract_addr);

        let events = env.events().all();
        let mut found = false;
        for event in events.iter() {
            let topics = event.1;
            if topics.len() >= 3 {
                if let Ok(t0) = Symbol::try_from_val(&env, &topics.get_unchecked(0)) {
                    if t0 == symbol_short!("registry") {
                        found = true;
                        break;
                    }
                }
            }
        }
        assert!(found, "ContractRegistered event not emitted");
    }

    #[test]
    fn test_empty_get_contracts() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let registry_id = env.register(Registry, ());
        let registry_client = RegistryClient::new(&env, &registry_id);
        registry_client.initialize(&admin);

        let creator = Address::generate(&env);
        assert_eq!(registry_client.get_contracts(&creator).len(), 0);
        assert_eq!(registry_client.get_contract_count(&creator), 0);
    }

    #[test]
    fn test_is_registered() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let registry_id = env.register(Registry, ());
        let registry_client = RegistryClient::new(&env, &registry_id);
        registry_client.initialize(&admin);

        let creator = Address::generate(&env);
        let contract_addr = Address::generate(&env);

        assert!(!registry_client.is_registered(&creator, &contract_addr));
        registry_client.register(&creator, &contract_addr);
        assert!(registry_client.is_registered(&creator, &contract_addr));
    }
}
