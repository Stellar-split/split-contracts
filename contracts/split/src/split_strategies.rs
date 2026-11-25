//! Issue #1186: payment splitting strategies (first slice).
//!
//! Pure helpers: split an amount across recipients with an equal, weighted
//! or waterfall strategy, pick a strategy from conditional rules, and count
//! completed splits with [`SplitTracker`].

/// Maximum number of recipients per split.
pub const MAX_SPLIT_RECIPIENTS: usize = 50;

/// Maximum number of strategy rules evaluated per selection.
pub const MAX_STRATEGY_RULES: usize = 20;

const BPS_DENOMINATOR: i128 = 10_000;

/// How an amount is divided between recipients.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SplitStrategy {
    /// Same share for everyone; leftover units go to the earliest recipients.
    Equal,
    /// Share proportional to `params[i]` in bps (must sum to 10_000);
    /// rounding dust goes to the earliest recipients with a non-zero weight.
    Weighted,
    /// Recipients are filled in order, each up to its cap `params[i]`.
    Waterfall,
}

/// Errors for split helpers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SplitError {
    /// Amount must be positive.
    InvalidAmount,
    /// No recipients supplied.
    NoRecipients,
    /// More than [`MAX_SPLIT_RECIPIENTS`] recipients.
    TooManyRecipients,
    /// `params` and `out` lengths differ.
    LengthMismatch,
    /// Weights are negative or do not sum to 10_000.
    InvalidWeights,
    /// A waterfall cap is negative.
    InvalidCap,
    /// Too many rules supplied.
    TooManyRules,
    /// No rule matched.
    NoMatch,
    /// Arithmetic overflow.
    Overflow,
}

/// Condition under which a strategy rule applies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StrategyCondition {
    /// Always matches.
    Always,
    /// Matches when amount >= value.
    AmountAtLeast(i128),
    /// Matches when amount < value.
    AmountBelow(i128),
}

/// Chooses `strategy` when `condition` holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StrategyRule {
    /// Condition to evaluate.
    pub condition: StrategyCondition,
    /// Strategy used when matched.
    pub strategy: SplitStrategy,
}

/// Returns true when `condition` holds for `amount`.
pub fn condition_matches(condition: StrategyCondition, amount: i128) -> bool {
    match condition {
        StrategyCondition::Always => true,
        StrategyCondition::AmountAtLeast(v) => amount >= v,
        StrategyCondition::AmountBelow(v) => amount < v,
    }
}

/// Returns the strategy of the first rule whose condition matches `amount`.
pub fn select_strategy(rules: &[StrategyRule], amount: i128) -> Result<SplitStrategy, SplitError> {
    if rules.len() > MAX_STRATEGY_RULES {
        return Err(SplitError::TooManyRules);
    }
    rules
        .iter()
        .find(|r| condition_matches(r.condition, amount))
        .map(|r| r.strategy)
        .ok_or(SplitError::NoMatch)
}

/// Splits `amount` into `out` using `strategy`. `params` holds the weights
/// (Weighted) or caps (Waterfall) and is ignored for Equal, but must have the
/// same length as `out`. Returns the undistributed remainder, which is only
/// non-zero for Waterfall when the caps total less than `amount`.
pub fn apply_strategy(
    strategy: SplitStrategy,
    amount: i128,
    params: &[i128],
    out: &mut [i128],
) -> Result<i128, SplitError> {
    if amount <= 0 {
        return Err(SplitError::InvalidAmount);
    }
    let n = out.len();
    if n == 0 {
        return Err(SplitError::NoRecipients);
    }
    if n > MAX_SPLIT_RECIPIENTS {
        return Err(SplitError::TooManyRecipients);
    }
    if params.len() != n {
        return Err(SplitError::LengthMismatch);
    }
    match strategy {
        SplitStrategy::Equal => {
            let base = amount / n as i128;
            let extra = (amount % n as i128) as usize;
            for (i, slot) in out.iter_mut().enumerate() {
                *slot = if i < extra { base + 1 } else { base };
            }
            Ok(0)
        }
        SplitStrategy::Weighted => {
            let mut total_weight: i128 = 0;
            for w in params {
                if *w < 0 {
                    return Err(SplitError::InvalidWeights);
                }
                total_weight = total_weight.checked_add(*w).ok_or(SplitError::InvalidWeights)?;
            }
            if total_weight != BPS_DENOMINATOR {
                return Err(SplitError::InvalidWeights);
            }
            let mut assigned: i128 = 0;
            for (slot, w) in out.iter_mut().zip(params) {
                *slot = amount.checked_mul(*w).ok_or(SplitError::Overflow)? / BPS_DENOMINATOR;
                assigned += *slot;
            }
            // Dust is below the number of non-zero weights, so one pass suffices.
            let mut dust = amount - assigned;
            for (slot, w) in out.iter_mut().zip(params) {
                if dust == 0 {
                    break;
                }
                if *w > 0 {
                    *slot += 1;
                    dust -= 1;
                }
            }
            Ok(0)
        }
        SplitStrategy::Waterfall => {
            let mut remaining = amount;
            for (slot, cap) in out.iter_mut().zip(params) {
                if *cap < 0 {
                    return Err(SplitError::InvalidCap);
                }
                *slot = remaining.min(*cap);
                remaining -= *slot;
            }
            Ok(remaining)
        }
    }
}

