//! Centralized constant definitions for the StellarSplit contract.

/// Issue #563: Minimum invoice TTL in ledgers.
/// Set to ~60 days of ledgers (assuming ~5 seconds per ledger on Soroban).
/// This ensures invoices remain accessible during typical dispute/resolution windows.
pub const MIN_INVOICE_TTL_LEDGERS: u32 = 518_400;

/// Issue #563: Maximum invoice TTL in ledgers.
/// Set to ~1 year of ledgers to allow long-term invoice archival and dispute resolution.
/// Invoices can be bumped multiple times within this window to extend their lifetime.
pub const MAX_INVOICE_TTL_LEDGERS: u32 = 31_536_000;
