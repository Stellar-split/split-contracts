//! Issue #1185: invoice delegation with permission levels (first slice).
//!
//! Extends the single all-rights invoice delegate from issue #43 with
//! per-delegate [`DelegationLevel`]s, optional expiry, revocation and
//! tracking. [`DelegationRegistry`] is a fixed-size table keyed by invoice id
//! and delegate id; [`DelegationRegistry::authorize`] checks a
//! [`DelegatedAction`] against the delegate's active level.

#![allow(dead_code)]

/// Maximum delegations held by a [`DelegationRegistry`].
pub const MAX_DELEGATIONS: usize = 16;

/// Permission levels, ordered lowest to highest.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DelegationLevel {
    /// Read invoice details.
    View = 0,
    /// Day-to-day management (deadline, metadata).
    Manage = 1,
    /// Everything the creator can do except delegating further.
    Full = 2,
}

/// Actions a delegate may perform on an invoice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DelegatedAction {
    /// Read invoice details.
    ViewInvoice,
    /// Extend the invoice deadline.
    ExtendDeadline,
    /// Update invoice metadata.
    UpdateMetadata,
    /// Cancel the invoice.
    CancelInvoice,
}

/// Errors returned by delegation operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DelegationError {
    /// The delegate's level does not allow the action, or it has no active delegation.
    Unauthorized,
    /// An active delegation already exists for this invoice and delegate.
    AlreadyDelegated,
    /// No active delegation for this invoice and delegate.
    NotFound,
    /// The expiry is not after the grant time.
    InvalidExpiry,
    /// Registry is full.
    RegistryFull,
}

/// One delegation of an invoice to a delegate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Delegation {
    /// Invoice identifier.
    pub invoice_id: u64,
    /// Delegate identifier.
    pub delegate: u64,
    /// Granted level.
    pub level: DelegationLevel,
    /// Grant time.
    pub granted_at: u64,
    /// Expiry time; 0 means no expiry.
    pub expires_at: u64,
}

impl Delegation {
    /// True when the delegation has not expired at `now`.
    pub fn is_active(&self, now: u64) -> bool {
        self.expires_at == 0 || now < self.expires_at
    }
}

/// Minimum level required for an action.
pub fn required_level(action: DelegatedAction) -> DelegationLevel {
    match action {
        DelegatedAction::ViewInvoice => DelegationLevel::View,
        DelegatedAction::ExtendDeadline | DelegatedAction::UpdateMetadata => {
            DelegationLevel::Manage
        }
        DelegatedAction::CancelInvoice => DelegationLevel::Full,
    }
}

/// Fixed-size delegation table with grant/revoke counters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DelegationRegistry {
    entries: [Option<Delegation>; MAX_DELEGATIONS],
    /// Delegations granted (saturating).
    pub granted: u32,
    /// Delegations revoked (saturating).
    pub revoked: u32,
}

impl DelegationRegistry {
    fn position(&self, invoice_id: u64, delegate: u64) -> Option<usize> {
        self.entries.iter().position(|e| {
            matches!(e, Some(d) if d.invoice_id == invoice_id && d.delegate == delegate)
        })
    }

    /// Grants `level` on `invoice_id` to `delegate`. An expired entry for the
    /// same pair is replaced.
    pub fn grant(
        &mut self,
        invoice_id: u64,
        delegate: u64,
        level: DelegationLevel,
        now: u64,
        expires_at: u64,
    ) -> Result<(), DelegationError> {
        if expires_at != 0 && expires_at <= now {
            return Err(DelegationError::InvalidExpiry);
        }
        let slot = match self.position(invoice_id, delegate) {
            Some(i) if self.entries[i].map(|d| d.is_active(now)).unwrap_or(false) => {
                return Err(DelegationError::AlreadyDelegated);
            }
            Some(i) => i,
            None => self
                .entries
                .iter()
                .position(|e| e.is_none())
                .ok_or(DelegationError::RegistryFull)?,
        };
        self.entries[slot] =
            Some(Delegation { invoice_id, delegate, level, granted_at: now, expires_at });
        self.granted = self.granted.saturating_add(1);
        Ok(())
    }

    /// Revokes the delegation for `invoice_id` and `delegate`.
    pub fn revoke(&mut self, invoice_id: u64, delegate: u64) -> Result<(), DelegationError> {
        let i = self.position(invoice_id, delegate).ok_or(DelegationError::NotFound)?;
        self.entries[i] = None;
        self.revoked = self.revoked.saturating_add(1);
        Ok(())
    }

