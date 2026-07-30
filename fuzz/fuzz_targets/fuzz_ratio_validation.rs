//! Fuzz target for issue #520 — ratio validation and high-precision splits.
//!
//! Exercises `_create_invoice` and `_release_full` with randomised denominators
//! and ratio combinations, verifying that the ratio validation and proportional
//! payout arithmetic never panics or overflows.
//!
//! Run with: `cargo fuzz run fuzz_ratio_validation`

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env, Vec};
use split::types::{InvoiceOptions, OverflowBehavior};
use split_fuzz::{client, default_options, fund, offset_timestamp, setup};

#[derive(Debug, Arbitrary)]
struct Input {
    denominator: u64,
    n_recipients: u8,
    pay_amount: u64,
}

fuzz_target!(|input: Input| {
    let env = Env::default();
    let (contract_id, token_id) = setup(&env);
    let c = client(&env, &contract_id);

    let n = ((input.n_recipients as usize % 4) + 1).min(4);
    let denominator = input.denominator.max(2).min(1_000_000);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    c.initialize(
        &admin,
        &0_i128,
        &treasury,
        &token_id,
        &0_u32,
        &None,
        &0_u32,
        &0_u32,
        &0_u64,
    );

    let creator = Address::generate(&env);
    let mut recipients: Vec<Address> = Vec::new(&env);
    let mut amounts: Vec<i128> = Vec::new(&env);
    let mut ratios: Vec<u32> = Vec::new(&env);

    let base_amount = ((input.pay_amount as i128) / n as i128).max(1);
    for i in 0..n {
        recipients.push_back(Address::generate(&env));
        amounts.push_back(base_amount);
        let ratio = ((denominator / n as u64) as u32).max(1);
        ratios.push_back(ratio);
    }

    let now = env.ledger().timestamp();
    let deadline = offset_timestamp(now, 86400);
    env.ledger().set_timestamp(now);

    let mut opts = default_options(&env);
    opts.ratios = ratios.clone();
    opts.ext.ratio_denominator = denominator;

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
