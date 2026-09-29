//! Issue #767: invoice access-control tiers (public, private, invite-only).
//!
//! The tier is stored in `(inv_vis, invoice_id)` and can be set exactly once by
//! the creator before any payment. The private code hash is never returned by
//! any read; `get_invoice_visibility` exposes only the tier symbol.
//!
//! * Public: payable by anyone (default when unset).
//! * Private: payable only via `pay_with_access_code` with the matching code.
//! * InviteOnly: defers to the invoice's `allowed_payers` whitelist; if none is
//!   configured the invoice is closed.

use super::*;
use soroban_sdk::{contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env, Symbol};

#[contracttype]
#[derive(Clone, Debug)]
pub enum InvoiceVisibility {
    Public,
    Private(BytesN<32>),
    InviteOnly,
}

fn vis_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("inv_vis"), id)
}

fn access_ok_key(id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("acc_ok"), id, payer.clone())
}

fn tier_symbol(v: &InvoiceVisibility) -> Symbol {
    match v {
        InvoiceVisibility::Public => symbol_short!("public"),
        InvoiceVisibility::Private(_) => symbol_short!("private"),
        InvoiceVisibility::InviteOnly => symbol_short!("invite"),
    }
}

/// Called from `_pay`: enforces the tier for the paying address.
pub(crate) fn check_access(env: &Env, invoice_id: u64, payer: &Address, invoice: &Invoice) {
    let vis: Option<InvoiceVisibility> = env.storage().persistent().get(&vis_key(invoice_id));
    match vis {
        None | Some(InvoiceVisibility::Public) => {}
        Some(InvoiceVisibility::Private(_)) => {
            let k = access_ok_key(invoice_id, payer);
            assert!(env.storage().temporary().has(&k), "AccessCodeRequired");
            env.storage().temporary().remove(&k);
        }
        Some(InvoiceVisibility::InviteOnly) => {
            assert!(invoice.allowed_payers.is_some(), "InviteOnlyClosed");
        }
    }
}

#[contractimpl]
impl SplitContract {
    /// Set the invoice visibility tier (creator only, once, before any payment).
    pub fn set_invoice_visibility(
        env: Env,
        creator: Address,
        invoice_id: u64,
        visibility: InvoiceVisibility,
    ) {
        require_not_paused(&env);
        creator.require_auth();
        let invoice = load_invoice(&env, invoice_id);
        assert!(invoice.creator == creator, "only creator can set visibility");
        assert!(invoice.funded == 0, "cannot set visibility after payment");
        assert!(
            !env.storage().persistent().has(&vis_key(invoice_id)),
            "VisibilityAlreadySet"
        );
        let tier = tier_symbol(&visibility);
        env.storage().persistent().set(&vis_key(invoice_id), &visibility);
        env.events().publish(
            (symbol_short!("split"), symbol_short!("vis_set"), invoice_id),
            tier,
        );
    }

    /// Returns the tier only (`public` / `private` / `invite`), never the code hash.
    pub fn get_invoice_visibility(env: Env, invoice_id: u64) -> Symbol {
        match env.storage().persistent().get::<_, InvoiceVisibility>(&vis_key(invoice_id)) {
            Some(v) => tier_symbol(&v),
            None => symbol_short!("public"),
        }
    }

    /// `pay` for Private invoices: verifies `sha256(access_code)` against the stored hash.
    pub fn pay_with_access_code(
        env: Env,
        payer: Address,
        invoice_id: u64,
        amount: i128,
        nonce: u64,
        access_code: Option<Bytes>,
    ) {
        require_fn_not_paused(&env, &symbol_short!("pay"));
        require_not_frozen(&env);
        payer.require_auth();
        if let Some(InvoiceVisibility::Private(hash)) = env
            .storage()
            .persistent()
            .get::<_, InvoiceVisibility>(&vis_key(invoice_id))
        {
            let code = access_code.expect("AccessCodeRequired");
            let got: BytesN<32> = env.crypto().sha256(&code).into();
            assert!(got == hash, "InvalidAccessCode");
            env.storage().temporary().set(&access_ok_key(invoice_id, &payer), &true);
        }
        Self::enforce_invoice_rate_limit(&env, invoice_id, &payer);
        Self::_pay(&env, &payer, invoice_id, amount, nonce, false, None, None, false);
    }
}
