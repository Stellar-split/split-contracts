#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env, Vec};
use split_fuzz::{client, default_options, fund, offset_timestamp, setup};

#[derive(Debug, Arbitrary)]
struct Input {
    deadline_offset_secs: i32,
    pay_amount: i64,
    call_after_deadline: bool,
    invoice_id: u64,
    call_twice: bool,
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

    if let Some(id) = seed_id {
        if input.pay_amount > 0 {
            let payer = Address::generate(&env);
            fund(&env, &token_id, &payer);
            let _ = c.try_pay(&payer, &id, &(input.pay_amount as i128), &0u64, &false, &false, &None);
        }
    }

    if input.call_after_deadline {
        env.ledger().set_timestamp(deadline.saturating_add(1));
    } else {
        env.ledger().set_timestamp(deadline.saturating_sub(1).max(now));
    }

    let invoice_id = match seed_id {
        Some(id) if input.invoice_id % 2 == 0 => id,
        _ => input.invoice_id,
    };

    let _ = c.try_refund(&invoice_id);

    if input.call_twice {
        let _ = c.try_refund(&invoice_id);
    }
});
