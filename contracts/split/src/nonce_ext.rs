//! Issue #778: replay-protection nonces stored in temporary storage.

use crate::error::ContractError;
use soroban_sdk::{contracttype, symbol_short, Address, BytesN, Env};

/// Temporary-storage TTL (ledgers) of a consumed nonce.
pub const NONCE_TTL_LEDGERS: u32 = 1;

#[contracttype]
#[derive(Clone)]
pub enum NonceKey {
    Used(BytesN<32>),
}

pub fn is_used(env: &Env, nonce: &BytesN<32>) -> bool {
    env.storage().temporary().has(&NonceKey::Used(nonce.clone()))
}

pub fn check_unused(env: &Env, nonce: &BytesN<32>) {
    if is_used(env, nonce) {
        env.panic_with_error(ContractError::NonceAlreadyUsed);
    }
}

pub fn consume(env: &Env, invoice_id: u64, payer: &Address, nonce: &BytesN<32>) {
    let key = NonceKey::Used(nonce.clone());
    env.storage().temporary().set(&key, &true);
    env.storage()
        .temporary()
        .extend_ttl(&key, NONCE_TTL_LEDGERS, NONCE_TTL_LEDGERS);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("nonce"), invoice_id),
        (payer.clone(), nonce.clone()),
    );
}
