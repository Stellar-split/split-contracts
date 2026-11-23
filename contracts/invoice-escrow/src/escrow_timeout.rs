//! Escrow timeout and auto-release (issue #1218).
//!
//! An [`EscrowTimeout`] tracks funds held for an invoice and the period after
//! which they are released automatically. Funds can be released in parts
//! before the timeout; once it expires, [`EscrowTimeout::auto_release`]
//! releases whatever is still held, and [`EscrowTimeout::poll_notice`] reports
//! the expiry once so the caller can emit a notification. Not wired to
//! contract storage: callers persist the struct and pass the ledger time.

#![allow(dead_code)]

/// Errors returned by timeout operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeoutError {
    /// Amount is zero or negative.
    InvalidAmount,
    /// Timeout period is zero.
    InvalidTimeout,
    /// Release amount is larger than what is still held.
    ExceedsHeld,
    /// The timeout has not expired yet.
    NotExpired,
    /// Nothing is left to release.
    NothingToRelease,
}

/// Result of an automatic release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoRelease {
    /// Amount released by this call.
    pub amount: i128,
    /// `true` if part of the escrow had already been released before.
    pub after_partial: bool,
}

/// Notification raised once when the timeout expires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeoutNotice {
    /// Timestamp at which the timeout expired.
    pub expired_at: u64,
    /// Amount still held at expiry.
    pub outstanding: i128,
}

/// Timeout state for one escrow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EscrowTimeout {
    held: i128,
    released: i128,
    started_at: u64,
    timeout_secs: u64,
    notified: bool,
}

impl EscrowTimeout {
    /// Starts tracking `held` funds at `started_at`, expiring after
    /// `timeout_secs`.
    pub fn new(held: i128, started_at: u64, timeout_secs: u64) -> Result<Self, TimeoutError> {
        if held <= 0 {
            return Err(TimeoutError::InvalidAmount);
        }
        if timeout_secs == 0 {
            return Err(TimeoutError::InvalidTimeout);
        }
        Ok(Self {
            held,
            released: 0,
            started_at,
            timeout_secs,
            notified: false,
        })
    }

    /// Timestamp at which the timeout expires.
    pub fn expires_at(&self) -> u64 {
        self.started_at.saturating_add(self.timeout_secs)
    }

    /// `true` once `now` has reached the expiry.
    pub fn is_expired(&self, now: u64) -> bool {
        now >= self.expires_at()
    }

    /// Seconds until expiry (0 once expired).
    pub fn seconds_left(&self, now: u64) -> u64 {
        self.expires_at().saturating_sub(now)
    }

    /// Amount still held.
    pub fn remaining(&self) -> i128 {
        self.held - self.released
    }

    /// Amount released so far.
    pub fn released(&self) -> i128 {
        self.released
    }

    /// Releases `amount` of the held funds and returns what is still held.
    pub fn release_partial(&mut self, amount: i128) -> Result<i128, TimeoutError> {
        if amount <= 0 {
            return Err(TimeoutError::InvalidAmount);
        }
        if amount > self.remaining() {
            return Err(TimeoutError::ExceedsHeld);
        }
        self.released += amount;
        Ok(self.remaining())
    }

    /// Releases everything still held once the timeout has expired.
    pub fn auto_release(&mut self, now: u64) -> Result<AutoRelease, TimeoutError> {
        if !self.is_expired(now) {
            return Err(TimeoutError::NotExpired);
        }
        let amount = self.remaining();
        if amount == 0 {
            return Err(TimeoutError::NothingToRelease);
        }
        let after_partial = self.released > 0;
        self.released = self.held;
        Ok(AutoRelease {
            amount,
            after_partial,
        })
    }

    /// Returns the expiry notice the first time it is polled after expiry,
    /// then `None`.
    pub fn poll_notice(&mut self, now: u64) -> Option<TimeoutNotice> {
        if self.notified || !self.is_expired(now) {
            return None;
        }
        self.notified = true;
        Some(TimeoutNotice {
            expired_at: self.expires_at(),
            outstanding: self.remaining(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_timeout_period() {
        let t = EscrowTimeout::new(1_000, 100, 50).unwrap();
        assert_eq!(t.expires_at(), 150);
        assert!(!t.is_expired(149));
        assert!(t.is_expired(150));
        assert_eq!(t.seconds_left(120), 30);
        assert_eq!(t.seconds_left(200), 0);
    }

    #[test]
    fn auto_release_blocked_before_timeout() {
        let mut t = EscrowTimeout::new(1_000, 100, 50).unwrap();
        assert_eq!(t.auto_release(149), Err(TimeoutError::NotExpired));
        assert_eq!(t.remaining(), 1_000);
    }

    #[test]
    fn auto_release_after_timeout_releases_all() {
        let mut t = EscrowTimeout::new(1_000, 100, 50).unwrap();
        assert_eq!(
            t.auto_release(150),
            Ok(AutoRelease {
                amount: 1_000,
                after_partial: false
            })
        );
        assert_eq!(t.remaining(), 0);
        assert_eq!(t.auto_release(160), Err(TimeoutError::NothingToRelease));
    }

    #[test]
    fn partial_release_then_auto_release_rest() {
        let mut t = EscrowTimeout::new(1_000, 100, 50).unwrap();
        assert_eq!(t.release_partial(300), Ok(700));
        assert_eq!(
            t.auto_release(200),
            Ok(AutoRelease {
                amount: 700,
                after_partial: true
            })
        );
        assert_eq!(t.released(), 1_000);
    }

    #[test]
    fn notice_sent_once_on_timeout() {
        let mut t = EscrowTimeout::new(1_000, 100, 50).unwrap();
        t.release_partial(400).unwrap();
        assert_eq!(t.poll_notice(149), None);
        assert_eq!(
            t.poll_notice(155),
            Some(TimeoutNotice {
                expired_at: 150,
                outstanding: 600
            })
        );
        assert_eq!(t.poll_notice(160), None);
    }

    #[test]
    fn rejects_invalid_inputs() {
        assert_eq!(
            EscrowTimeout::new(0, 0, 10),
            Err(TimeoutError::InvalidAmount)
        );
        assert_eq!(
            EscrowTimeout::new(10, 0, 0),
            Err(TimeoutError::InvalidTimeout)
        );
        let mut t = EscrowTimeout::new(100, 0, 10).unwrap();
        assert_eq!(t.release_partial(0), Err(TimeoutError::InvalidAmount));
        assert_eq!(t.release_partial(101), Err(TimeoutError::ExceedsHeld));
        assert_eq!(t.remaining(), 100);
    }
}