    /// Revokes every delegation on `invoice_id`; returns how many were removed.
    pub fn revoke_all(&mut self, invoice_id: u64) -> u32 {
        let mut removed = 0u32;
        for e in self.entries.iter_mut() {
            if matches!(e, Some(d) if d.invoice_id == invoice_id) {
                *e = None;
                removed += 1;
            }
        }
        self.revoked = self.revoked.saturating_add(removed);
        removed
    }

    /// Returns the delegation for the pair, if any (active or expired).
    pub fn get(&self, invoice_id: u64, delegate: u64) -> Option<Delegation> {
        self.position(invoice_id, delegate).and_then(|i| self.entries[i])
    }

    /// Active level of `delegate` on `invoice_id` at `now`.
    pub fn level_of(&self, invoice_id: u64, delegate: u64, now: u64) -> Option<DelegationLevel> {
        self.get(invoice_id, delegate).filter(|d| d.is_active(now)).map(|d| d.level)
    }

    /// Returns an error unless `delegate` may perform `action` on `invoice_id` at `now`.
    pub fn authorize(
        &self,
        invoice_id: u64,
        delegate: u64,
        action: DelegatedAction,
        now: u64,
    ) -> Result<(), DelegationError> {
        match self.level_of(invoice_id, delegate, now) {
            Some(level) if level >= required_level(action) => Ok(()),
            _ => Err(DelegationError::Unauthorized),
        }
    }

    /// Number of active delegations on `invoice_id` at `now`.
    pub fn active_count(&self, invoice_id: u64, now: u64) -> u32 {
        self.entries
            .iter()
            .flatten()
            .filter(|d| d.invoice_id == invoice_id && d.is_active(now))
            .count() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_enforce_actions() {
        let mut r = DelegationRegistry::default();
        r.grant(1, 10, DelegationLevel::Manage, 100, 0).unwrap();
        assert!(r.authorize(1, 10, DelegatedAction::ViewInvoice, 100).is_ok());
        assert!(r.authorize(1, 10, DelegatedAction::ExtendDeadline, 100).is_ok());
        assert_eq!(
            r.authorize(1, 10, DelegatedAction::CancelInvoice, 100),
            Err(DelegationError::Unauthorized)
        );
        assert_eq!(
            r.authorize(2, 10, DelegatedAction::ViewInvoice, 100),
            Err(DelegationError::Unauthorized)
        );
        assert_eq!(
            r.grant(1, 10, DelegationLevel::Full, 100, 0),
            Err(DelegationError::AlreadyDelegated)
        );
    }

    #[test]
    fn expiry_and_regrant() {
        let mut r = DelegationRegistry::default();
        assert_eq!(
            r.grant(1, 10, DelegationLevel::View, 100, 100),
            Err(DelegationError::InvalidExpiry)
        );
        r.grant(1, 10, DelegationLevel::Full, 100, 200).unwrap();
        assert_eq!(r.level_of(1, 10, 199), Some(DelegationLevel::Full));
        assert_eq!(r.level_of(1, 10, 200), None);
        r.grant(1, 10, DelegationLevel::View, 200, 0).unwrap();
        assert_eq!(r.level_of(1, 10, 500), Some(DelegationLevel::View));
        assert_eq!(r.granted, 2);
    }

    #[test]
    fn revocation_and_tracking() {
        let mut r = DelegationRegistry::default();
        r.grant(1, 10, DelegationLevel::View, 0, 0).unwrap();
        r.grant(1, 11, DelegationLevel::Manage, 0, 0).unwrap();
        r.grant(2, 10, DelegationLevel::Full, 0, 0).unwrap();
        assert_eq!(r.active_count(1, 0), 2);

        r.revoke(1, 10).unwrap();
        assert_eq!(r.revoke(1, 10), Err(DelegationError::NotFound));
        assert_eq!(
            r.authorize(1, 10, DelegatedAction::ViewInvoice, 0),
            Err(DelegationError::Unauthorized)
        );
        assert_eq!(r.revoke_all(1), 1);
        assert_eq!(r.active_count(1, 0), 0);
        assert_eq!(r.active_count(2, 0), 1);
        assert_eq!((r.granted, r.revoked), (3, 2));
    }

    #[test]
    fn registry_full() {
        let mut r = DelegationRegistry::default();
        for i in 0..MAX_DELEGATIONS as u64 {
            r.grant(1, i, DelegationLevel::View, 0, 0).unwrap();
        }
        assert_eq!(
            r.grant(1, 99, DelegationLevel::View, 0, 0),
            Err(DelegationError::RegistryFull)
        );
    }
}
