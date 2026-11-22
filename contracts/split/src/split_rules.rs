//! Split rules engine (issue #913).
//!
//! Lets a creator attach criteria-based rules to an invoice that compute each
//! recipient's amount automatically from the invoice's live state, instead of
//! the static per-recipient amounts.
//!
//! A rule set is a list of [`SplitRuleDef`]s. For each recipient, rules
//! targeting that recipient are considered in ascending `priority` order and
//! the **first** whose [`RuleCriteria`] matches the [`RuleContext`] decides
//! the recipient's [`RuleAmount`]. Recipients with no matching rule receive
//! nothing.
//!
//! Amounts are resolved in two passes: `Fixed` and `Percentage` first, then a
//! single optional `Remainder` recipient receives whatever is left of the
//! funded amount. Evaluation fails if resolved amounts exceed the funded
//! amount.

use soroban_sdk::{contracttype, symbol_short, Address, Env, Symbol, Vec};

use crate::events;
use crate::types::InvoiceStatus;

/// Maximum number of rules per invoice.
pub const MAX_RULES: u32 = 50;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Condition under which a rule applies.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuleCriteria {
    /// Always matches.
    Always,
    /// Matches when `funded >= value`.
    FundedAtLeast(i128),
    /// Matches when `funded < value`.
    FundedBelow(i128),
    /// Matches when the invoice has at least `value` payments.
    PayerCountAtLeast(u32),
    /// Matches when the ledger timestamp is `>= value`.
    AfterTimestamp(u64),
    /// Matches when the ledger timestamp is `< value`.
    BeforeTimestamp(u64),
}

/// How a matching rule computes the recipient's amount.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuleAmount {
    /// Exactly this amount.
    Fixed(i128),
    /// `funded * bps / 10_000`.
    Percentage(u32),
    /// Whatever remains after all `Fixed` and `Percentage` amounts.
    Remainder,
}

/// One rule in an invoice's rule set.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SplitRuleDef {
    /// Index into the invoice's recipient list.
    pub recipient_index: u32,
    /// Lower values are evaluated first.
    pub priority: u32,
    pub criteria: RuleCriteria,
    pub amount: RuleAmount,
}

/// Invoice state that rules are evaluated against.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleContext {
    pub funded: i128,
    pub payer_count: u32,
    pub timestamp: u64,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

