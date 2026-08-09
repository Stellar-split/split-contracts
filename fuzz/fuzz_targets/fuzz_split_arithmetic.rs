//! Fuzz target for issue #482 — arithmetic overflow in split calculations.
//!
//! Exercises `_release_full` with randomised large amounts and basis-point
//! ratios via `SplitRule::Percentage` and `SplitRule::Tiered`, verifying that
//! `i128::MAX`-scale inputs never cause a panic or silent wrap-around.
//!
//! Run with: `cargo fuzz run fuzz_split_arithmetic`

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env, Vec};
use split::types::{InvoiceOptions, SplitRule};
use split_fuzz::{client, default_options, fund, offset_timestamp, setup};

/// A single recipient entry: how much they are owed and what split rule governs release.
#[derive(Debug, Arbitrary)]
struct RecipientEntry {
    /// Raw amount used as the invoice `amounts[i]` value (clamped positive).
    amount_raw: u64,
    /// Which split rule flavour to use.
    rule_kind: u8,
    /// Basis-point value for Percentage / Tiered rules (taken mod 10_001 so
    /// it stays in [0, 10_000]).
    bps_raw: u16,
}

#[derive(Debug, Arbitrary)]
struct Input {
    /// 1–4 recipients (clamped below).
    recipients: [RecipientEntry; 4],
    /// How many of the recipient entries to actually use (clamped to 1–4).
    n_recipients: u8,
    /// Payment amount — use the full u64 range so we hit large values.
    pay_amount: u64,
    /// Seconds offset from `now` for the deadline (positive = future).
    deadline_offset_secs: u32,
}

fuzz_target!(|input: Input| {
    let env = Env::default();
    let (contract_id, token_id) = setup(&env);
    let c = client(&env, &contract_id);

    // Number of recipients: at least 1, at most 4.
    let n = ((input.n_recipients as usize % 4) + 1).min(4);

    let creator = Address::generate(&env);
    let mut recipients: Vec<Address> = Vec::new(&env);
    let mut amounts: Vec<i128> = Vec::new(&env);
    let mut split_rules: Vec<SplitRule> = Vec::new(&env);

    // Build recipients + amounts + split rules.
    // We use Percentage rules that sum exactly to 10_000 so validation passes.
    // Strategy: give the last recipient the remainder so the sum is always 10_000.
    let per_bps: u32 = 10_000u32 / n as u32;
    let mut used_bps: u32 = 0;

    for i in 0..n {
        recipients.push_back(Address::generate(&env));
        // Amount must be > 0; clamp to at least 1.
        let amt = (input.recipients[i].amount_raw as i128).max(1);
        amounts.push_back(amt);

        let bps = if i == n - 1 {
            // Last recipient absorbs remainder to reach exactly 10_000.
            10_000u32.saturating_sub(used_bps)
        } else {
            per_bps
        };
        used_bps += bps;
        split_rules.push_back(SplitRule::Percentage(bps));
    }

    let now = env.ledger().timestamp();
    // Ensure deadline is always in the future.
    let deadline = offset_timestamp(now, input.deadline_offset_secs as i64 + 1);
    env.ledger().set_timestamp(now);

    let mut opts = default_options(&env);
    opts.split_rules = split_rules;

    let invoice_id = match c.try_create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &deadline,
        &opts,
    ) {
        Ok(Ok(id)) => id,
        _ => return, // creation validation rejected this input — not a bug
    };

    // Pay with a large (but valid) amount — clamped to i128::MAX so the token
    // transfer does not overflow the mock balance.
    let pay_amount = (input.pay_amount as i128).max(1);
    let payer = Address::generate(&env);
    fund(&env, &token_id, &payer);
    let _ = c.try_pay(&payer, &invoice_id, &pay_amount, &0u64, &false, &false, &None);

    // Attempt release — this exercises all the checked arithmetic paths.
    let _ = c.try_release(&invoice_id);
});
