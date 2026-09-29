//! Issue #869: Invoice redemption tokens for secondary markets.
//!
//! An invoice creator can issue redemption tokens after an invoice is released.
//! Each token represents a claim on a portion of the funded pool and can be
//! freely transferred between addresses, enabling secondary-market trading.
//! The holder of a token may redeem it to receive the underlying funds.

use super::*;
use crate::events;
use crate::storage_keys::{CompoundKey, InvoiceKey};
use soroban_sdk::{contractimpl, panic_with_error, token, Address, Env};

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn token_count_key(invoice_id: u64) -> InvoiceKey {
    InvoiceKey::RedemptionTokenCount(invoice_id)
}

fn token_key(invoice_id: u64, token_id: u64) -> CompoundKey {
    CompoundKey::RedemptionToken(invoice_id, token_id)
}

/// Increment and return the next token id for an invoice.
fn next_token_id(env: &Env, invoice_id: u64) -> u64 {
    let key = token_count_key(invoice_id);
    let id: u64 = env.storage().persistent().get(&key).unwrap_or(0);
    env.storage().persistent().set(&key, &(id + 1));
    id
}

// ---------------------------------------------------------------------------
// Contract entry points
// ---------------------------------------------------------------------------

#[contractimpl]
impl SplitContract {
    /// Issue a redemption token for a released invoice (creator only).
    ///
    /// The creator specifies the initial `holder` and the `claim_amount` the
    /// token represents. The invoice must be in `Released` status.
    /// Returns the new token id.
    pub fn issue_redemption_token(
        env: Env,
        creator: Address,
        invoice_id: u64,
        holder: Address,
        claim_amount: i128,
        expires_at: Option<u64>,
    ) -> u64 {
        require_not_paused(&env);
        creator.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        if invoice.creator != creator {
            panic_with_error!(&env, ContractError::NotAuthorized);
        }
        if invoice.status != InvoiceStatus::Released {
            panic_with_error!(&env, ContractError::InvalidStatus);
        }
        if claim_amount <= 0 {
            panic_with_error!(&env, ContractError::InvalidAmount);
        }
        let token_id = next_token_id(&env, invoice_id);
        let redemption = RedemptionToken {
            invoice_id,
            holder: holder.clone(),
            claim_amount,
            redeemed: false,
            issued_at: env.ledger().timestamp(),
            expires_at,
        };
        env.storage()
            .persistent()
            .set(&token_key(invoice_id, token_id), &redemption);
        events::redemption_token_issued(
            &env,
            invoice_id,
            token_id,
            &holder,
            claim_amount,
            &expires_at,
        );
        token_id
    }

    /// Transfer a redemption token to a new holder.
    /// Only the current holder can transfer.
    pub fn transfer_redemption_token(
        env: Env,
        invoice_id: u64,
        token_id: u64,
        from: Address,
        to: Address,
    ) {
        require_not_paused(&env);
        from.require_auth();
        let key = token_key(invoice_id, token_id);
        let mut redemption: RedemptionToken = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, ContractError::RedemptionTokenNotFound));
        if redemption.holder != from {
            panic_with_error!(&env, ContractError::NotTokenHolder);
        }
        if redemption.redeemed {
            panic_with_error!(&env, ContractError::TokenAlreadyRedeemed);
        }
        if let Some(exp) = redemption.expires_at {
            if env.ledger().timestamp() > exp {
                panic_with_error!(&env, ContractError::TokenExpired);
            }
        }
        events::redemption_token_transferred(&env, invoice_id, token_id, &from, &to);
        redemption.holder = to;
        env.storage().persistent().set(&key, &redemption);
    }

    /// Redeem a token, transferring `claim_amount` of the invoice token from
    /// the contract to the holder.
    pub fn redeem_token(env: Env, invoice_id: u64, token_id: u64, holder: Address) {
        require_not_paused(&env);
        holder.require_auth();
        let key = token_key(invoice_id, token_id);
        let mut redemption: RedemptionToken = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, ContractError::RedemptionTokenNotFound));
        if redemption.holder != holder {
            panic_with_error!(&env, ContractError::NotTokenHolder);
        }
        if redemption.redeemed {
            panic_with_error!(&env, ContractError::TokenAlreadyRedeemed);
        }
        if let Some(exp) = redemption.expires_at {
            if env.ledger().timestamp() > exp {
                panic_with_error!(&env, ContractError::TokenExpired);
            }
        }
        let invoice = load_invoice(&env, invoice_id);
        let tok = funding_token_for(&invoice);
        token::Client::new(&env, &tok).transfer(
            &env.current_contract_address(),
            &holder,
            &redemption.claim_amount,
        );
        redemption.redeemed = true;
        env.storage().persistent().set(&key, &redemption);
        events::redemption_token_redeemed(
            &env,
            invoice_id,
            token_id,
            &holder,
            redemption.claim_amount,
        );
    }

    /// Get a redemption token record. Returns `None` if it does not exist.
    pub fn get_redemption_token(
        env: Env,
        invoice_id: u64,
        token_id: u64,
    ) -> Option<RedemptionToken> {
        env.storage()
            .persistent()
            .get(&token_key(invoice_id, token_id))
    }

    /// Get the total number of redemption tokens issued for an invoice.
    pub fn get_redemption_token_count(env: Env, invoice_id: u64) -> u64 {
        env.storage()
            .persistent()
            .get(&token_count_key(invoice_id))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{client, make_invoice, setup_initialized};
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn cannot_issue_token_on_pending_invoice() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let recipient = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &recipient, 1000, &token, 9_999);
        let holder = Address::generate(&env);
        // Invoice is Pending — should fail with InvalidStatus.
        assert!(c
            .try_issue_redemption_token(&creator, &id, &holder, &100, &None)
            .is_err());
    }

    #[test]
    fn token_count_starts_at_zero() {
        let (env, cid, _token) = setup_initialized();
        let c = client(&env, &cid);
        assert_eq!(c.get_redemption_token_count(&42u64), 0u64);
    }

    #[test]
    fn transfer_nonexistent_token_errors() {
        let (env, cid, _token) = setup_initialized();
        let c = client(&env, &cid);
        let from = Address::generate(&env);
        let to = Address::generate(&env);
        assert!(c
            .try_transfer_redemption_token(&42u64, &0u64, &from, &to)
            .is_err());
    }

    #[test]
    fn get_nonexistent_token_returns_none() {
        let (env, cid, _token) = setup_initialized();
        let c = client(&env, &cid);
        assert!(c.get_redemption_token(&99u64, &0u64).is_none());
    }

    #[test]
    fn redeem_nonexistent_token_errors() {
        let (env, cid, _token) = setup_initialized();
        let c = client(&env, &cid);
        let holder = Address::generate(&env);
        assert!(c.try_redeem_token(&99u64, &0u64, &holder).is_err());
    }

    #[test]
    fn only_creator_can_issue_token() {
        let (env, cid, token) = setup_initialized();
        let c = client(&env, &cid);
        let creator = Address::generate(&env);
        let other = Address::generate(&env);
        let recipient = Address::generate(&env);
        let id = make_invoice(&env, &c, &creator, &recipient, 1000, &token, 9_999);
        let holder = Address::generate(&env);
        assert!(c
            .try_issue_redemption_token(&other, &id, &holder, &100, &None)
            .is_err());
    }
}
