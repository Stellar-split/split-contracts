#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env, Vec};
use split_fuzz::{client, default_options, offset_timestamp, setup};

const MAX_RECIPIENTS: usize = 20;

#[derive(Debug, Arbitrary)]
struct Input {
    /// Reduced mod (MAX_RECIPIENTS + 1); 0 deliberately exercises the
    /// "must have at least one recipient" rejection path.
    recipient_count: u8,
    amounts: [i64; MAX_RECIPIENTS],
    /// Signed offset from "now" — negative values exercise the
    /// "deadline must be in the future" rejection path.
    deadline_offset_secs: i32,
    bonus_pool: i32,
    bonus_max_payers: u32,
    tax_bps: u16,
    tax_authority_set: bool,
    min_funding_bps: u16,
    penalty_bps: u16,
    insurance_premium_bps: u16,
    force_duplicate_recipients: bool,
}

fuzz_target!(|input: Input| {
    let env = Env::default();
    let (contract_id, token_id) = setup(&env);
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let n = (input.recipient_count as usize) % (MAX_RECIPIENTS + 1);

    let mut recipients = Vec::new(&env);
    let mut amounts = Vec::new(&env);
    let shared_recipient = Address::generate(&env);
    for i in 0..n {
        if input.force_duplicate_recipients {
            recipients.push_back(shared_recipient.clone());
        } else {
            recipients.push_back(Address::generate(&env));
        }
        amounts.push_back(input.amounts[i] as i128);
    }

    let now = env.ledger().timestamp();
    let deadline = offset_timestamp(now, input.deadline_offset_secs as i64);

    let mut options = default_options(&env);
    options.bonus_pool = input.bonus_pool as i128;
    options.bonus_max_payers = input.bonus_max_payers;
    options.min_funding_bps = Some((input.min_funding_bps % 10_001) as u32);
    options.penalty_bps = Some((input.penalty_bps % 10_001) as u32);
    options.insurance_premium_bps = Some((input.insurance_premium_bps % 10_001) as u32);
    options.tax_bps = Some((input.tax_bps % 10_001) as u32);
    if input.tax_authority_set {
        options.tax_authority = Some(Address::generate(&env));
    }

    // Use try_create_invoice so validation rejections (deadline in the past,
    // negative amounts, duplicate recipients, etc.) come back as Err instead
    // of panicking.  Any genuine host-level crash is still a real finding.
    let _ = c.try_create_invoice(&creator, &recipients, &amounts, &token_id, &deadline, &options);
});
