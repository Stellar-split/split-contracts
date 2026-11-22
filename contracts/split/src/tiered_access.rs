//! Issue #1170: tiered access control (first slice).
//!
//! Pure helpers: an ordered tier ladder, permission-to-minimum-tier mapping,
//! delegation rules and grant/deny counters. Not covered: storing tier
//! assignments per address, entry points enforcing tiers, signature checks
//! (`require_auth`), persisted access logs and events.

/// Access tiers, ordered lowest to highest.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AccessTier {
    /// Read-only.
    Viewer = 0,
    /// Day-to-day operations.
    Operator = 1,
    /// May delegate lower tiers.
    Manager = 2,
    /// Full access.
    Admin = 3,
}

/// Permissions mapped to a minimum tier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Permission {
    /// View invoices.
    ViewInvoice,
    /// Record payments.
    RecordPayment,
    /// Change invoice configuration.
    ConfigureInvoice,
    /// Change contract-wide settings.
    ManageSettings,
}

/// Errors for tier helpers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessError {
    /// Tier too low for the permission.
    Unauthorized,
    /// Delegation not permitted.
    InvalidDelegation,
}

/// Minimum tier required for a permission.
pub fn min_tier(permission: Permission) -> AccessTier {
    match permission {
        Permission::ViewInvoice => AccessTier::Viewer,
        Permission::RecordPayment => AccessTier::Operator,
        Permission::ConfigureInvoice => AccessTier::Manager,
        Permission::ManageSettings => AccessTier::Admin,
    }
}

/// True when `tier` grants `permission`.
pub fn is_allowed(tier: AccessTier, permission: Permission) -> bool {
    tier >= min_tier(permission)
}

/// Returns an error when `tier` does not grant `permission`.
pub fn require_permission(tier: AccessTier, permission: Permission) -> Result<(), AccessError> {
    if is_allowed(tier, permission) {
        Ok(())
    } else {
        Err(AccessError::Unauthorized)
    }
}

/// Checks that `granter` may delegate `target` tier: granter must be at least
/// Manager and `target` must be strictly below the granter's own tier.
pub fn check_delegation(granter: AccessTier, target: AccessTier) -> Result<(), AccessError> {
    if granter >= AccessTier::Manager && target < granter {
        Ok(())
    } else {
        Err(AccessError::InvalidDelegation)
    }
}

/// Access counters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AccessStats {
    /// Allowed checks.
    pub granted: u32,
    /// Denied checks.
    pub denied: u32,
}

impl AccessStats {
    /// Records one check outcome (saturating).
    pub fn record(&mut self, allowed: bool) {
        if allowed {
            self.granted = self.granted.saturating_add(1);
        } else {
            self.denied = self.denied.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_and_delegation() {
        assert!(is_allowed(AccessTier::Operator, Permission::RecordPayment));
        assert_eq!(
            require_permission(AccessTier::Operator, Permission::ManageSettings),
            Err(AccessError::Unauthorized)
        );
        assert!(check_delegation(AccessTier::Manager, AccessTier::Operator).is_ok());
        assert!(check_delegation(AccessTier::Manager, AccessTier::Manager).is_err());
        assert!(check_delegation(AccessTier::Operator, AccessTier::Viewer).is_err());
    }

    #[test]
    fn stats() {
        let mut s = AccessStats::default();
        s.record(true);
        s.record(false);
        assert_eq!(s, AccessStats { granted: 1, denied: 1 });
    }
}
