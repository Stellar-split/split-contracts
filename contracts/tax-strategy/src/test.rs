#![cfg(test)]

extern crate std;

use crate::{
    Error, StrategyConfig, StrategyKind, TaxBracket, TaxStrategyContract, TaxStrategyContractClient,
    MAX_DEDUCTION_BPS, MAX_RATE_BPS, MIN_PERIOD_LEDGERS,
};
use soroban_sdk::testutils::{Address as _, Events as _, Ledger};
use soroban_sdk::{symbol_short, token, vec, Address, Env, Symbol, TryFromVal, Val, Vec};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct Setup {
    env: Env,
    client: TaxStrategyContractClient<'static>,
    admin: Address,
    creator: Address,
    payer: Address,
    reserve: Address,
    savings: Address,
    token: Address,
}

fn setup() -> Setup {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(TaxStrategyContract, ());
    let client = TaxStrategyContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(token_admin).address();
    let payer = Address::generate(&env);
    token::StellarAssetClient::new(&env, &token).mint(&payer, &1_000_000);

    Setup {
        creator: Address::generate(&env),
        reserve: Address::generate(&env),
        savings: Address::generate(&env),
        env,
        client,
        admin,
        payer,
        token,
    }
}

/// 0% up to 1_000, 10% from 1_000, 30% from 5_000.
fn brackets(env: &Env) -> Vec<TaxBracket> {
    vec![
        env,
        TaxBracket { threshold: 0, rate_bps: 0 },
        TaxBracket { threshold: 1_000, rate_bps: 1_000 },
        TaxBracket { threshold: 5_000, rate_bps: 3_000 },
    ]
}

fn flat(s: &Setup, flat_bps: u32, deduction_bps: u32) -> StrategyConfig {
    StrategyConfig {
        kind: StrategyKind::Flat,
        flat_bps,
        brackets: Vec::new(&s.env),
        template: None,
        reserve_wallet: s.reserve.clone(),
        deduction_bps,
        deduction_wallet: if deduction_bps > 0 { Some(s.savings.clone()) } else { None },
        period_ledgers: MIN_PERIOD_LEDGERS,
    }
}

fn progressive(s: &Setup) -> StrategyConfig {
    StrategyConfig {
        kind: StrategyKind::Progressive,
        flat_bps: 0,
        brackets: brackets(&s.env),
        template: None,
        reserve_wallet: s.reserve.clone(),
        deduction_bps: 0,
        deduction_wallet: None,
        period_ledgers: MIN_PERIOD_LEDGERS,
    }
}

fn template(s: &Setup, name: Symbol) -> StrategyConfig {
    StrategyConfig {
        kind: StrategyKind::Template,
        flat_bps: 0,
        brackets: Vec::new(&s.env),
        template: Some(name),
        reserve_wallet: s.reserve.clone(),
        deduction_bps: 0,
        deduction_wallet: None,
        period_ledgers: MIN_PERIOD_LEDGERS,
    }
}

fn balance(s: &Setup, who: &Address) -> i128 {
    token::Client::new(&s.env, &s.token).balance(who)
}

