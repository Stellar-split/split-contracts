#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env, Vec};
use split_fuzz::{catch_expected, client, default_options, fund, offset_timestamp, setup};

#[derive(Debug, Arbitrary)]
struct Input {
    /// Deadline offset for the one seed invoice created up front. Negative
    /// values mean payment is attempted against an already-expired invoice.
    deadline_offset_secs: i32,
    /// Raw invoice id passed to `pay` — deliberately not clamped, so most
    /// inputs target nonexistent invoices (id != 1) and exercise
    /// `InvoiceNotFound`, while a fraction land on the one real invoice.
    invoice_id: u64,
    /// Full i128 range, including zero and negative — `pay` must reject
    /// non-positive amounts and must not overflow `funded` for huge ones.
    amount: i128,
    nonce: u64,
    auto_convert: bool,
    donate_on_failure: bool,
    /// Occasionally pay as the invoice's own creator/recipient instead of a
    /// fresh address, probing any self-payment special-casing.
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

    // Seed invoice creation itself is out of scope for this target (it's
    // covered by fuzz_create_invoice); swallow any rejection here and skip
    // straight to nothing-to-pay-into if the seed couldn't be created.
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

    let payer = if input.reuse_creator_as_payer {
        creator.clone()
    } else {
        Address::generate(&env)
    };
    fund(&env, &token_id, &payer);

    // Bias roughly half of inputs onto the real invoice id so the fuzzer
    // spends meaningful time past the InvoiceNotFound guard, while the raw
    // `input.invoice_id` still covers arbitrary/nonexistent ids directly.
    let invoice_id = match seed_id {
        Some(id) if input.invoice_id % 2 == 0 => id,
        _ => input.invoice_id,
    };

    let _ = catch_expected(std::panic::AssertUnwindSafe(|| {
        c.pay(
            &payer,
            &invoice_id,
            &input.amount,
            &input.nonce,
            &input.auto_convert,
            &input.donate_on_failure,
        )
    }));
});
