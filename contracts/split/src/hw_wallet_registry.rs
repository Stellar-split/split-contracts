//! Issue #1171: hardware wallet signer registry (first slice).
//!
//! Hardware wallet signing and device communication happen off-chain. This
//! module only provides the on-chain bookkeeping side: a fixed-capacity,
//! heap-free registry of device *public* keys, supporting several devices per
//! account, revocation and per-device signature tracking. It never handles
//! private key material.

/// Maximum number of devices tracked by one registry.
pub const MAX_DEVICES: usize = 8;

/// Errors returned by the registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HwWalletError {
    /// The registry has no free slot.
    RegistryFull,
    /// The public key is already registered.
    DuplicateDevice,
    /// No device with that public key exists.
    UnknownDevice,
    /// The device has been revoked.
    DeviceRevoked,
}

/// Kind of hardware device a key belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    /// Ledger-style device.
    Ledger,
    /// Trezor-style device.
    Trezor,
    /// Any other device.
    Other,
}

/// Registered device record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceRecord {
    /// Device kind.
    pub kind: DeviceKind,
    /// Ed25519 public key of the device (public data only).
    pub public_key: [u8; 32],
    /// Ledger timestamp of registration.
    pub registered_at: u64,
    /// Ledger timestamp of last recorded signature (0 if none).
    pub last_used: u64,
    /// Number of signatures recorded for this device.
    pub sign_count: u32,
    /// Whether the device was revoked.
    pub revoked: bool,
}

/// Fixed-capacity registry of hardware devices.
#[derive(Clone, Copy, Debug)]
pub struct HwRegistry {
    slots: [Option<DeviceRecord>; MAX_DEVICES],
}

impl Default for HwRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl HwRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        HwRegistry { slots: [None; MAX_DEVICES] }
    }

    fn find(&self, key: &[u8; 32]) -> Option<usize> {
        self.slots
            .iter()
            .position(|s| matches!(s, Some(r) if r.public_key == *key))
    }

    /// Registers a device public key at time `now`.
    pub fn register(
        &mut self,
        kind: DeviceKind,
        public_key: [u8; 32],
        now: u64,
    ) -> Result<(), HwWalletError> {
        if self.find(&public_key).is_some() {
            return Err(HwWalletError::DuplicateDevice);
        }
        let idx = self
            .slots
            .iter()
            .position(|s| s.is_none())
            .ok_or(HwWalletError::RegistryFull)?;
        self.slots[idx] = Some(DeviceRecord {
            kind,
            public_key,
            registered_at: now,
            last_used: 0,
            sign_count: 0,
            revoked: false,
        });
        Ok(())
    }

    /// Revokes a device; revoked devices can no longer record signatures.
    pub fn revoke(&mut self, public_key: &[u8; 32]) -> Result<(), HwWalletError> {
        let idx = self.find(public_key).ok_or(HwWalletError::UnknownDevice)?;
        if let Some(rec) = self.slots[idx].as_mut() {
            rec.revoked = true;
        }
        Ok(())
    }

    /// Records that a (separately verified) signature from the device was
    /// accepted at time `now`. Signature verification is not done here.
    pub fn record_signature(
        &mut self,
        public_key: &[u8; 32],
        now: u64,
    ) -> Result<u32, HwWalletError> {
        let idx = self.find(public_key).ok_or(HwWalletError::UnknownDevice)?;
        match self.slots[idx].as_mut() {
            Some(rec) if rec.revoked => Err(HwWalletError::DeviceRevoked),
            Some(rec) => {
                rec.sign_count = rec.sign_count.saturating_add(1);
                rec.last_used = now;
                Ok(rec.sign_count)
            }
            None => Err(HwWalletError::UnknownDevice),
        }
    }

    /// Returns the record for a device, if registered.
    pub fn get(&self, public_key: &[u8; 32]) -> Option<DeviceRecord> {
        self.find(public_key).and_then(|i| self.slots[i])
    }

    /// Number of registered, non-revoked devices.
    pub fn active_count(&self) -> usize {
        self.slots
            .iter()
            .filter(|s| matches!(s, Some(r) if !r.revoked))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_track_revoke() {
        let mut r = HwRegistry::new();
        let k = [1u8; 32];
        assert_eq!(r.register(DeviceKind::Ledger, k, 10), Ok(()));
        assert_eq!(r.register(DeviceKind::Trezor, k, 11), Err(HwWalletError::DuplicateDevice));
        assert_eq!(r.record_signature(&k, 20), Ok(1));
        assert_eq!(r.record_signature(&k, 30), Ok(2));
        assert_eq!(r.get(&k).map(|d| d.last_used), Some(30));
        assert_eq!(r.revoke(&k), Ok(()));
        assert_eq!(r.record_signature(&k, 40), Err(HwWalletError::DeviceRevoked));
        assert_eq!(r.active_count(), 0);
    }

    #[test]
    fn capacity_and_unknown() {
        let mut r = HwRegistry::new();
        for i in 0..MAX_DEVICES {
            assert_eq!(r.register(DeviceKind::Other, [i as u8; 32], 0), Ok(()));
        }
        assert_eq!(r.register(DeviceKind::Other, [99u8; 32], 0), Err(HwWalletError::RegistryFull));
        assert_eq!(r.record_signature(&[99u8; 32], 0), Err(HwWalletError::UnknownDevice));
        assert_eq!(r.active_count(), MAX_DEVICES);
    }
}
