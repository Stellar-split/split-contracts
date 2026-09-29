//! Issue #787: global protocol analytics aggregate.
//!
//! `ProtocolStats` lives in instance storage and is updated by hooks in
//! invoice creation (`on_created`) and payment (`on_paid`). Unique creators and
//! payers are deduplicated with a persistent boolean flag per address.
//!
//! `total_released_amount` / `total_refunded_amount` are read at query time
//! from the pre-existing issue #28 counters (`total_released_key`,
//! `total_refunded_key`), which every release/refund path already maintains.
//! This keeps a single source of truth instead of patching each of those paths.
//!
//! Storage: own key enum `AnalyticsKey`.
use soroban_sdk::{contracttype, symbol_short, Address, Env, Symbol};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AnalyticsKey {
    /// Instance: aggregate counters (`StoredStats`).
    Stats,
    /// Persistent: creator has been counted.
    Creator(Address),
    /// Persistent: payer has been counted.
    Payer(Address),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct StoredStats {
    pub total_invoices: u64,
    pub total_paid_amount: i128,
    pub unique_creators: u32,
    pub unique_payers: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolStats {
    pub total_invoices: u64,
    pub total_paid_amount: i128,
    pub total_released_amount: i128,
    pub total_refunded_amount: i128,
    pub unique_creators: u32,
    pub unique_payers: u32,
}

fn load(env: &Env) -> StoredStats {
    env.storage()
        .instance()
        .get(&AnalyticsKey::Stats)
        .unwrap_or_default()
}

fn save(env: &Env, s: &StoredStats) {
    env.storage().instance().set(&AnalyticsKey::Stats, s);
}

fn emit(env: &Env, name: Symbol, value: i128) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("stats_upd")),
        (name, value),
    );
}

/// Returns true if `key` was newly flagged.
fn flag_once(env: &Env, key: AnalyticsKey) -> bool {
    if env.storage().persistent().has(&key) {
        return false;
    }
    env.storage().persistent().set(&key, &true);
    true
}

pub fn on_created(env: &Env, creator: &Address) {
    let mut s = load(env);
    s.total_invoices = s.total_invoices.saturating_add(1);
    emit(env, symbol_short!("invoices"), s.total_invoices as i128);
    if flag_once(env, AnalyticsKey::Creator(creator.clone())) {
        s.unique_creators = s.unique_creators.saturating_add(1);
        emit(env, symbol_short!("creators"), s.unique_creators as i128);
    }
    save(env, &s);
}

pub fn on_paid(env: &Env, payer: &Address, amount: i128) {
    let mut s = load(env);
    s.total_paid_amount = s.total_paid_amount.saturating_add(amount);
    emit(env, symbol_short!("paid"), s.total_paid_amount);
    if flag_once(env, AnalyticsKey::Payer(payer.clone())) {
        s.unique_payers = s.unique_payers.saturating_add(1);
        emit(env, symbol_short!("payers"), s.unique_payers as i128);
    }
    save(env, &s);
}

pub fn get(env: &Env, released: i128, refunded: i128) -> ProtocolStats {
    let s = load(env);
    ProtocolStats {
        total_invoices: s.total_invoices,
        total_paid_amount: s.total_paid_amount,
        total_released_amount: released,
        total_refunded_amount: refunded,
        unique_creators: s.unique_creators,
        unique_payers: s.unique_payers,
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger},
        token::StellarAssetClient,
        Address, Env, Vec,
    };

    #[test]
    fn stats_track_create_pay_release_refund_and_dedupe() {
        let env = Env::default();
        env.mock_all_auths();
        let contract = env.register(SplitContract, ());
        let token = env
            .register_stellar_asset_contract_v2(Address::generate(&env))
            .address();
        let c = SplitContractClient::new(&env, &contract);
        c.initialize(
            &Address::generate(&env),
            &0_i128,
            &Address::generate(&env),
            &token,
            &0_u32,
            &None,
            &0_u32,
            &0_u32,
            &0_u64,
        );
        env.ledger().set_timestamp(1_000);
        let creator = Address::generate(&env);
        let payer = Address::generate(&env);
        StellarAssetClient::new(&env, &token).mint(&payer, &1_000);

        let mk = |amount: i128| {
            let mut r = Vec::new(&env);
            r.push_back(Address::generate(&env));
            let mut a = Vec::new(&env);
            a.push_back(amount);
            let opts = crate::types::InvoiceOptions {
                co_creators: Vec::new(&env),
                allow_early_withdrawal: false,
                bonus_pool: 0,
                bonus_max_payers: 0,
                creator_cosigner: None,
                velocity_limit: 0,
                velocity_window: 0,
                prerequisite_id: None,
                tranches: Vec::new(&env),
                co_signers: Vec::new(&env),
                required_signatures: 0,
                penalty_bps: None,
                penalty_deadline: None,
                min_funding_bps: None,
                release_stages: Vec::new(&env),
                price_oracle: None,
                swap_tokens: Vec::new(&env),
                tax_bps: None,
                tax_authority: None,
                insurance_premium_bps: None,
                smart_route: None,
                notification_contract: None,
                overflow_behavior: crate::types::OverflowBehavior::Reject,
                convert_to_stream: false,
                accepted_tokens: Vec::new(&env),
                forward_to: None,
                forward_invoice_id: None,
                split_rules: Vec::new(&env),
                auto_resolve_rules: Vec::new(&env),
                condition_oracle: None,
                cross_chain_ref: None,
                allowed_payers: None,
                refund_grace_secs: None,
                priorities: Vec::new(&env),
                require_kyc: false,
                scheduled_release_at: None,
                ratios: Vec::new(&env),
                cosigners: None,
                cosigner_threshold: None,
                ext: crate::types::InvoiceOptions2::default(),
            };
            c.create_invoice(&creator, &r, &a, &token, &2_000_u64, &opts)
        };

        let s0 = c.get_protocol_stats();
        assert_eq!(s0.total_invoices, 0);

        let a = mk(100);
        let b = mk(100);
        let s1 = c.get_protocol_stats();
        assert_eq!(s1.total_invoices, 2);
        assert_eq!(s1.unique_creators, 1);

        c.pay(&payer, &a, &100, &0_u64, &false, &false, &None);
        c.pay(&payer, &b, &40, &0_u64, &false, &false, &None);
        let s2 = c.get_protocol_stats();
        assert_eq!(s2.total_paid_amount, 140);
        assert_eq!(s2.unique_payers, 1);

        c.release(&a);
        assert_eq!(c.get_protocol_stats().total_released_amount, 100);

        env.ledger().set_timestamp(5_000);
        c.refund(&b);
        assert_eq!(c.get_protocol_stats().total_refunded_amount, 40);
    }
}
