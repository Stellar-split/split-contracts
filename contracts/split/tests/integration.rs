//! Issue #793: canonical end-to-end health check for the split contract,
//! driving one invoice through a 30-day lifecycle on a simulated ledger clock.

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};
use split::types::{InvoiceOptions, InvoiceOptions2, InvoiceStatus, OverflowBehavior};
use split::{SplitContract, SplitContractClient};

const DAY: u64 = 86_400;
const START: u64 = 1_700_000_000;

fn at_day(env: &Env, day: u64) {
    env.ledger().set_timestamp(START + day * DAY);
}

fn options(env: &Env, scheduled_release_at: u64) -> InvoiceOptions {
    InvoiceOptions {
        co_creators: Vec::new(env),
        allow_early_withdrawal: false,
        bonus_pool: 0,
        bonus_max_payers: 0,
        creator_cosigner: None,
        velocity_limit: 0,
        velocity_window: 0,
        prerequisite_id: None,
        tranches: Vec::new(env),
        co_signers: Vec::new(env),
        required_signatures: 0,
        penalty_bps: None,
        penalty_deadline: None,
        min_funding_bps: None,
        release_stages: Vec::new(env),
        price_oracle: None,
        swap_tokens: Vec::new(env),
        tax_bps: None,
        tax_authority: None,
        insurance_premium_bps: None,
        smart_route: None,
        notification_contract: None,
        overflow_behavior: OverflowBehavior::Reject,
        convert_to_stream: false,
        accepted_tokens: Vec::new(env),
        forward_to: None,
        forward_invoice_id: None,
        split_rules: Vec::new(env),
        auto_resolve_rules: Vec::new(env),
        condition_oracle: None,
        cross_chain_ref: None,
        allowed_payers: None,
        refund_grace_secs: None,
        priorities: Vec::new(env),
        require_kyc: false,
        // Hold the funds until day 30 so the final payment doesn't auto-release.
        scheduled_release_at: Some(scheduled_release_at),
        ratios: Vec::new(env),
        cosigners: None,
        cosigner_threshold: None,
        ext: InvoiceOptions2::default(),
    }
}

#[test]
fn test_full_30_day_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SplitContract, ());
    let c = SplitContractClient::new(&env, &contract_id);
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let token = TokenClient::new(&env, &token_id);
    let minter = StellarAssetClient::new(&env, &token_id);

    c.initialize(
        &Address::generate(&env),
        &0_i128,
        &Address::generate(&env),
        &token_id,
        &0_u32,
        &None,
        &0_u32,
        &0_u32,
        &0_u64,
    );

    let creator = Address::generate(&env);
    let recipient_a = Address::generate(&env);
    let recipient_b = Address::generate(&env);
    let payers = [
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    ];
    for payer in payers.iter() {
        minter.mint(payer, &100);
    }

    // Day 0: create a 300-unit invoice split 200/100, due on day 40.
    at_day(&env, 0);
    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient_a.clone());
    recipients.push_back(recipient_b.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);
    amounts.push_back(100_i128);
    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &(START + 40 * DAY),
        &options(&env, START + 30 * DAY),
    );
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);

    // Days 1, 5, 10: three payers contribute 100 each.
    for (payer, (day, funded)) in payers.iter().zip([(1, 100), (5, 200), (10, 300)]) {
        at_day(&env, day);
        c.pay(payer, &id, &100_i128, &0_u64, &false, &false, &None);

        let invoice = c.get_invoice(&id);
        assert_eq!(invoice.funded, funded);
        assert_eq!(invoice.status, InvoiceStatus::Pending);
        assert_eq!(token.balance(payer), 0);
        assert_eq!(token.balance(&contract_id), funded);
    }
    assert_eq!(token.balance(&recipient_a), 0);
    assert_eq!(token.balance(&recipient_b), 0);

    // Day 30: the scheduled release pays every recipient in full.
    at_day(&env, 30);
    c.trigger_scheduled_release(&id);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(token.balance(&recipient_a), 200);
    assert_eq!(token.balance(&recipient_b), 100);
    assert_eq!(token.balance(&contract_id), 0);
}
