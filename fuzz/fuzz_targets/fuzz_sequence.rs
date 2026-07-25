#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env, Vec};
use split_fuzz::{client, default_options, fund, offset_timestamp, setup};

const MAX_OPS: usize = 24;

#[derive(Debug, Arbitrary)]
enum Op {
    Create {
        amount: i64,
        deadline_offset_secs: i32,
    },
    Pay {
        invoice_slot: u8,
        amount: i64,
        use_fresh_payer: bool,
    },
    Release {
        invoice_slot: u8,
    },
    Refund {
        invoice_slot: u8,
        advance_secs: u16,
    },
}

#[derive(Debug, Arbitrary)]
struct Input {
    ops: std::vec::Vec<Op>,
}

fn resolve_id(created: &[u64], slot: u8) -> u64 {
    if !created.is_empty() && slot % 2 == 0 {
        created[(slot as usize / 2) % created.len()]
    } else {
        slot as u64
    }
}

fuzz_target!(|input: Input| {
    let env = Env::default();
    let (contract_id, token_id) = setup(&env);
    let c = client(&env, &contract_id);
    let options = default_options(&env);

    let mut created: std::vec::Vec<u64> = std::vec::Vec::new();
    let mut payers: std::vec::Vec<Address> = std::vec::Vec::new();

    for op in input.ops.into_iter().take(MAX_OPS) {
        match op {
            Op::Create { amount, deadline_offset_secs } => {
                let creator = Address::generate(&env);
                let recipient = Address::generate(&env);
                let mut recipients = Vec::new(&env);
                recipients.push_back(recipient);
                let mut amounts = Vec::new(&env);
                amounts.push_back(amount as i128);
                let now = env.ledger().timestamp();
                let deadline = offset_timestamp(now, deadline_offset_secs as i64);

                if let Ok(id) = c.try_create_invoice(
                    &creator, &recipients, &amounts, &token_id, &deadline, &options,
                ) {
                    created.push(id);
                }
            }
            Op::Pay { invoice_slot, amount, use_fresh_payer } => {
                let invoice_id = resolve_id(&created, invoice_slot);
                let payer = if !use_fresh_payer && !payers.is_empty() {
                    payers[invoice_slot as usize % payers.len()].clone()
                } else {
                    let fresh = Address::generate(&env);
                    fund(&env, &token_id, &fresh);
                    payers.push(fresh.clone());
                    fresh
                };
                let _ = c.try_pay(&payer, &invoice_id, &(amount as i128), &0u64, &false, &false);
            }
            Op::Release { invoice_slot } => {
                let invoice_id = resolve_id(&created, invoice_slot);
                let _ = c.try_release(&invoice_id);
            }
            Op::Refund { invoice_slot, advance_secs } => {
                let invoice_id = resolve_id(&created, invoice_slot);
                if advance_secs > 0 {
                    let now = env.ledger().timestamp();
                    env.ledger().set_timestamp(now.saturating_add(advance_secs as u64));
                }
                let _ = c.try_refund(&invoice_id);
            }
        }
    }
});
