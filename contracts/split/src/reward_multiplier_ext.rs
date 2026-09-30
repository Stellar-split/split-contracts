//! Issue #880: recipient reward multiplier system.
//!
//! A creator can attach a reward multiplier (in basis points) to one or more
//! recipients on an invoice. At release time, each recipient's payout is
//! scaled by `multiplier_bps / 10_000`. The multiplier is capped at 20_000
//! (2×) and must be ≥ 1 (i.e. zero or negative multipliers are rejected).
//!
//! Only the invoice creator may set or clear multipliers. Multipliers can be
//! updated at any time while the invoice is still `Pending`.
//!
//! ## Events
//! - `(split, mul_set, invoice_id)` — data: `(recipient, multiplier_bps)`
//! - `(split, mul_clr, invoice_id)` — data: `recipient`
//!
//! ## Storage
//! Uses a local `RewardMulKey` enum stored in **persistent** storage.

use super::*;
use soroban_sdk::{contractimpl, contracttype, symbol_short, Address, Env, Vec};

/// Maximum multiplier in basis points (2× = 20_000).
pub const MAX_MULTIPLIER_BPS: u32 = 20_000;
/// Minimum multiplier in basis points (must be at least 1 bps).
pub const MIN_MULTIPLIER_BPS: u32 = 1;

/// Persistent storage keys for the reward multiplier module.
#[contracttype]
#[derive(Clone)]
pub enum RewardMulKey {
    /// `u32` multiplier (in bps) for a specific `(invoice_id, recipient)`.
    Multiplier(u64, Address),
    /// `Vec<Address>` — ordered list of recipients that have a multiplier set,
    /// so `get_all_multipliers` can enumerate them without a full storage scan.
    MultiplierRecipients(u64),
}

/// A single entry returned by `get_all_multipliers`.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct RecipientMultiplier {
    pub recipient: Address,
    /// Multiplier in basis points (10_000 = 1×, 20_000 = 2×).
    pub multiplier_bps: u32,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn load_recipients(env: &Env, invoice_id: u64) -> Vec<Address> {
    env.storage()
        .persistent()
        .get(&RewardMulKey::MultiplierRecipients(invoice_id))
        .unwrap_or_else(|| Vec::new(env))
}

fn save_recipients(env: &Env, invoice_id: u64, list: &Vec<Address>) {
    env.storage()
        .persistent()
        .set(&RewardMulKey::MultiplierRecipients(invoice_id), list);
}

