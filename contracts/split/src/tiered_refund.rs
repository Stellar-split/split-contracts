//! Issue #1183: tiered refund processing (first slice).
//!
//! Pure helpers: a refund schedule whose tiers lower the refundable share as
//! time since payment grows, processing rules (fee, minimum, cap) applied on
//! top of the tier, a quote calculator and refund counters.

/// Maximum number of tiers in a schedule.
pub const MAX_REFUND_TIERS: usize = 10;

const BPS_DENOMINATOR: i128 = 10_000;

/// One refund tier: from `min_elapsed` seconds after payment, the payer may
/// recover `refund_bps` of the amount paid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RefundTier {
    /// Seconds since payment at which this tier starts.
    pub min_elapsed: u64,
    /// Refundable share in bps.
    pub refund_bps: u32,
}

/// Default schedule: full refund on day 0, 75% after 1 day, 50% after 7
/// days, nothing after 30 days.
pub const DEFAULT_REFUND_TIERS: [RefundTier; 4] = [
    RefundTier { min_elapsed: 0, refund_bps: 10_000 },
    RefundTier { min_elapsed: 86_400, refund_bps: 7_500 },
    RefundTier { min_elapsed: 604_800, refund_bps: 5_000 },
    RefundTier { min_elapsed: 2_592_000, refund_bps: 0 },
];

/// Processing rules applied after the tier share.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RefundRules {
    /// Processing fee in bps of the tier amount, kept by the contract.
    pub fee_bps: u32,
    /// Net refunds below this are rejected.
    pub min_refund: i128,
    /// Upper bound on the net refund, if any.
    pub max_refund: Option<i128>,
}

impl Default for RefundRules {
    fn default() -> Self {
        RefundRules { fee_bps: 0, min_refund: 1, max_refund: None }
    }
}

/// Errors for refund helpers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefundError {
    /// Schedule is empty or has more than [`MAX_REFUND_TIERS`] tiers.
    InvalidTierCount,
    /// Tier starts are not strictly increasing.
    UnorderedTiers,
    /// A tier's bps exceeds 10_000 or is higher than an earlier tier's.
    InvalidTierBps,
    /// Rules are out of range.
    InvalidRules,
    /// Paid amount must be positive.
    InvalidAmount,
    /// No tier covers the elapsed time.
    NoTier,
    /// Net refund is below `min_refund`.
    BelowMinimum,
    /// Arithmetic overflow.
    Overflow,
}

/// Checks that tiers start in strictly increasing order and that the
/// refundable share never rises over time.
pub fn validate_tiers(tiers: &[RefundTier]) -> Result<(), RefundError> {
    if tiers.is_empty() || tiers.len() > MAX_REFUND_TIERS {
        return Err(RefundError::InvalidTierCount);
    }
    for (i, tier) in tiers.iter().enumerate() {
        if tier.refund_bps as i128 > BPS_DENOMINATOR {
            return Err(RefundError::InvalidTierBps);
        }
        if i > 0 {
            let prev = tiers[i - 1];
            if tier.min_elapsed <= prev.min_elapsed {
                return Err(RefundError::UnorderedTiers);
            }
            if tier.refund_bps > prev.refund_bps {
                return Err(RefundError::InvalidTierBps);
            }
        }
    }
    Ok(())
}

/// Checks the processing rules.
pub fn validate_rules(rules: &RefundRules) -> Result<(), RefundError> {
    if rules.fee_bps as i128 > BPS_DENOMINATOR || rules.min_refund < 0 {
        return Err(RefundError::InvalidRules);
    }
    if let Some(max) = rules.max_refund {
        if max < rules.min_refund {
            return Err(RefundError::InvalidRules);
        }
    }
    Ok(())
}

/// Index of the tier that applies after `elapsed` seconds: the last tier
/// whose `min_elapsed` is `<= elapsed`.
pub fn tier_for(tiers: &[RefundTier], elapsed: u64) -> Option<usize> {
    tiers.iter().rposition(|t| t.min_elapsed <= elapsed)
}

/// Breakdown of a refund.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RefundQuote {
    /// Index of the applied tier.
    pub tier_index: u32,
    /// Amount allowed by the tier before fees.
    pub gross: i128,
    /// Processing fee withheld.
    pub fee: i128,
    /// Amount returned to the payer.
    pub net: i128,
}

