use soroban_sdk::{Env, Address, Symbol, symbol_short, i128};

pub fn emit_initialized(env: &Env, admin: Address) {
    env.events().publish(
        (symbol_short!("carbon"), symbol_short!("init")),
        admin,
    );
}

pub fn emit_carbon_registered(env: &Env, invoice_id: Symbol, credits: i128) {
    env.events().publish(
        (symbol_short!("carbon"), symbol_short!("reg")),
        (invoice_id, credits),
    );
}

pub fn emit_invoice_linked(env: &Env, invoice_id: Symbol, wallet: Address, credits: i128) {
    env.events().publish(
        (symbol_short!("carbon"), symbol_short!("link")),
        (invoice_id, wallet, credits),
    );
}

pub fn emit_carbon_verified(env: &Env, invoice_id: Symbol) {
    env.events().publish(
        (symbol_short!("carbon"), symbol_short!("verify")),
        invoice_id,
    );
}

pub fn emit_credits_redeemed(env: &Env, wallet: Address, amount: i128) {
    env.events().publish(
        (symbol_short!("carbon"), symbol_short!("redeem")),
        (wallet, amount),
    );
}
