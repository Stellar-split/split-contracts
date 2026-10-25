//! StellarSplit Tax Strategy — on-chain creator tax optimisation.
//!
//! Creators configure how incoming payments are split before they land in
//! their wallet:
//!
//! 1. **Deduction** — an optional pre-tax share routed to a deductible
//!    bucket (retirement, business expenses, …). It lowers taxable income.
//! 2. **Tax reserve** — computed on the remaining taxable income using one
//!    of three strategies: a flat rate, creator-defined marginal brackets,
//!    or admin-published bracket templates (e.g. per jurisdiction).
//!    Brackets are applied to cumulative income within a tax period, so
//!    the reserve follows the marginal rate actually owed.
//! 3. **Net** — the remainder goes to the creator.
//!
//! Funds never sit in this contract: `process_income` pulls the gross
//! amount from the payer and pays all three destinations directly.
//! `estimate` and `estimate_config` let creators compare strategies before
//! committing to one. Every state change emits an event under the `tax` topic.

#![no_std]

mod errors;
mod types;

pub use errors::Error;
pub use types::{IncomeBreakdown, PeriodSummary, StrategyConfig, StrategyKind, TaxBracket, TaxStrategy};

use soroban_sdk::{contract, contractimpl, symbol_short, token, Address, Env, Symbol, Vec};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const BPS_DENOMINATOR: i128 = 10_000;
/// Highest reserve rate any bracket or flat strategy may use (60%).
pub const MAX_RATE_BPS: u32 = 6_000;
/// Highest pre-tax deduction share (30%).
pub const MAX_DEDUCTION_BPS: u32 = 3_000;
pub const MAX_BRACKETS: u32 = 10;
/// Roughly one week at 5s ledgers.
pub const MIN_PERIOD_LEDGERS: u32 = 120_960;
/// Roughly two years at 5s ledgers.
pub const MAX_PERIOD_LEDGERS: u32 = 12_614_400;

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

fn admin_key() -> Symbol {
    symbol_short!("admin")
}

fn template_key(name: &Symbol) -> (Symbol, Symbol) {
    (symbol_short!("tmpl"), name.clone())
}

fn strategy_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("strategy"), creator.clone())
}

fn period_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("period"), creator.clone())
}

fn last_period_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("last_prd"), creator.clone())
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

fn emit_template_set(env: &Env, name: &Symbol, brackets: u32) {
    env.events().publish(
        (symbol_short!("tax"), symbol_short!("tmpl_set"), name.clone()),
        brackets,
    );
}

fn emit_template_removed(env: &Env, name: &Symbol) {
    env.events().publish(
        (symbol_short!("tax"), symbol_short!("tmpl_del"), name.clone()),
        (),
    );
}

fn emit_strategy_set(env: &Env, creator: &Address, config: &StrategyConfig) {
    env.events().publish(
        (symbol_short!("tax"), symbol_short!("strategy"), creator.clone()),
        (config.kind, config.deduction_bps, config.period_ledgers),
    );
}

fn emit_strategy_removed(env: &Env, creator: &Address) {
    env.events().publish(
        (symbol_short!("tax"), symbol_short!("removed"), creator.clone()),
        (),
    );
}

fn emit_income(env: &Env, creator: &Address, token: &Address, b: &IncomeBreakdown) {
    env.events().publish(
        (symbol_short!("tax"), symbol_short!("income"), creator.clone()),
        (token.clone(), b.gross, b.deducted, b.reserved, b.net),
    );
}

