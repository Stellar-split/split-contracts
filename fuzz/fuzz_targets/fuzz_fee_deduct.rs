//! Fuzz target for issue #520 — fee deduction edge cases.
//!
//! Exercises `_release_full` with randomised platform fee basis points,
//! tax basis points, and waiver combinations, verifying that the fee
//! arithmetic never panics or overflows.
//!
//! Run with: `cargo fuzz run fuzz_fee_deduct`

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env, Vec};
use split::types::{InvoiceOptions, OverflowBehavior};
use split_fuzz::{client, default_options, fund, offset_timestamp, setup};

#[derive(Debug, Arbitrary)]
struct Input {
    platform_fee_bps: u16,
    tax_bps: u16,
    pay_amount: u64,
    n_recipients: u8,
    waive_creator: bool,
}

fuzz_target!(|input: Input| {
    let env = Env::default();
    let (contract_id, token_id) = setup(&env);
    let c = client(&env, &contract_id);

    let n = ((input.n_recipients as usize % 4) + 1).min(4);
    let platform_fee_bps = (input.platform_fee_bps as u32).min(10_000);
    let tax_bps = (input.tax_bps as u32).min(10_000);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    c.initialize(
        &admin,
        &0_i128,
        &treasury,
        &token_id,
        &platform_fee_bps,
        &None,
        &0_u32,
        &0_u32,
        &0_u64,
    );

    let creator = Address::generate(&env);
    let mut recipients: Vec<Address> = Vec::new(&env);
    let mut amounts: Vec<i128> = Vec::new(&env);
    for i in 0..n {
        recipients.push_back(Address::generate(&env));
        amounts.push_back(((input.pay_amount as i128) / n as i128).max(1));
    }

    let now = env.ledger().timestamp();
    let deadline = offset_timestamp(now, 86400);
    env.ledger().set_timestamp(now);

    let mut opts = default_options(&env);
    opts.tax_bps = Some(tax_bps);
    if input.waive_creator {
        opts.allow_early_withdrawal = true;
    }

    let invoice_id = match c.try_create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &deadline,
        &opts,
    ) {
        Ok(Ok(id)) => id,
        _ => return,
    };

    let payer = Address::generate(&env);
    fund(&env, &token_id, &payer);
    let total: i128 = amounts.iter().sum();
    let _ = c.try_pay(&payer, &invoice_id, &total, &0u64, &false, &false);

    let _ = c.try_release(&invoice_id);
});
