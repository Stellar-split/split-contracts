//! Multi-token payment distribution (issue #1217).
//!
//! Splits payments in several token assets across the same recipients in one
//! call. Each asset carries its own platform fee (in basis points) and the
//! balance available to pay it; every balance is checked before anything is
//! computed, so either all assets distribute or none do. The net amount of
//! each asset is split by the recipients' shares with the largest-remainder
//! method used in `calc.rs`, so each asset's payouts sum to its net exactly.
//! Not wired to contract storage or token transfers.

#![allow(dead_code)]

use crate::calc::calc_platform_fee;
use crate::constants::{MAX_PLATFORM_FEE_BPS, TOTAL_BPS};
use crate::math::{mul_div, mul_mod, BPS_DENOMINATOR};

/// Maximum token assets in one distribution.
pub const MAX_TOKENS: usize = 4;
/// Maximum recipients in one distribution.
pub const MAX_RECIPIENTS: usize = 8;

/// Errors returned by distribution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistributionError {
    /// No tokens, or more than [`MAX_TOKENS`].
    InvalidTokenCount,
    /// No recipients, more than [`MAX_RECIPIENTS`], or shares not summing to
    /// 10 000 bps.
    InvalidShares,
    /// The same token appears twice.
    DuplicateToken,
    /// Amount is zero or negative.
    InvalidAmount,
    /// Fee is above `MAX_PLATFORM_FEE_BPS`.
    InvalidFee,
    /// Balance is lower than the amount to distribute.
    InsufficientBalance,
    /// Arithmetic overflowed.
    Overflow,
}

/// One token asset to distribute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenPayment {
    /// Token identifier (caller-defined).
    pub token: u32,
    /// Gross amount to distribute.
    pub amount: i128,
    /// Platform fee for this token, in basis points.
    pub fee_bps: u32,
    /// Balance available for this token.
    pub balance: i128,
}

/// Distribution result for one token asset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenPayout {
    /// Token identifier.
    pub token: u32,
    /// Fee deducted from the gross amount.
    pub fee: i128,
    /// Amount split across recipients (gross minus fee).
    pub net: i128,
    shares: [i128; MAX_RECIPIENTS],
    recipients: usize,
}

impl TokenPayout {
    /// Payout per recipient, in the order of the shares passed in.
    pub fn shares(&self) -> &[i128] {
        &self.shares[..self.recipients]
    }
}

/// Payouts for every token asset of a distribution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Distribution {
    payouts: [TokenPayout; MAX_TOKENS],
    len: usize,
}

impl Distribution {
    /// Payouts in the order the tokens were passed in.
    pub fn payouts(&self) -> &[TokenPayout] {
        &self.payouts[..self.len]
    }

    /// Payout for `token`, if it was part of the distribution.
    pub fn get(&self, token: u32) -> Option<&TokenPayout> {
        self.payouts().iter().find(|p| p.token == token)
    }

    /// Total fee deducted for `token` (0 if absent).
    pub fn fee_for(&self, token: u32) -> i128 {
        self.get(token).map(|p| p.fee).unwrap_or(0)
    }
}

/// Validates every token's amount, fee and balance against `shares_bps`.
pub fn validate(payments: &[TokenPayment], shares_bps: &[u32]) -> Result<(), DistributionError> {
    if payments.is_empty() || payments.len() > MAX_TOKENS {
        return Err(DistributionError::InvalidTokenCount);
    }
    if shares_bps.is_empty() || shares_bps.len() > MAX_RECIPIENTS {
        return Err(DistributionError::InvalidShares);
    }
    let total: u32 = shares_bps
        .iter()
        .fold(0u32, |acc, s| acc.saturating_add(*s));
    if total != TOTAL_BPS {
        return Err(DistributionError::InvalidShares);
    }
    for (i, p) in payments.iter().enumerate() {
        if payments[..i].iter().any(|q| q.token == p.token) {
            return Err(DistributionError::DuplicateToken);
        }
        if p.amount <= 0 {
            return Err(DistributionError::InvalidAmount);
        }
        if p.fee_bps > MAX_PLATFORM_FEE_BPS {
            return Err(DistributionError::InvalidFee);
        }
        if p.balance < p.amount {
            return Err(DistributionError::InsufficientBalance);
        }
    }
    Ok(())
}

/// Distributes every token in `payments` across recipients by `shares_bps`,
/// after deducting each token's fee.
pub fn distribute(
    payments: &[TokenPayment],
    shares_bps: &[u32],
) -> Result<Distribution, DistributionError> {
    validate(payments, shares_bps)?;
    let empty = TokenPayout {
        token: 0,
        fee: 0,
        net: 0,
        shares: [0; MAX_RECIPIENTS],
        recipients: 0,
    };
    let mut out = Distribution {
        payouts: [empty; MAX_TOKENS],
        len: payments.len(),
    };
    for (i, p) in payments.iter().enumerate() {
        let fee =
            calc_platform_fee(p.amount, p.fee_bps).map_err(|_| DistributionError::Overflow)?;
        let net = p.amount - fee;
        out.payouts[i] = TokenPayout {
            token: p.token,
            fee,
            net,
            shares: split_net(net, shares_bps)?,
            recipients: shares_bps.len(),
        };
    }
    Ok(out)
}