/// Running totals of completed splits.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SplitTracker {
    /// Number of splits recorded.
    pub splits: u32,
    /// Total amount paid to recipients.
    pub distributed: i128,
    /// Total amount left undistributed.
    pub undistributed: i128,
}

impl SplitTracker {
    /// Records one split of `amount` that left `undistributed` unassigned (saturating).
    pub fn record(&mut self, amount: i128, undistributed: i128) {
        self.splits = self.splits.saturating_add(1);
        self.distributed = self.distributed.saturating_add(amount - undistributed);
        self.undistributed = self.undistributed.saturating_add(undistributed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_split_spreads_dust() {
        let mut out = [0i128; 3];
        assert_eq!(apply_strategy(SplitStrategy::Equal, 10, &[0, 0, 0], &mut out), Ok(0));
        assert_eq!(out, [4, 3, 3]);
    }

    #[test]
    fn weighted_split() {
        let mut out = [0i128; 3];
        let weights = [5_000, 3_000, 2_000];
        assert_eq!(apply_strategy(SplitStrategy::Weighted, 101, &weights, &mut out), Ok(0));
        assert_eq!(out.iter().sum::<i128>(), 101);
        assert_eq!(out, [51, 30, 20]);
        assert_eq!(
            apply_strategy(SplitStrategy::Weighted, 100, &[5_000, 4_000, 0], &mut out),
            Err(SplitError::InvalidWeights)
        );
    }

    #[test]
    fn waterfall_split() {
        let mut out = [0i128; 2];
        assert_eq!(apply_strategy(SplitStrategy::Waterfall, 100, &[60, 30], &mut out), Ok(10));
        assert_eq!(out, [60, 30]);
        assert_eq!(apply_strategy(SplitStrategy::Waterfall, 50, &[60, 30], &mut out), Ok(0));
        assert_eq!(out, [50, 0]);
    }

    #[test]
    fn input_errors() {
        let mut out = [0i128; 2];
        assert_eq!(
            apply_strategy(SplitStrategy::Equal, 0, &[0, 0], &mut out),
            Err(SplitError::InvalidAmount)
        );
        assert_eq!(
            apply_strategy(SplitStrategy::Equal, 10, &[0], &mut out),
            Err(SplitError::LengthMismatch)
        );
        assert_eq!(
            apply_strategy(SplitStrategy::Equal, 10, &[], &mut []),
            Err(SplitError::NoRecipients)
        );
    }

    #[test]
    fn rule_selection_and_tracking() {
        let rules = [
            StrategyRule { condition: StrategyCondition::AmountAtLeast(1_000), strategy: SplitStrategy::Waterfall },
            StrategyRule { condition: StrategyCondition::Always, strategy: SplitStrategy::Equal },
        ];
        assert_eq!(select_strategy(&rules, 1_000), Ok(SplitStrategy::Waterfall));
        assert_eq!(select_strategy(&rules, 10), Ok(SplitStrategy::Equal));
        assert_eq!(select_strategy(&rules[..1], 10), Err(SplitError::NoMatch));

        let mut t = SplitTracker::default();
        t.record(100, 10);
        assert_eq!(t, SplitTracker { splits: 1, distributed: 90, undistributed: 10 });
    }
}
