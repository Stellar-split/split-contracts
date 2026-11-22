//! Shared types for the tax-strategy contract.

use soroban_sdk::{contracttype, Address, Symbol, Vec};

/// How the tax reserve is computed on taxable income.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StrategyKind {
    /// A single flat rate on all taxable income.
    Flat,
    /// Creator-defined marginal brackets on cumulative period income.
    Progressive,
    /// Marginal brackets from an admin-published template (e.g. a jurisdiction).
    Template,
}

/// A marginal bracket: `rate_bps` applies to period income at or above
/// `threshold`, up to the next bracket's threshold.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaxBracket {
    pub threshold: i128,
    pub rate_bps: u32,
}

/// Creator-supplied configuration for `set_strategy`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StrategyConfig {
    pub kind: StrategyKind,
    /// Rate for `Flat` strategies; ignored otherwise.
    pub flat_bps: u32,
    /// Brackets for `Progressive` strategies; ignored otherwise.
    pub brackets: Vec<TaxBracket>,
    /// Template name for `Template` strategies; ignored otherwise.
    pub template: Option<Symbol>,
    /// Wallet that receives the tax reserve.
    pub reserve_wallet: Address,
    /// Pre-tax share routed to a deductible bucket (retirement, business
    /// expenses, …) before the tax reserve is computed.
    pub deduction_bps: u32,
    pub deduction_wallet: Option<Address>,
    /// Length of a tax period in ledgers; bracket progress resets each period.
    pub period_ledgers: u32,
}

/// Stored strategy for a creator.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaxStrategy {
    pub creator: Address,
    pub config: StrategyConfig,
    pub updated_at: u64,
}

/// Running totals for the creator's current tax period.
#[contracttype]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PeriodSummary {
    pub start_ledger: u32,
    pub gross_income: i128,
    /// Taxable income (gross minus deductions) — drives bracket progress.
    pub taxable_income: i128,
    pub deducted: i128,
    pub reserved: i128,
    pub net_paid: i128,
}

/// How a single income payment is split.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncomeBreakdown {
    pub gross: i128,
    pub deducted: i128,
    pub taxable: i128,
    pub reserved: i128,
    pub net: i128,
    /// Effective reserve rate on the gross amount, in bps.
    pub effective_rate_bps: u32,
}
