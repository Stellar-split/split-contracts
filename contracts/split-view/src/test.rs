use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, Vec,
};
use split::types::{InvoiceOptions, InvoiceOptions2, OverflowBehavior};
use split::{SplitContract, SplitContractClient};

use crate::{CreatorDashboard, SplitViewContract, SplitViewContractClient};

fn options(env: &Env) -> InvoiceOptions {
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
        scheduled_release_at: None,
        ratios: Vec::new(env),
        cosigners: None,
        cosigner_threshold: None,
        ext: InvoiceOptions2::default(),
    }
}

struct Setup<'a> {
    env: Env,
    split: SplitContractClient<'a>,
    view: SplitViewContractClient<'a>,
    token_id: Address,
}

fn setup<'a>() -> Setup<'a> {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000);

    let split_id = env.register(SplitContract, ());
    let split = SplitContractClient::new(&env, &split_id);
    let token_id = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    split.initialize(
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

    let view_id = env.register(SplitViewContract, ());
    let view = SplitViewContractClient::new(&env, &view_id);
    view.initialize(&split_id);

    Setup {
        env,
        split,
        view,
        token_id,
    }
}

fn create_invoice(s: &Setup, creator: &Address, amount: i128) -> u64 {
    let mut recipients = Vec::new(&s.env);
    recipients.push_back(Address::generate(&s.env));
    let mut amounts = Vec::new(&s.env);
    amounts.push_back(amount);
    s.split.create_invoice(
        creator,
        &recipients,
        &amounts,
        &s.token_id,
        &9_999_u64,
        &options(&s.env),
    )
}

#[test]
fn test_view_points_at_split_contract_once() {
    let s = setup();
    assert_eq!(s.view.get_split_contract(), s.split.address);
    assert!(s.view.try_initialize(&s.split.address).is_err());
}

#[test]
fn test_creator_dashboard_matches_split_creator_stats() {
    let s = setup();
    let creator = Address::generate(&s.env);
    let payer = Address::generate(&s.env);
    StellarAssetClient::new(&s.env, &s.token_id).mint(&payer, &250);

    assert_eq!(
        s.view.get_creator_dashboard(&creator),
        CreatorDashboard {
            total_invoices: 0,
            total_raised: 0,
            total_released: 0,
            total_payers: 0,
            total_refunded: 0
        }
    );

    let first = create_invoice(&s, &creator, 300);
    create_invoice(&s, &creator, 500);
    s.split
        .pay(&payer, &first, &250_i128, &0_u64, &false, &false, &None);

    let stats = s.split.get_creator_stats(&creator);
    let dashboard = s.view.get_creator_dashboard(&creator);
    assert_eq!(dashboard.total_invoices, 2);
    assert_eq!(dashboard.total_invoices, stats.total_invoices);
    assert_eq!(dashboard.total_raised, stats.total_raised);
    assert_eq!(dashboard.total_released, stats.total_released);
    assert_eq!(dashboard.total_payers, stats.total_payers);
    assert_eq!(dashboard.total_refunded, stats.total_refunded);
}

#[test]
fn test_payer_history_returns_latest_entries() {
    let s = setup();
    let creator = Address::generate(&s.env);
    let payer = Address::generate(&s.env);
    StellarAssetClient::new(&s.env, &s.token_id).mint(&payer, &1_000);

    let ids = [
        create_invoice(&s, &creator, 500),
        create_invoice(&s, &creator, 500),
        create_invoice(&s, &creator, 500),
    ];
    for (i, id) in ids.iter().enumerate() {
        s.split.contribute(id, &payer, &(10 * (i as i128 + 1)));
    }

    let all = s.view.get_payer_history(&payer, &10);
    assert_eq!(all.len(), 3);

    let latest = s.view.get_payer_history(&payer, &2);
    assert_eq!(latest.len(), 2);
    assert_eq!(latest.get(0).unwrap().invoice_id, ids[1]);
    assert_eq!(latest.get(0).unwrap().amount, 20);
    assert_eq!(latest.get(1).unwrap().invoice_id, ids[2]);
    assert_eq!(latest.get(1).unwrap().amount, 30);

    assert!(s
        .view
        .get_payer_history(&Address::generate(&s.env), &5)
        .is_empty());
}