/// Computes the refund for `paid` after `elapsed` seconds under `tiers` and
/// `rules`. All divisions round down, in the contract's favour by at most
/// one unit each.
pub fn quote_refund(
    tiers: &[RefundTier],
    rules: &RefundRules,
    paid: i128,
    elapsed: u64,
) -> Result<RefundQuote, RefundError> {
    validate_tiers(tiers)?;
    validate_rules(rules)?;
    if paid <= 0 {
        return Err(RefundError::InvalidAmount);
    }
    let index = tier_for(tiers, elapsed).ok_or(RefundError::NoTier)?;
    let gross = paid
        .checked_mul(tiers[index].refund_bps as i128)
        .ok_or(RefundError::Overflow)?
        / BPS_DENOMINATOR;
    let fee = gross
        .checked_mul(rules.fee_bps as i128)
        .ok_or(RefundError::Overflow)?
        / BPS_DENOMINATOR;
    let mut net = gross - fee;
    if let Some(max) = rules.max_refund {
        net = net.min(max);
    }
    if net < rules.min_refund {
        return Err(RefundError::BelowMinimum);
    }
    Ok(RefundQuote { tier_index: index as u32, gross, fee, net })
}

/// Refund counters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RefundTracker {
    /// Refunds processed.
    pub processed: u32,
    /// Refund requests rejected.
    pub rejected: u32,
    /// Total returned to payers.
    pub total_refunded: i128,
    /// Total fees withheld.
    pub total_fees: i128,
}

impl RefundTracker {
    /// Records the outcome of one refund request (saturating).
    pub fn record(&mut self, outcome: &Result<RefundQuote, RefundError>) {
        match outcome {
            Ok(q) => {
                self.processed = self.processed.saturating_add(1);
                self.total_refunded = self.total_refunded.saturating_add(q.net);
                self.total_fees = self.total_fees.saturating_add(q.fee);
            }
            Err(_) => self.rejected = self.rejected.saturating_add(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_schedule_tiers() {
        let rules = RefundRules::default();
        let q = quote_refund(&DEFAULT_REFUND_TIERS, &rules, 1_000, 0).unwrap();
        assert_eq!((q.tier_index, q.net), (0, 1_000));
        let q = quote_refund(&DEFAULT_REFUND_TIERS, &rules, 1_000, 86_400).unwrap();
        assert_eq!((q.tier_index, q.net), (1, 750));
        let q = quote_refund(&DEFAULT_REFUND_TIERS, &rules, 1_000, 700_000).unwrap();
        assert_eq!((q.tier_index, q.net), (2, 500));
        assert_eq!(
            quote_refund(&DEFAULT_REFUND_TIERS, &rules, 1_000, 2_592_000),
            Err(RefundError::BelowMinimum)
        );
    }

    #[test]
    fn rules_apply_fee_and_cap() {
        let rules = RefundRules { fee_bps: 100, min_refund: 10, max_refund: Some(700) };
        let q = quote_refund(&DEFAULT_REFUND_TIERS, &rules, 1_000, 0).unwrap();
        assert_eq!(q, RefundQuote { tier_index: 0, gross: 1_000, fee: 10, net: 700 });
        let bad = RefundRules { fee_bps: 10_001, ..rules };
        assert_eq!(
            quote_refund(&DEFAULT_REFUND_TIERS, &bad, 1_000, 0),
            Err(RefundError::InvalidRules)
        );
    }

    #[test]
    fn custom_schedule_validation() {
        let late_start = [RefundTier { min_elapsed: 60, refund_bps: 5_000 }];
        assert_eq!(
            quote_refund(&late_start, &RefundRules::default(), 100, 30),
            Err(RefundError::NoTier)
        );
        let rising = [
            RefundTier { min_elapsed: 0, refund_bps: 5_000 },
            RefundTier { min_elapsed: 10, refund_bps: 6_000 },
        ];
        assert_eq!(validate_tiers(&rising), Err(RefundError::InvalidTierBps));
        let unordered = [
            RefundTier { min_elapsed: 10, refund_bps: 5_000 },
            RefundTier { min_elapsed: 10, refund_bps: 4_000 },
        ];
        assert_eq!(validate_tiers(&unordered), Err(RefundError::UnorderedTiers));
        assert_eq!(validate_tiers(&[]), Err(RefundError::InvalidTierCount));
    }

    #[test]
    fn tracker_counts_outcomes() {
        let mut t = RefundTracker::default();
        t.record(&Ok(RefundQuote { tier_index: 0, gross: 100, fee: 1, net: 99 }));
        t.record(&Err(RefundError::NoTier));
        assert_eq!(t, RefundTracker { processed: 1, rejected: 1, total_refunded: 99, total_fees: 1 });
    }
}
