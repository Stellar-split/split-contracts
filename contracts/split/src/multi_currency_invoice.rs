//! Multi-currency invoicing helpers (issue #1164), first slice.
//!
//! A fixed-capacity [`RateTable`] holds rates against a common base unit
//! (scaled by [`RATE_SCALE`]) and converts amounts between currencies with
//! checked arithmetic. Conversion rounds toward zero (the sub-unit remainder
//! is discarded, not accumulated). Not wired to contract storage or oracles.

#![allow(dead_code)]

/// Fixed-point scale for rates (7 decimals).
pub const RATE_SCALE: i128 = 10_000_000;
/// Maximum currencies tracked.
pub const MAX_CURRENCIES: usize = 8;

/// Errors returned by currency operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurrencyError {
    /// Rate is zero or negative.
    InvalidRate,
    /// Amount is negative.
    InvalidAmount,
    /// Currency is not in the table.
    UnknownCurrency,
    /// Table is full.
    TableFull,
    /// Rate is older than the allowed age.
    StaleRate,
    /// Arithmetic overflowed.
    Overflow,
}

/// Rate of one currency: base units per 1 unit, scaled by [`RATE_SCALE`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rate {
    /// Currency code (caller-defined identifier).
    pub code: u32,
    /// Scaled rate to the base unit.
    pub rate: i128,
    /// Ledger of the last update.
    pub updated_at: u32,
}

/// Conversion counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConversionStats {
    /// Successful conversions.
    pub conversions: u32,
    /// Rate updates applied.
    pub rate_updates: u32,
}

/// Table of currency rates.
#[derive(Clone, Copy, Debug)]
pub struct RateTable {
    rates: [Rate; MAX_CURRENCIES],
    len: usize,
    stats: ConversionStats,
}

impl Default for RateTable {
    fn default() -> Self {
        Self::new()
    }
}

impl RateTable {
    /// Creates an empty table.
    pub fn new() -> Self {
        Self {
            rates: [Rate { code: 0, rate: 0, updated_at: 0 }; MAX_CURRENCIES],
            len: 0,
            stats: ConversionStats { conversions: 0, rate_updates: 0 },
        }
    }

    /// Usage counters.
    pub fn stats(&self) -> ConversionStats {
        self.stats
    }

    /// Looks up the rate for `code`.
    pub fn get(&self, code: u32) -> Option<Rate> {
        self.rates[..self.len].iter().find(|r| r.code == code).copied()
    }

    /// Inserts or updates the rate for `code` at ledger `now`.
    pub fn set_rate(&mut self, code: u32, rate: i128, now: u32) -> Result<(), CurrencyError> {
        if rate <= 0 {
            return Err(CurrencyError::InvalidRate);
        }
        let entry = Rate { code, rate, updated_at: now };
        if let Some(slot) = self.rates[..self.len].iter_mut().find(|r| r.code == code) {
            *slot = entry;
        } else {
            if self.len == MAX_CURRENCIES {
                return Err(CurrencyError::TableFull);
            }
            self.rates[self.len] = entry;
            self.len += 1;
        }
        self.stats.rate_updates = self.stats.rate_updates.saturating_add(1);
        Ok(())
    }

    /// Converts `amount` from `from` to `to`, rounding toward zero. Fails with
    /// [`CurrencyError::StaleRate`] if either rate is older than `max_age`
    /// ledgers at `now` (`max_age == 0` disables the check).
    pub fn convert(
        &mut self,
        amount: i128,
        from: u32,
        to: u32,
        now: u32,
        max_age: u32,
    ) -> Result<i128, CurrencyError> {
        if amount < 0 {
            return Err(CurrencyError::InvalidAmount);
        }
        let f = self.get(from).ok_or(CurrencyError::UnknownCurrency)?;
        let t = self.get(to).ok_or(CurrencyError::UnknownCurrency)?;
        if max_age > 0 && (now.saturating_sub(f.updated_at) > max_age || now.saturating_sub(t.updated_at) > max_age) {
            return Err(CurrencyError::StaleRate);
        }
        let out = amount
            .checked_mul(f.rate)
            .ok_or(CurrencyError::Overflow)?
            / t.rate;
        self.stats.conversions = self.stats.conversions.saturating_add(1);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const USD: u32 = 1;
    const EUR: u32 = 2;

    fn table() -> RateTable {
        let mut t = RateTable::new();
        t.set_rate(USD, RATE_SCALE, 10).unwrap();
        t.set_rate(EUR, RATE_SCALE * 11 / 10, 10).unwrap();
        t
    }

    #[test]
    fn converts_between_currencies() {
        let mut t = table();
        assert_eq!(t.convert(1000, EUR, USD, 10, 0), Ok(1100));
        assert_eq!(t.convert(1100, USD, EUR, 10, 0), Ok(1000));
        assert_eq!(t.stats().conversions, 2);
    }

    #[test]
    fn rounds_toward_zero() {
        let mut t = table();
        assert_eq!(t.convert(1, USD, EUR, 10, 0), Ok(0));
    }

    #[test]
    fn rate_updates_and_errors() {
        let mut t = table();
        assert_eq!(t.set_rate(USD, 0, 11), Err(CurrencyError::InvalidRate));
        t.set_rate(EUR, RATE_SCALE * 2, 20).unwrap();
        assert_eq!(t.get(EUR).map(|r| r.rate), Some(RATE_SCALE * 2));
        assert_eq!(t.convert(-1, USD, EUR, 20, 0), Err(CurrencyError::InvalidAmount));
        assert_eq!(t.convert(1, USD, 99, 20, 0), Err(CurrencyError::UnknownCurrency));
        assert_eq!(t.convert(1, USD, EUR, 20, 5), Err(CurrencyError::StaleRate));
        assert_eq!(t.convert(i128::MAX, EUR, USD, 20, 0), Err(CurrencyError::Overflow));
    }

    #[test]
    fn table_capacity() {
        let mut t = RateTable::new();
        for c in 0..MAX_CURRENCIES as u32 {
            t.set_rate(c, RATE_SCALE, 0).unwrap();
        }
        assert_eq!(t.set_rate(100, RATE_SCALE, 0), Err(CurrencyError::TableFull));
    }
}
