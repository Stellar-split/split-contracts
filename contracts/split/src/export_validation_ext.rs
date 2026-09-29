//! Issue #820: invoice export with metadata schema validation.
//!
//! `export_invoice` produces a versioned, flat [`InvoiceExport`] record.
//! `validate_invoice_export` checks a record against the schema rules so that
//! off-chain consumers can verify an export before trusting it.

use super::*;
use soroban_sdk::{contractimpl, contracttype, symbol_short, Address, Env, Vec};

pub const EXPORT_SCHEMA_VERSION: u32 = 1;

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct InvoiceExport {
    pub schema_version: u32,
    pub invoice_id: u64,
    pub creator: Address,
    pub recipients: Vec<Address>,
    pub amounts: Vec<i128>,
    pub funding_token: Address,
    pub total: i128,
    pub funded: i128,
    pub deadline: u64,
    pub status: InvoiceStatus,
}

fn is_compliant(e: &InvoiceExport) -> bool {
    if e.schema_version != EXPORT_SCHEMA_VERSION
        || e.recipients.is_empty()
        || e.recipients.len() != e.amounts.len()
        || e.funded < 0
        || e.funded > e.total
    {
        return false;
    }
    let mut sum: i128 = 0;
    for a in e.amounts.iter() {
        if a <= 0 {
            return false;
        }
        sum = match sum.checked_add(a) {
            Some(s) => s,
            None => return false,
        };
    }
    sum == e.total
}

#[contractimpl]
impl SplitContract {
    /// Export an invoice as a schema-versioned record.
    pub fn export_invoice(env: Env, invoice_id: u64) -> InvoiceExport {
        let inv = load_invoice(&env, invoice_id);
        InvoiceExport {
            schema_version: EXPORT_SCHEMA_VERSION,
            invoice_id,
            creator: inv.creator,
            total: inv.amounts.iter().sum(),
            recipients: inv.recipients,
            amounts: inv.amounts,
            funding_token: inv.funding_token,
            funded: inv.funded,
            deadline: inv.deadline,
            status: inv.status,
        }
    }

    /// Validate an export against the schema; emits `exp_val` with the result.
    pub fn validate_invoice_export(env: Env, export: InvoiceExport) -> bool {
        let ok = is_compliant(&export);
        env.events().publish(
            (
                symbol_short!("split"),
                symbol_short!("exp_val"),
                export.invoice_id,
            ),
            ok,
        );
        ok
    }
}

#[cfg(test)]
mod tests {
    use crate::ext_test_util::{fixture, new_invoice};

    #[test]
    fn export_is_valid() {
        let f = fixture();
        let (id, creator, _) = new_invoice(&f, 500);
        let e = f.c.export_invoice(&id);
        assert_eq!(e.total, 500);
        assert_eq!(e.creator, creator);
        assert!(f.c.validate_invoice_export(&e));
    }

    #[test]
    fn tampered_export_is_invalid() {
        let f = fixture();
        let (id, _, _) = new_invoice(&f, 500);
        let base = f.c.export_invoice(&id);

        let mut e = base.clone();
        e.total = 400;
        assert!(!f.c.validate_invoice_export(&e));

        let mut e = base.clone();
        e.schema_version = 2;
        assert!(!f.c.validate_invoice_export(&e));

        let mut e = base.clone();
        e.funded = 600;
        assert!(!f.c.validate_invoice_export(&e));

        let mut e = base;
        e.amounts.set(0, -1);
        assert!(!f.c.validate_invoice_export(&e));
    }
}
