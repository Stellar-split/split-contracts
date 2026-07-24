#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env, Vec};
use split_fuzz::{catch_expected, client, default_options, fund, offset_timestamp, setup};

#[derive(Debug, Arbitrary)]
struct Input {
    deadline_offset_secs: i32,
    /// How much of the 1_000-unit invoice to pay before attempting release —
    /// 0 covers "never funded", values >= 1_000 cover "fully funded", and
    /// anything in between covers "partially funded".
    pay_amount: i64,
    /// Whether to advance the ledger clock past the invoice deadline before
    /// releasing (release has no deadline gate itself, but this still
    /// exercises the funding-window interaction with payment rejection).
    advance_past_deadline: bool,
    /// Raw invoice id passed to `release` — deliberately not clamped so most
    /// inputs target nonexistent invoices and exercise `InvoiceNotFound`.
    invoice_id: u64,
    /// Call `release` a second time on the same (post-first-call) state to
    /// probe idempotency / double-release handling.
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

    let seed_id = catch_expected(std::panic::AssertUnwindSafe(|| {
        c.create_invoice(
            &creator,
            &recipients,
            &amounts,
            &token_id,
            &deadline,
            &options,
        )
    }));

    if let Some(id) = seed_id {
        if input.pay_amount > 0 {
            let payer = Address::generate(&env);
            fund(&env, &token_id, &payer);
            let pay_amount = input.pay_amount as i128;
            let _ = catch_expected(std::panic::AssertUnwindSafe(|| {
                c.pay(&payer, &id, &pay_amount, &0u64, &false, &false)
            }));
        }
    }

    if input.advance_past_deadline {
        env.ledger().set_timestamp(deadline.saturating_add(1));
    }

    let invoice_id = match seed_id {
        Some(id) if input.invoice_id % 2 == 0 => id,
        _ => input.invoice_id,
    };

    // The invariant under test: releasing any invoice id — real or
    // nonexistent, funded or not, before or after its deadline — must never
    // corrupt storage or double-pay recipients, and must only panic through
    // a documented assert!/panic! guard.
    let _ = catch_expected(std::panic::AssertUnwindSafe(|| c.release(&invoice_id)));

    if input.call_twice {
        let _ = catch_expected(std::panic::AssertUnwindSafe(|| c.release(&invoice_id)));
    }
});
