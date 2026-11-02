//! Centralized constant definitions for the StellarSplit contract.

/// Issue #563: Minimum invoice TTL in ledgers.
/// Set to ~60 days of ledgers (assuming ~5 seconds per ledger on Soroban).
/// This ensures invoices remain accessible during typical dispute/resolution windows.
pub const MIN_INVOICE_TTL_LEDGERS: u32 = 518_400;

/// Issue #563: Maximum invoice TTL in ledgers.
/// Set to ~1 year of ledgers to allow long-term invoice archival and dispute resolution.
/// Invoices can be bumped multiple times within this window to extend their lifetime.
pub const MAX_INVOICE_TTL_LEDGERS: u32 = 31_536_000;

/// Maximum TTL for temporary storage in Soroban ledgers.
/// Temporary storage has a lower maximum TTL than persistent/instance storage.
/// This is approximately 1 month (~6_311_999 ledgers at ~5 seconds per ledger).
pub const MAX_TEMPORARY_TTL_LEDGERS: u32 = 6_311_999;

/// Issue #935: Total number of basis points representing 100%.
///
/// Basis points (bps) are used for all rate and ratio values in the contract.
/// One basis point equals 1/100th of a percent (0.01%), so 10_000 bps == 100%.
/// Any split rule, fee, or ratio expressed in bps must never exceed this value.
pub const TOTAL_BPS: u32 = 10_000;

/// Issue #935: Maximum platform fee rate in basis points.
///
/// The platform fee is deducted from the invoice total before it is distributed
/// to split recipients. This cap (1000 bps == 10%) prevents misconfiguration
/// from silently draining funds from recipients or making invoices uneconomical.
/// It also bounds the worst-case impact of a compromised or buggy admin config.
pub const MAX_PLATFORM_FEE_BPS: u32 = 1_000;

/// Issue #935: Minimum platform fee rate in basis points.
///
/// A fee of 0 bps is valid (no platform fee). This constant documents the
/// inclusive lower bound so validation logic can be written symmetrically.
pub const MIN_PLATFORM_FEE_BPS: u32 = 0;

/// Issue #939: Maximum number of split rules allowed per invoice.
///
/// Bounds the amount of work performed during distribution and prevents
/// unbounded storage growth / iteration costs from misconfigured invoices.
pub const MAX_SPLIT_RULES: u32 = 32;

/// Issue #939: Minimum number of split rules allowed per invoice.
///
/// An invoice must have at least one recipient to be distributable.
pub const MIN_SPLIT_RULES: u32 = 1;

/// Issue #939: Maximum ratio value for a single split rule in basis points.
///
/// A single rule may allocate up to 100% of the distributable amount.
/// Combined with the sum check against [`TOTAL_BPS`], this ensures no rule
/// can exceed the total and that all rules together sum to exactly 100%.
pub const MAX_RATIO_BPS: u32 = TOTAL_BPS;