/// Returns the effective multiplier for `recipient` on `invoice_id`, or
/// `10_000` (1×, no-op) if none has been configured.
pub(crate) fn effective_multiplier(env: &Env, invoice_id: u64, recipient: &Address) -> u32 {
    env.storage()
        .persistent()
        .get(&RewardMulKey::Multiplier(invoice_id, recipient.clone()))
        .unwrap_or(10_000u32)
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

#[contractimpl]
impl SplitContract {
    /// Set a reward multiplier for `recipient` on `invoice_id`.
    ///
    /// - Only callable by the invoice creator.
    /// - Invoice must be in `Pending` status.
    /// - `multiplier_bps` must be in `[1, 20_000]`.
    pub fn set_reward_multiplier(
        env: Env,
        creator: Address,
        invoice_id: u64,
        recipient: Address,
        multiplier_bps: u32,
    ) {
        require_not_paused(&env);
        creator.require_auth();

        assert!(
            multiplier_bps >= MIN_MULTIPLIER_BPS && multiplier_bps <= MAX_MULTIPLIER_BPS,
            "multiplier_bps out of range [1, 20000]"
        );

        let invoice = load_invoice(&env, invoice_id);
        assert!(invoice.creator == creator, "only creator can set multiplier");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(
            invoice.recipients.iter().any(|r| r == recipient),
            "recipient not on invoice"
        );

        // Track which recipients have a multiplier so we can enumerate them.
        let mut list = load_recipients(&env, invoice_id);
        if !list.contains(recipient.clone()) {
            list.push_back(recipient.clone());
            save_recipients(&env, invoice_id, &list);
        }

        env.storage().persistent().set(
            &RewardMulKey::Multiplier(invoice_id, recipient.clone()),
            &multiplier_bps,
        );

        env.events().publish(
            (symbol_short!("split"), symbol_short!("mul_set"), invoice_id),
            (recipient, multiplier_bps),
        );
    }

    /// Remove the reward multiplier for `recipient` on `invoice_id` (resets
    /// to the default 1×).
    ///
    /// - Only callable by the invoice creator.
    /// - Invoice must be in `Pending` status.
    pub fn clear_reward_multiplier(
        env: Env,
        creator: Address,
        invoice_id: u64,
        recipient: Address,
    ) {
        require_not_paused(&env);
        creator.require_auth();

        let invoice = load_invoice(&env, invoice_id);
        assert!(invoice.creator == creator, "only creator can clear multiplier");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        env.storage()
            .persistent()
            .remove(&RewardMulKey::Multiplier(invoice_id, recipient.clone()));

        // Remove from the enumeration list.
        let list = load_recipients(&env, invoice_id);
        let mut new_list = Vec::new(&env);
        for r in list.iter() {
            if r != recipient {
                new_list.push_back(r);
            }
        }
        save_recipients(&env, invoice_id, &new_list);

        env.events().publish(
            (symbol_short!("split"), symbol_short!("mul_clr"), invoice_id),
            recipient,
        );
    }

    /// Returns the current multiplier (in bps) for `recipient`, or `10_000`
    /// if none is configured.
    pub fn get_reward_multiplier(env: Env, invoice_id: u64, recipient: Address) -> u32 {
        effective_multiplier(&env, invoice_id, &recipient)
    }

    /// Returns all `(recipient, multiplier_bps)` entries configured for an
    /// invoice.
    pub fn get_all_multipliers(env: Env, invoice_id: u64) -> Vec<RecipientMultiplier> {
        let list = load_recipients(&env, invoice_id);
        let mut out = Vec::new(&env);
        for r in list.iter() {
            let bps: u32 = env
                .storage()
                .persistent()
                .get(&RewardMulKey::Multiplier(invoice_id, r.clone()))
                .unwrap_or(10_000u32);
            out.push_back(RecipientMultiplier {
                recipient: r,
                multiplier_bps: bps,
            });
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ext_test_util::{fixture, mint, new_invoice, pay};
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::Address;

    #[test]
    fn set_and_get_multiplier() {
        let f = fixture();
        let (id, creator, recipient) = new_invoice(&f, 1_000);
        f.c.set_reward_multiplier(&creator, &id, &recipient, &15_000u32);
        assert_eq!(f.c.get_reward_multiplier(&id, &recipient), 15_000u32);
    }

    #[test]
    fn default_multiplier_is_10000() {
        let f = fixture();
        let (id, _, recipient) = new_invoice(&f, 1_000);
        assert_eq!(f.c.get_reward_multiplier(&id, &recipient), 10_000u32);
    }

    #[test]
    fn clear_multiplier_returns_to_default() {
        let f = fixture();
        let (id, creator, recipient) = new_invoice(&f, 1_000);
        f.c.set_reward_multiplier(&creator, &id, &recipient, &15_000u32);
        f.c.clear_reward_multiplier(&creator, &id, &recipient);
        assert_eq!(f.c.get_reward_multiplier(&id, &recipient), 10_000u32);
    }

    #[test]
    fn get_all_multipliers_lists_entries() {
        let f = fixture();
        let (id, creator, recipient) = new_invoice(&f, 1_000);
        f.c.set_reward_multiplier(&creator, &id, &recipient, &12_000u32);
        let all = f.c.get_all_multipliers(&id);
        assert_eq!(all.len(), 1);
        assert_eq!(all.get(0).unwrap().recipient, recipient);
        assert_eq!(all.get(0).unwrap().multiplier_bps, 12_000u32);
    }

    #[test]
    #[should_panic(expected = "only creator can set multiplier")]
    fn non_creator_cannot_set_multiplier() {
        let f = fixture();
        let (id, _, recipient) = new_invoice(&f, 1_000);
        let other = Address::generate(&f.env);
        f.c.set_reward_multiplier(&other, &id, &recipient, &15_000u32);
    }

    #[test]
    #[should_panic(expected = "multiplier_bps out of range")]
    fn zero_multiplier_rejected() {
        let f = fixture();
        let (id, creator, recipient) = new_invoice(&f, 1_000);
        f.c.set_reward_multiplier(&creator, &id, &recipient, &0u32);
    }

    #[test]
    #[should_panic(expected = "multiplier_bps out of range")]
    fn too_large_multiplier_rejected() {
        let f = fixture();
        let (id, creator, recipient) = new_invoice(&f, 1_000);
        f.c.set_reward_multiplier(&creator, &id, &recipient, &20_001u32);
    }

    #[test]
    #[should_panic(expected = "recipient not on invoice")]
    fn unknown_recipient_rejected() {
        let f = fixture();
        let (id, creator, _) = new_invoice(&f, 1_000);
        let stranger = Address::generate(&f.env);
        f.c.set_reward_multiplier(&creator, &id, &stranger, &15_000u32);
    }

    #[test]
    fn multiplier_after_pay_still_readable() {
        let f = fixture();
        let (id, creator, recipient) = new_invoice(&f, 100);
        mint(&f.env, &f.token, &creator, 100);
        pay(&f, &creator, id, 50);
        // multiplier on a paid invoice (still Pending until fully funded)
        f.c.set_reward_multiplier(&creator, &id, &recipient, &11_000u32);
        assert_eq!(f.c.get_reward_multiplier(&id, &recipient), 11_000u32);
    }
}
