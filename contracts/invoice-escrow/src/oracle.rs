//! External price-oracle interface used by oracle condition guards.
//!
//! The interface follows the SEP-40 `lastprice` shape: an oracle returns the
//! most recent price for an asset together with the timestamp at which that
//! price was observed, or `None` when it has no price for the asset.
//!
//! The escrow contract always calls oracles through the generated
//! `try_lastprice`, so a misbehaving oracle (panic, missing contract, wrong
//! return type) is reported as [`crate::errors::Error::OracleUnavailable`]
//! rather than aborting the caller's transaction.

use soroban_sdk::{contractclient, Address, Env};

use crate::types::PriceData;

#[contractclient(name = "PriceOracleClient")]
pub trait PriceOracle {
    /// Return the latest price for `asset`, or `None` if unavailable.
    fn lastprice(env: Env, asset: Address) -> Option<PriceData>;
}