/// Largest-remainder split of `net` by `shares_bps`; ties go to the lower
/// index.
fn split_net(net: i128, shares_bps: &[u32]) -> Result<[i128; MAX_RECIPIENTS], DistributionError> {
    let mut shares = [0i128; MAX_RECIPIENTS];
    let mut rems = [0i128; MAX_RECIPIENTS];
    let mut assigned = 0i128;
    for (i, s) in shares_bps.iter().enumerate() {
        shares[i] =
            mul_div(net, *s as i128, BPS_DENOMINATOR).map_err(|_| DistributionError::Overflow)?;
        rems[i] =
            mul_mod(net, *s as i128, BPS_DENOMINATOR).map_err(|_| DistributionError::Overflow)?;
        assigned += shares[i];
    }
    let mut leftover = net - assigned;
    let mut given = [false; MAX_RECIPIENTS];
    while leftover > 0 {
        let mut best = 0usize;
        let mut found = false;
        for i in 0..shares_bps.len() {
            if !given[i] && (!found || rems[i] > rems[best]) {
                best = i;
                found = true;
            }
        }
        if !found {
            break;
        }
        shares[best] += 1;
        given[best] = true;
        leftover -= 1;
    }
    Ok(shares)
}

#[cfg(test)]
mod tests {
    use super::*;

    const XLM: u32 = 1;
    const USDC: u32 = 2;

    fn pay(token: u32, amount: i128, fee_bps: u32) -> TokenPayment {
        TokenPayment {
            token,
            amount,
            fee_bps,
            balance: amount,
        }
    }

    #[test]
    fn distributes_each_token() {
        let d = distribute(&[pay(XLM, 1_000, 0), pay(USDC, 2_000, 0)], &[6_000, 4_000]).unwrap();
        assert_eq!(d.payouts().len(), 2);
        assert_eq!(d.get(XLM).unwrap().shares(), &[600, 400]);
        assert_eq!(d.get(USDC).unwrap().shares(), &[1_200, 800]);
    }

    #[test]
    fn fees_calculated_per_token() {
        let d = distribute(&[pay(XLM, 10_000, 100), pay(USDC, 10_000, 250)], &[10_000]).unwrap();
        assert_eq!(d.fee_for(XLM), 100);
        assert_eq!(d.fee_for(USDC), 250);
        assert_eq!(d.get(XLM).unwrap().net, 9_900);
        assert_eq!(d.get(USDC).unwrap().shares(), &[9_750]);
    }

    #[test]
    fn payouts_sum_to_net() {
        let d = distribute(&[pay(XLM, 10, 0)], &[3_334, 3_333, 3_333]).unwrap();
        let p = d.get(XLM).unwrap();
        assert_eq!(p.shares(), &[4, 3, 3]);
        assert_eq!(p.shares().iter().sum::<i128>(), p.net);
    }

    #[test]
    fn insufficient_balance_rejected() {
        let mut short = pay(USDC, 500, 0);
        short.balance = 499;
        assert_eq!(
            distribute(&[pay(XLM, 100, 0), short], &[10_000]),
            Err(DistributionError::InsufficientBalance)
        );
    }

    #[test]
    fn invalid_shares_rejected() {
        let p = [pay(XLM, 100, 0)];
        assert_eq!(
            distribute(&p, &[5_000]),
            Err(DistributionError::InvalidShares)
        );
        assert_eq!(distribute(&p, &[]), Err(DistributionError::InvalidShares));
        assert_eq!(
            distribute(&p, &[1_250; MAX_RECIPIENTS + 1]),
            Err(DistributionError::InvalidShares)
        );
    }

    #[test]
    fn duplicate_and_token_count_rejected() {
        assert_eq!(
            distribute(&[pay(XLM, 1, 0), pay(XLM, 2, 0)], &[10_000]),
            Err(DistributionError::DuplicateToken)
        );
        assert_eq!(
            distribute(&[], &[10_000]),
            Err(DistributionError::InvalidTokenCount)
        );
        let many = [
            pay(1, 1, 0),
            pay(2, 1, 0),
            pay(3, 1, 0),
            pay(4, 1, 0),
            pay(5, 1, 0),
        ];
        assert_eq!(
            distribute(&many, &[10_000]),
            Err(DistributionError::InvalidTokenCount)
        );
    }

    #[test]
    fn invalid_amount_and_fee_rejected() {
        assert_eq!(
            distribute(&[pay(XLM, 0, 0)], &[10_000]),
            Err(DistributionError::InvalidAmount)
        );
        assert_eq!(
            distribute(&[pay(XLM, 100, MAX_PLATFORM_FEE_BPS + 1)], &[10_000]),
            Err(DistributionError::InvalidFee)
        );
    }

    #[test]
    fn single_token_matches_multi_token() {
        let one = distribute(&[pay(USDC, 7_777, 30)], &[2_500, 7_500]).unwrap();
        let both =
            distribute(&[pay(XLM, 1_234, 0), pay(USDC, 7_777, 30)], &[2_500, 7_500]).unwrap();
        assert_eq!(one.get(USDC), both.get(USDC));
    }
}
