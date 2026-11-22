//! Invoice watermark helpers (issue #1165), first slice.
//!
//! Derives a deterministic 64-bit watermark tag from invoice id, issuer id,
//! recipient id and a caller-supplied secret, and keeps a small fixed-size
//! registry for verification and tracking. The tag uses FNV-1a, which is NOT
//! cryptographically secure: this slice does not prevent forgery by an
//! attacker who knows the secret scheme. Not wired to contract storage.

#![allow(dead_code)]

/// Maximum watermarks tracked by a [`WatermarkRegistry`].
pub const MAX_WATERMARKS: usize = 16;

/// Errors returned by watermark operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatermarkError {
    /// A watermark already exists for this invoice.
    AlreadyMarked,
    /// Registry is full.
    RegistryFull,
    /// No watermark recorded for this invoice.
    NotFound,
}

/// Visual/placement style (customisation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatermarkStyle {
    /// Tag appears once in the header.
    Header,
    /// Tag repeats across the document.
    Tiled,
    /// Tag appears only in metadata.
    Hidden,
}

/// A watermark bound to one invoice and recipient.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Watermark {
    /// Invoice identifier.
    pub invoice_id: u64,
    /// Recipient the copy was issued to.
    pub recipient: u64,
    /// Derived tag.
    pub tag: u64,
    /// Placement style.
    pub style: WatermarkStyle,
}

fn fnv(words: &[u64]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for w in words {
        for b in w.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// Computes the tag for the given fields and `secret`.
pub fn compute_tag(invoice_id: u64, issuer: u64, recipient: u64, secret: u64) -> u64 {
    fnv(&[secret, invoice_id, issuer, recipient])
}

/// Builds a watermark for an invoice copy.
pub fn make_watermark(
    invoice_id: u64,
    issuer: u64,
    recipient: u64,
    secret: u64,
    style: WatermarkStyle,
) -> Watermark {
    Watermark { invoice_id, recipient, tag: compute_tag(invoice_id, issuer, recipient, secret), style }
}

/// Returns true if `wm` matches the tag recomputed from the given fields.
pub fn verify_watermark(wm: &Watermark, issuer: u64, secret: u64) -> bool {
    wm.tag == compute_tag(wm.invoice_id, issuer, wm.recipient, secret)
}

/// Fixed-capacity registry of issued watermarks.
#[derive(Clone, Copy, Debug)]
pub struct WatermarkRegistry {
    items: [Option<Watermark>; MAX_WATERMARKS],
    verifications: u32,
}

impl Default for WatermarkRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl WatermarkRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self { items: [None; MAX_WATERMARKS], verifications: 0 }
    }

    /// Number of watermarks recorded.
    pub fn count(&self) -> usize {
        self.items.iter().filter(|i| i.is_some()).count()
    }

    /// Number of verifications performed.
    pub fn verifications(&self) -> u32 {
        self.verifications
    }

    /// Records a watermark; one per invoice.
    pub fn record(&mut self, wm: Watermark) -> Result<(), WatermarkError> {
        if self.items.iter().flatten().any(|w| w.invoice_id == wm.invoice_id) {
            return Err(WatermarkError::AlreadyMarked);
        }
        match self.items.iter_mut().find(|s| s.is_none()) {
            Some(slot) => {
                *slot = Some(wm);
                Ok(())
            }
            None => Err(WatermarkError::RegistryFull),
        }
    }

    /// Looks up the recorded watermark for an invoice.
    pub fn get(&self, invoice_id: u64) -> Option<Watermark> {
        self.items.iter().flatten().find(|w| w.invoice_id == invoice_id).copied()
    }

    /// Verifies a presented tag against the recorded watermark.
    pub fn verify(&mut self, invoice_id: u64, presented_tag: u64) -> Result<bool, WatermarkError> {
        let wm = self.get(invoice_id).ok_or(WatermarkError::NotFound)?;
        self.verifications = self.verifications.saturating_add(1);
        Ok(wm.tag == presented_tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_is_deterministic_and_bound_to_fields() {
        let a = compute_tag(1, 2, 3, 99);
        assert_eq!(a, compute_tag(1, 2, 3, 99));
        assert_ne!(a, compute_tag(1, 2, 4, 99));
        assert_ne!(a, compute_tag(1, 2, 3, 100));
    }

    #[test]
    fn verifies_and_detects_tamper() {
        let wm = make_watermark(7, 2, 3, 99, WatermarkStyle::Tiled);
        assert!(verify_watermark(&wm, 2, 99));
        assert!(!verify_watermark(&wm, 2, 98));
        let forged = Watermark { recipient: 4, ..wm };
        assert!(!verify_watermark(&forged, 2, 99));
    }

    #[test]
    fn registry_tracks_and_verifies() {
        let mut r = WatermarkRegistry::new();
        let wm = make_watermark(7, 2, 3, 99, WatermarkStyle::Header);
        r.record(wm).unwrap();
        assert_eq!(r.record(wm), Err(WatermarkError::AlreadyMarked));
        assert_eq!(r.count(), 1);
        assert_eq!(r.verify(7, wm.tag), Ok(true));
        assert_eq!(r.verify(7, wm.tag ^ 1), Ok(false));
        assert_eq!(r.verify(8, 0), Err(WatermarkError::NotFound));
        assert_eq!(r.verifications(), 2);
    }

    #[test]
    fn registry_capacity() {
        let mut r = WatermarkRegistry::new();
        for i in 0..MAX_WATERMARKS as u64 {
            r.record(make_watermark(i, 1, 1, 1, WatermarkStyle::Hidden)).unwrap();
        }
        assert_eq!(
            r.record(make_watermark(100, 1, 1, 1, WatermarkStyle::Hidden)),
            Err(WatermarkError::RegistryFull)
        );
    }
}