fn emit_period_closed(env: &Env, creator: &Address, p: &PeriodSummary) {
    env.events().publish(
        (symbol_short!("tax"), symbol_short!("period"), creator.clone()),
        (p.start_ledger, p.gross_income, p.deducted, p.reserved, p.net_paid),
    );
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn require_admin(env: &Env, admin: &Address) -> Result<(), Error> {
    let stored: Address = env
        .storage()
        .instance()
        .get(&admin_key())
        .ok_or(Error::NotInitialized)?;
    if stored != *admin {
        return Err(Error::NotAdmin);
    }
    admin.require_auth();
    Ok(())
}

fn validate_brackets(brackets: &Vec<TaxBracket>) -> Result<(), Error> {
    if brackets.is_empty() || brackets.len() > MAX_BRACKETS {
        return Err(Error::InvalidBrackets);
    }
    let mut prev: Option<i128> = None;
    for b in brackets.iter() {
        if b.rate_bps > MAX_RATE_BPS {
            return Err(Error::InvalidRate);
        }
        match prev {
            None if b.threshold != 0 => return Err(Error::InvalidBrackets),
            Some(p) if b.threshold <= p => return Err(Error::InvalidBrackets),
            _ => {}
        }
        prev = Some(b.threshold);
    }
    Ok(())
}

fn validate_config(env: &Env, config: &StrategyConfig) -> Result<(), Error> {
    match config.kind {
        StrategyKind::Flat => {
            if config.flat_bps > MAX_RATE_BPS {
                return Err(Error::InvalidRate);
            }
        }
        StrategyKind::Progressive => validate_brackets(&config.brackets)?,
        StrategyKind::Template => {
            let name = config.template.as_ref().ok_or(Error::MissingTemplate)?;
            if !env.storage().persistent().has(&template_key(name)) {
                return Err(Error::TemplateNotFound);
            }
        }
    }
    if config.deduction_bps > MAX_DEDUCTION_BPS
        || (config.deduction_bps > 0 && config.deduction_wallet.is_none())
    {
        return Err(Error::InvalidDeduction);
    }
    if !(MIN_PERIOD_LEDGERS..=MAX_PERIOD_LEDGERS).contains(&config.period_ledgers) {
        return Err(Error::InvalidPeriod);
    }
    Ok(())
}

/// Marginal tax on income moving from `prior` to `prior + amount`.
fn marginal_tax(brackets: &Vec<TaxBracket>, prior: i128, amount: i128) -> Result<i128, Error> {
    let start = prior;
    let end = prior.checked_add(amount).ok_or(Error::Overflow)?;
    let mut tax: i128 = 0;
    let len = brackets.len();
    for i in 0..len {
        let b = brackets.get_unchecked(i);
        let upper = if i + 1 < len {
            brackets.get_unchecked(i + 1).threshold
        } else {
            i128::MAX
        };
        let lo = start.max(b.threshold);
        let hi = end.min(upper);
        if hi > lo {
            let slice_tax = (hi - lo)
                .checked_mul(b.rate_bps as i128)
                .ok_or(Error::Overflow)?
                / BPS_DENOMINATOR;
            tax = tax.checked_add(slice_tax).ok_or(Error::Overflow)?;
        }
    }
    Ok(tax)
}

fn load_template(env: &Env, name: &Symbol) -> Result<Vec<TaxBracket>, Error> {
    env.storage()
        .persistent()
        .get(&template_key(name))
        .ok_or(Error::TemplateNotFound)
}

fn compute_breakdown(
    env: &Env,
    config: &StrategyConfig,
    prior_taxable: i128,
    gross: i128,
) -> Result<IncomeBreakdown, Error> {
    if gross <= 0 {
        return Err(Error::InvalidAmount);
    }
    let deducted = gross
        .checked_mul(config.deduction_bps as i128)
        .ok_or(Error::Overflow)?
        / BPS_DENOMINATOR;
    let taxable = gross - deducted;
    let reserved = match config.kind {
        StrategyKind::Flat => {
            taxable
                .checked_mul(config.flat_bps as i128)
                .ok_or(Error::Overflow)?
                / BPS_DENOMINATOR
        }
        StrategyKind::Progressive => marginal_tax(&config.brackets, prior_taxable, taxable)?,
        StrategyKind::Template => {
            let name = config.template.as_ref().ok_or(Error::MissingTemplate)?;
            marginal_tax(&load_template(env, name)?, prior_taxable, taxable)?
        }
    };
    let net = taxable - reserved;
    let effective_rate_bps = {
        let numerator = reserved
            .checked_mul(BPS_DENOMINATOR)
            .ok_or(Error::Overflow)?;
        let rate = if gross > 0 {
            ((numerator as f64) / (gross as f64)).round() as i128
        } else {
            0
        };
        (rate.min(BPS_DENOMINATOR as i128)) as u32
    };
    Ok(IncomeBreakdown {
        gross,
        deducted,
        taxable,
        reserved,
        net,
        effective_rate_bps,
    })
}

fn new_period(start_ledger: u32) -> PeriodSummary {
    PeriodSummary {
        start_ledger,
        ..PeriodSummary::default()
    }
}

/// Loads the creator's current period, rolling it over (and archiving the
/// previous one) when `period_ledgers` have elapsed.
fn current_period(env: &Env, creator: &Address, period_ledgers: u32, roll: bool) -> PeriodSummary {
    let now = env.ledger().sequence();
    match env.storage().persistent().get::<_, PeriodSummary>(&period_key(creator)) {
        Some(p) if now < p.start_ledger.saturating_add(period_ledgers) => p,
        Some(p) => {
            if roll {
                env.storage().persistent().set(&last_period_key(creator), &p);
                emit_period_closed(env, creator, &p);
            }
            new_period(now)
        }
        None => new_period(now),
    }
}

fn load_strategy(env: &Env, creator: &Address) -> Result<TaxStrategy, Error> {
    env.storage()
        .persistent()
        .get(&strategy_key(creator))
        .ok_or(Error::StrategyNotFound)
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct TaxStrategyContract;

#[contractimpl]
impl TaxStrategyContract {
    /// Initialise the contract with an admin who publishes bracket templates.
    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&admin_key()) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&admin_key(), &admin);
        Ok(())
    }

    /// Admin-only: publish or replace a named bracket template. Creators on
    /// `Template` strategies pick up changes on their next payment.
    pub fn set_template(env: Env, admin: Address, name: Symbol, brackets: Vec<TaxBracket>) -> Result<(), Error> {
        require_admin(&env, &admin)?;
        validate_brackets(&brackets)?;
        env.storage().persistent().set(&template_key(&name), &brackets);
        emit_template_set(&env, &name, brackets.len());
        Ok(())
    }

    /// Admin-only: delete a template. Creators still pointing at it must
    /// switch strategy before receiving further income.
    pub fn remove_template(env: Env, admin: Address, name: Symbol) -> Result<(), Error> {
        require_admin(&env, &admin)?;
        let key = template_key(&name);
        if !env.storage().persistent().has(&key) {
            return Err(Error::TemplateNotFound);
        }
        env.storage().persistent().remove(&key);
        emit_template_removed(&env, &name);
        Ok(())
    }

    /// Creator sets or replaces their tax strategy. Current period progress is kept.
    pub fn set_strategy(env: Env, creator: Address, config: StrategyConfig) -> Result<(), Error> {
        creator.require_auth();
        if !env.storage().instance().has(&admin_key()) {
            return Err(Error::NotInitialized);
        }
        validate_config(&env, &config)?;
        let strategy = TaxStrategy {
            creator: creator.clone(),
            config: config.clone(),
            updated_at: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&strategy_key(&creator), &strategy);
        emit_strategy_set(&env, &creator, &config);
        Ok(())
    }

    /// Creator removes their strategy; income will no longer be routed here.
    pub fn remove_strategy(env: Env, creator: Address) -> Result<(), Error> {
        creator.require_auth();
        let key = strategy_key(&creator);
        if !env.storage().persistent().has(&key) {
            return Err(Error::StrategyNotFound);
        }
        env.storage().persistent().remove(&key);
        emit_strategy_removed(&env, &creator);
        Ok(())
    }

    /// Route `gross` of `token` from `payer` to `creator` according to the
    /// creator's strategy: deduction → deduction wallet, reserve → reserve
    /// wallet, remainder → creator. Returns the breakdown.
    pub fn process_income(
        env: Env,
        payer: Address,
        creator: Address,
        token: Address,
        gross: i128,
    ) -> Result<IncomeBreakdown, Error> {
        payer.require_auth();
        let strategy = load_strategy(&env, &creator)?;
        let config = &strategy.config;
        let mut period = current_period(&env, &creator, config.period_ledgers, true);
        let b = compute_breakdown(&env, config, period.taxable_income, gross)?;

        let client = token::Client::new(&env, &token);
        if b.deducted > 0 {
            if let Some(wallet) = &config.deduction_wallet {
                client.transfer(&payer, wallet, &b.deducted);
            }
        }
        if b.reserved > 0 {
            client.transfer(&payer, &config.reserve_wallet, &b.reserved);
        }
        if b.net > 0 {
            client.transfer(&payer, &creator, &b.net);
        }

        period.gross_income = period.gross_income.checked_add(b.gross).ok_or(Error::Overflow)?;
        period.taxable_income = period.taxable_income.checked_add(b.taxable).ok_or(Error::Overflow)?;
        period.deducted = period.deducted.checked_add(b.deducted).ok_or(Error::Overflow)?;
        period.reserved = period.reserved.checked_add(b.reserved).ok_or(Error::Overflow)?;
        period.net_paid = period.net_paid.checked_add(b.net).ok_or(Error::Overflow)?;
        env.storage().persistent().set(&period_key(&creator), &period);

        emit_income(&env, &creator, &token, &b);
        Ok(b)
    }

    // -----------------------------------------------------------------------
    // Views
    // -----------------------------------------------------------------------

    /// Breakdown `gross` would produce under the creator's current strategy,
    /// taking current period progress into account.
    pub fn estimate(env: Env, creator: Address, gross: i128) -> Result<IncomeBreakdown, Error> {
        let strategy = load_strategy(&env, &creator)?;
        let period = current_period(&env, &creator, strategy.config.period_ledgers, false);
        compute_breakdown(&env, &strategy.config, period.taxable_income, gross)
    }

    /// Breakdown `gross` would produce under a candidate `config`, using the
    /// creator's current period progress. Lets creators compare strategies.
    pub fn estimate_config(
        env: Env,
        creator: Address,
        config: StrategyConfig,
        gross: i128,
    ) -> Result<IncomeBreakdown, Error> {
        validate_config(&env, &config)?;
        let period = current_period(&env, &creator, config.period_ledgers, false);
        compute_breakdown(&env, &config, period.taxable_income, gross)
    }

    pub fn get_strategy(env: Env, creator: Address) -> Option<TaxStrategy> {
        env.storage().persistent().get(&strategy_key(&creator))
    }

    pub fn get_template(env: Env, name: Symbol) -> Option<Vec<TaxBracket>> {
        env.storage().persistent().get(&template_key(&name))
    }

    /// Totals for the creator's current period (zeroed if it has elapsed).
    pub fn get_period(env: Env, creator: Address) -> PeriodSummary {
        let period_ledgers = load_strategy(&env, &creator)
            .map(|s| s.config.period_ledgers)
            .unwrap_or(MAX_PERIOD_LEDGERS);
        current_period(&env, &creator, period_ledgers, false)
    }

    /// Totals for the creator's most recently closed period, if any.
    pub fn get_last_period(env: Env, creator: Address) -> Option<PeriodSummary> {
        env.storage().persistent().get(&last_period_key(&creator))
    }
}

#[cfg(test)]
mod test;