fn rules_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("srule_eng"), invoice_id)
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validate a rule set against an invoice with `recipient_count` recipients.
///
/// Checks: non-empty, at most [`MAX_RULES`], every `recipient_index` in
/// range, no two rules for the same recipient share a priority, `Fixed`
/// amounts positive, `Percentage` within `1..=10_000`, threshold criteria
/// non-negative, at most one recipient using `Remainder`, and the sum of
/// unconditional (`Always`) percentages not above 100 %.
pub fn validate_rules(rules: &Vec<SplitRuleDef>, recipient_count: u32) {
    assert!(!rules.is_empty(), "split rules must not be empty");
    assert!(rules.len() <= MAX_RULES, "too many split rules");

    let mut remainder_recipient: Option<u32> = None;
    let mut always_bps: u32 = 0;

    for i in 0..rules.len() {
        let rule = rules.get(i).unwrap();
        assert!(
            rule.recipient_index < recipient_count,
            "rule recipient index out of range"
        );

        for j in (i + 1)..rules.len() {
            let other = rules.get(j).unwrap();
            assert!(
                !(other.recipient_index == rule.recipient_index && other.priority == rule.priority),
                "duplicate rule priority for recipient"
            );
        }

        if let RuleCriteria::FundedAtLeast(v) | RuleCriteria::FundedBelow(v) = rule.criteria {
            assert!(v >= 0, "rule threshold must be non-negative");
        }

        match rule.amount {
            RuleAmount::Fixed(amt) => {
                assert!(amt > 0, "fixed rule amount must be positive");
            }
            RuleAmount::Percentage(bps) => {
                assert!((1..=10_000).contains(&bps), "rule percentage must be 1..=10000 bps");
                if rule.criteria == RuleCriteria::Always {
                    always_bps = always_bps.saturating_add(bps);
                }
            }
            RuleAmount::Remainder => match remainder_recipient {
                Some(r) => assert!(
                    r == rule.recipient_index,
                    "only one recipient may take the remainder"
                ),
                None => remainder_recipient = Some(rule.recipient_index),
            },
        }
    }

    assert!(
        always_bps <= 10_000,
        "unconditional percentages exceed 100%"
    );
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

pub fn criteria_matches(criteria: &RuleCriteria, ctx: &RuleContext) -> bool {
    match criteria {
        RuleCriteria::Always => true,
        RuleCriteria::FundedAtLeast(v) => ctx.funded >= *v,
        RuleCriteria::FundedBelow(v) => ctx.funded < *v,
        RuleCriteria::PayerCountAtLeast(v) => ctx.payer_count >= *v,
        RuleCriteria::AfterTimestamp(v) => ctx.timestamp >= *v,
        RuleCriteria::BeforeTimestamp(v) => ctx.timestamp < *v,
    }
}

/// Select the winning rule for `recipient_index`: the matching rule with the
/// lowest priority value.
fn select_rule(
    rules: &Vec<SplitRuleDef>,
    recipient_index: u32,
    ctx: &RuleContext,
) -> Option<SplitRuleDef> {
    let mut best: Option<SplitRuleDef> = None;
    for rule in rules.iter() {
        if rule.recipient_index != recipient_index || !criteria_matches(&rule.criteria, ctx) {
            continue;
        }
        let better = match &best {
            Some(b) => rule.priority < b.priority,
            None => true,
        };
        if better {
            best = Some(rule);
        }
    }
    best
}

/// Evaluate `rules` for `recipient_count` recipients against `ctx`, returning
/// one amount per recipient. Panics if resolved amounts exceed `ctx.funded`.
pub fn evaluate(
    env: &Env,
    rules: &Vec<SplitRuleDef>,
    recipient_count: u32,
    ctx: &RuleContext,
) -> Vec<i128> {
    let funded = if ctx.funded > 0 { ctx.funded } else { 0 };
    let mut amounts: Vec<i128> = Vec::new(env);
    let mut remainder_idx: Option<u32> = None;
    let mut allocated: i128 = 0;

    for i in 0..recipient_count {
        let amount = match select_rule(rules, i, ctx) {
            Some(rule) => match rule.amount {
                RuleAmount::Fixed(amt) => amt,
                RuleAmount::Percentage(bps) => funded
                    .checked_mul(bps as i128)
                    .expect("ArithmeticOverflow")
                    / 10_000,
                RuleAmount::Remainder => {
                    remainder_idx = Some(i);
                    0
                }
            },
            None => 0,
        };
        allocated = allocated.checked_add(amount).expect("ArithmeticOverflow");
        amounts.push_back(amount);
    }

    assert!(allocated <= funded, "split rules exceed funded amount");
    if let Some(idx) = remainder_idx {
        amounts.set(idx, funded - allocated);
    }
    amounts
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

pub fn get_rules(env: &Env, invoice_id: u64) -> Vec<SplitRuleDef> {
    env.storage()
        .persistent()
        .get(&rules_key(invoice_id))
        .unwrap_or_else(|| Vec::new(env))
}

/// Attach `rules` to an invoice. `caller` must be the creator and the invoice
/// must still be pending.
pub fn set_rules(env: &Env, caller: &Address, invoice_id: u64, rules: Vec<SplitRuleDef>) {
    caller.require_auth();
    let invoice = crate::load_invoice(env, invoice_id);
    assert!(invoice.creator == *caller, "only the creator can set split rules");
    assert!(
        invoice.status == InvoiceStatus::Pending,
        "split rules can only be set on pending invoices"
    );
    validate_rules(&rules, invoice.recipients.len());
    env.storage().persistent().set(&rules_key(invoice_id), &rules);
    events::split_rules_set(env, invoice_id, rules.len());
}

/// Remove an invoice's rules. `caller` must be the creator and the invoice
/// must still be pending.
pub fn clear_rules(env: &Env, caller: &Address, invoice_id: u64) {
    caller.require_auth();
    let invoice = crate::load_invoice(env, invoice_id);
    assert!(invoice.creator == *caller, "only the creator can clear split rules");
    assert!(
        invoice.status == InvoiceStatus::Pending,
        "split rules can only be cleared on pending invoices"
    );
    env.storage().persistent().remove(&rules_key(invoice_id));
    events::split_rules_set(env, invoice_id, 0);
}

/// Evaluate an invoice's stored rules against its current state.
pub fn evaluate_for_invoice(env: &Env, invoice_id: u64) -> Vec<i128> {
    let invoice = crate::load_invoice(env, invoice_id);
    let rules = get_rules(env, invoice_id);
    assert!(!rules.is_empty(), "no split rules set for invoice");
    let ctx = RuleContext {
        funded: invoice.funded,
        payer_count: invoice.payments.len(),
        timestamp: env.ledger().timestamp(),
    };
    evaluate(env, &rules, invoice.recipients.len(), &ctx)
}