/// Returns true if an event with topics `(tax, action, ..)` was emitted.
fn has_event(env: &Env, action: Symbol) -> bool {
    for event in env.events().all().events().iter() {
        let soroban_sdk::xdr::ContractEventBody::V0(body) = &event.body;
        let topics: &[soroban_sdk::xdr::ScVal] = body.topics.as_ref();
        if topics.len() < 2 {
            continue;
        }
        let t0 = Val::try_from_val(env, &topics[0]).ok();
        let t1 = Val::try_from_val(env, &topics[1]).ok();
        if let (Some(t0), Some(t1)) = (t0, t1) {
            let s0 = Symbol::try_from_val(env, &t0).ok();
            let s1 = Symbol::try_from_val(env, &t1).ok();
            if s0 == Some(symbol_short!("tax")) && s1 == Some(action.clone()) {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Admin templates
// ---------------------------------------------------------------------------

#[test]
fn test_initialize_twice_fails() {
    let s = setup();
    assert_eq!(s.client.try_initialize(&s.admin), Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn test_set_and_get_template() {
    let s = setup();
    s.client.set_template(&s.admin, &symbol_short!("ng_2026"), &brackets(&s.env));
    assert!(has_event(&s.env, symbol_short!("tmpl_set")));
    assert_eq!(s.client.get_template(&symbol_short!("ng_2026")), Some(brackets(&s.env)));
}

#[test]
fn test_non_admin_cannot_set_template() {
    let s = setup();
    assert_eq!(
        s.client.try_set_template(&s.creator, &symbol_short!("x"), &brackets(&s.env)),
        Err(Ok(Error::NotAdmin))
    );
}

#[test]
fn test_template_brackets_must_start_at_zero() {
    let s = setup();
    let bad = vec![&s.env, TaxBracket { threshold: 10, rate_bps: 100 }];
    assert_eq!(
        s.client.try_set_template(&s.admin, &symbol_short!("x"), &bad),
        Err(Ok(Error::InvalidBrackets))
    );
}

#[test]
fn test_template_brackets_must_ascend() {
    let s = setup();
    let bad = vec![
        &s.env,
        TaxBracket { threshold: 0, rate_bps: 100 },
        TaxBracket { threshold: 500, rate_bps: 200 },
        TaxBracket { threshold: 500, rate_bps: 300 },
    ];
    assert_eq!(
        s.client.try_set_template(&s.admin, &symbol_short!("x"), &bad),
        Err(Ok(Error::InvalidBrackets))
    );
}

#[test]
fn test_template_rate_too_high_fails() {
    let s = setup();
    let bad = vec![&s.env, TaxBracket { threshold: 0, rate_bps: MAX_RATE_BPS + 1 }];
    assert_eq!(
        s.client.try_set_template(&s.admin, &symbol_short!("x"), &bad),
        Err(Ok(Error::InvalidRate))
    );
}

#[test]
fn test_remove_template() {
    let s = setup();
    s.client.set_template(&s.admin, &symbol_short!("ng_2026"), &brackets(&s.env));
    s.client.remove_template(&s.admin, &symbol_short!("ng_2026"));
    assert!(has_event(&s.env, symbol_short!("tmpl_del")));
    assert!(s.client.get_template(&symbol_short!("ng_2026")).is_none());
    assert_eq!(
        s.client.try_remove_template(&s.admin, &symbol_short!("ng_2026")),
        Err(Ok(Error::TemplateNotFound))
    );
}

// ---------------------------------------------------------------------------
// Strategy configuration
// ---------------------------------------------------------------------------

#[test]
fn test_set_strategy() {
    let s = setup();
    s.client.set_strategy(&s.creator, &flat(&s, 2_000, 0));
    assert!(has_event(&s.env, symbol_short!("strategy")));
    let stored = s.client.get_strategy(&s.creator).unwrap();
    assert_eq!(stored.config.kind, StrategyKind::Flat);
    assert_eq!(stored.config.flat_bps, 2_000);
}

#[test]
fn test_flat_rate_too_high_fails() {
    let s = setup();
    assert_eq!(
        s.client.try_set_strategy(&s.creator, &flat(&s, MAX_RATE_BPS + 1, 0)),
        Err(Ok(Error::InvalidRate))
    );
}

#[test]
fn test_deduction_too_high_fails() {
    let s = setup();
    assert_eq!(
        s.client.try_set_strategy(&s.creator, &flat(&s, 1_000, MAX_DEDUCTION_BPS + 1)),
        Err(Ok(Error::InvalidDeduction))
    );
}

#[test]
fn test_deduction_without_wallet_fails() {
    let s = setup();
    let mut cfg = flat(&s, 1_000, 500);
    cfg.deduction_wallet = None;
    assert_eq!(
        s.client.try_set_strategy(&s.creator, &cfg),
        Err(Ok(Error::InvalidDeduction))
    );
}

#[test]
fn test_invalid_period_fails() {
    let s = setup();
    let mut cfg = flat(&s, 1_000, 0);
    cfg.period_ledgers = MIN_PERIOD_LEDGERS - 1;
    assert_eq!(s.client.try_set_strategy(&s.creator, &cfg), Err(Ok(Error::InvalidPeriod)));
}

#[test]
fn test_template_strategy_requires_existing_template() {
    let s = setup();
    assert_eq!(
        s.client.try_set_strategy(&s.creator, &template(&s, symbol_short!("nope"))),
        Err(Ok(Error::TemplateNotFound))
    );
    let mut cfg = template(&s, symbol_short!("nope"));
    cfg.template = None;
    assert_eq!(s.client.try_set_strategy(&s.creator, &cfg), Err(Ok(Error::MissingTemplate)));
}

#[test]
fn test_remove_strategy() {
    let s = setup();
    s.client.set_strategy(&s.creator, &flat(&s, 1_000, 0));
    s.client.remove_strategy(&s.creator);
    assert!(has_event(&s.env, symbol_short!("removed")));
    assert!(s.client.get_strategy(&s.creator).is_none());
    assert_eq!(
        s.client.try_process_income(&s.payer, &s.creator, &s.token, &1_000),
        Err(Ok(Error::StrategyNotFound))
    );
}

// ---------------------------------------------------------------------------
// Income routing
// ---------------------------------------------------------------------------

#[test]
fn test_flat_strategy_with_deduction() {
    let s = setup();
    s.client.set_strategy(&s.creator, &flat(&s, 2_000, 1_000));
    let b = s.client.process_income(&s.payer, &s.creator, &s.token, &2_000);
    assert!(has_event(&s.env, symbol_short!("income")));

    assert_eq!(b.deducted, 200);
    assert_eq!(b.taxable, 1_800);
    assert_eq!(b.reserved, 360);
    assert_eq!(b.net, 1_440);
    assert_eq!(b.effective_rate_bps, 1_800);

    assert_eq!(balance(&s, &s.savings), 200);
    assert_eq!(balance(&s, &s.reserve), 360);
    assert_eq!(balance(&s, &s.creator), 1_440);
}

#[test]
fn test_deduction_lowers_reserve_versus_no_deduction() {
    let s = setup();
    s.client.set_strategy(&s.creator, &flat(&s, 2_000, 0));
    let without = s.client.estimate(&s.creator, &10_000);
    let with = s.client.estimate_config(&s.creator, &flat(&s, 2_000, 2_000), &10_000);
    assert_eq!(without.reserved, 2_000);
    assert_eq!(with.reserved, 1_600);
    assert!(with.effective_rate_bps < without.effective_rate_bps);
}

#[test]
fn test_progressive_brackets_track_period_income() {
    let s = setup();
    s.client.set_strategy(&s.creator, &progressive(&s));

    // 0 → 2_000: 1_000 at 0% + 1_000 at 10%.
    let first = s.client.process_income(&s.payer, &s.creator, &s.token, &2_000);
    assert_eq!(first.reserved, 100);

    // 2_000 → 6_000: 3_000 at 10% + 1_000 at 30%.
    let second = s.client.process_income(&s.payer, &s.creator, &s.token, &4_000);
    assert_eq!(second.reserved, 600);

    let period = s.client.get_period(&s.creator);
    assert_eq!(period.gross_income, 6_000);
    assert_eq!(period.taxable_income, 6_000);
    assert_eq!(period.reserved, 700);
    assert_eq!(period.net_paid, 5_300);
    assert_eq!(balance(&s, &s.reserve), 700);
    assert_eq!(balance(&s, &s.creator), 5_300);
}

#[test]
fn test_template_strategy_follows_template_updates() {
    let s = setup();
    s.client.set_template(&s.admin, &symbol_short!("ng_2026"), &brackets(&s.env));
    s.client.set_strategy(&s.creator, &template(&s, symbol_short!("ng_2026")));
    assert_eq!(s.client.estimate(&s.creator, &2_000).reserved, 100);

    let flat_10 = vec![&s.env, TaxBracket { threshold: 0, rate_bps: 1_000 }];
    s.client.set_template(&s.admin, &symbol_short!("ng_2026"), &flat_10);
    assert_eq!(s.client.estimate(&s.creator, &2_000).reserved, 200);
}

#[test]
fn test_removed_template_blocks_income() {
    let s = setup();
    s.client.set_template(&s.admin, &symbol_short!("ng_2026"), &brackets(&s.env));
    s.client.set_strategy(&s.creator, &template(&s, symbol_short!("ng_2026")));
    s.client.remove_template(&s.admin, &symbol_short!("ng_2026"));
    assert_eq!(
        s.client.try_process_income(&s.payer, &s.creator, &s.token, &1_000),
        Err(Ok(Error::TemplateNotFound))
    );
}

#[test]
fn test_zero_income_fails() {
    let s = setup();
    s.client.set_strategy(&s.creator, &flat(&s, 1_000, 0));
    assert_eq!(
        s.client.try_process_income(&s.payer, &s.creator, &s.token, &0),
        Err(Ok(Error::InvalidAmount))
    );
}

#[test]
fn test_period_rollover_resets_brackets() {
    let s = setup();
    s.client.set_strategy(&s.creator, &progressive(&s));
    s.client.process_income(&s.payer, &s.creator, &s.token, &6_000);
    assert_eq!(s.client.get_period(&s.creator).reserved, 700);

    let start = s.client.get_period(&s.creator).start_ledger;
    s.env
        .ledger()
        .with_mut(|l| l.sequence_number = start + MIN_PERIOD_LEDGERS);

    // Before any new income the view already reports a fresh period.
    assert_eq!(s.client.get_period(&s.creator).gross_income, 0);

    // New period: brackets restart from zero.
    let b = s.client.process_income(&s.payer, &s.creator, &s.token, &2_000);
    assert!(has_event(&s.env, symbol_short!("period")));
    assert_eq!(b.reserved, 100);

    let last = s.client.get_last_period(&s.creator).unwrap();
    assert_eq!(last.gross_income, 6_000);
    assert_eq!(last.reserved, 700);
    assert_eq!(s.client.get_period(&s.creator).gross_income, 2_000);
}

#[test]
fn test_estimate_does_not_move_funds_or_state() {
    let s = setup();
    s.client.set_strategy(&s.creator, &progressive(&s));
    let est = s.client.estimate(&s.creator, &6_000);
    assert_eq!(est.reserved, 700);
    assert_eq!(s.client.get_period(&s.creator).gross_income, 0);
    assert_eq!(balance(&s, &s.reserve), 0);
}

#[test]
fn test_compare_flat_vs_progressive() {
    let s = setup();
    s.client.set_strategy(&s.creator, &flat(&s, 2_000, 0));
    let flat_est = s.client.estimate(&s.creator, &3_000);
    let prog_est = s.client.estimate_config(&s.creator, &progressive(&s), &3_000);
    assert_eq!(flat_est.reserved, 600);
    assert_eq!(prog_est.reserved, 200);
}
