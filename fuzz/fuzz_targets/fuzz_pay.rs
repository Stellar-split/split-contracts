#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env, Vec};
use split_fuzz::{client, default_options, fund, offset_timestamp, setup};

#[derive(Debug, Arbitrary)]
struct Input {
    deadline_offset_secs: i32,
    invoice_id: u64,
    amount: i128,
    nonce: u64,
    auto_convert: bool,
    donate_on_failure: bool,
    reuse_creator_as_payer: bool,
}

fuzz_target!(|input: Input| {
    let env = Env::default();
    let (contract_id, token_id) = setup(&env);
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient);
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let now = env.ledger().timestamp();
    let deadline = offset_timestamp(now, input.deadline_offset_secs as i64);
    let options = default_options(&env);

    let seed_id = c
        .try_create_invoice(&creator, &recipients, &amounts, &token_id, &deadline, &options)
        .ok()
        .and_then(|r| r.ok());

    let payer = if input.reuse_creator_as_payer {
        creator.clone()
    } else {
        Address::generate(&env)
    };
    fund(&env, &token_id, &payer);

    let invoice_id = match seed_id {
        Some(id) if input.invoice_id % 2 == 0 => id,
        _ => input.invoice_id,
    };

    let _ = c.try_pay(
        &payer,
        &invoice_id,
        &input.amount,
        &input.nonce,
        &input.auto_convert,
        &input.donate_on_failure,
        &None,
    );
});
