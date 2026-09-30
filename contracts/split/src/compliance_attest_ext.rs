//! Issue #879: invoice compliance attestation system.
//!
//! Authorised compliance officers can submit structured compliance records
//! against an invoice, stating the compliance framework they applied (e.g.
//! "AML", "KYC", "GDPR"), the outcome, and an optional notes string.
//! Records are append-only (an officer may not overwrite a prior record) and
//! capped at 10 per invoice.
//!
//! ## Roles
//! - `admin` (the initialised contract admin) calls `register_compliance_officer`
//!   / `remove_compliance_officer` to manage the officer set.
//! - Any registered officer calls `submit_compliance_record` to attest.
//! - Anyone may call `get_compliance_records` / `is_compliance_officer`.
//!
//! ## Events
//! - `(split, co_add)` — data: `officer` — officer registered
//! - `(split, co_rem)` — data: `officer` — officer removed
//! - `(split, cmp_sub, invoice_id)` — data: `(officer, framework, passed)` — record submitted
//!
//! ## Storage
//! Uses a local `ComplianceAttestKey` enum in **persistent** storage and
//! the `Officers` entry in **instance** storage.

use super::*;
use soroban_sdk::{contractimpl, contracttype, symbol_short, Address, Env, String, Vec};

/// Maximum length (in bytes) of a compliance framework name (e.g. "KYC").
pub const MAX_FRAMEWORK_LEN: u32 = 32;
/// Maximum length of the optional notes field.
pub const MAX_NOTES_LEN: u32 = 512;
/// Maximum number of compliance records per invoice.
pub const MAX_RECORDS_PER_INVOICE: u32 = 10;

/// A single compliance attestation record.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ComplianceRecord {
    /// Address of the compliance officer who submitted the record.
    pub officer: Address,
    /// Compliance framework identifier (e.g. "AML", "KYC").
    pub framework: String,
    /// Whether the invoice passed the compliance check.
    pub passed: bool,
    /// Optional free-form notes (max 512 bytes).
    pub notes: Option<String>,
    /// Ledger timestamp of submission.
    pub timestamp: u64,
}

/// Persistent storage keys for the compliance attestation module.
#[contracttype]
#[derive(Clone)]
pub enum ComplianceAttestKey {
    /// `Vec<ComplianceRecord>` for an invoice.
    Records(u64),
    /// `Vec<Address>` registered compliance officers (instance storage key sentinel).
    Officers,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn load_records(env: &Env, invoice_id: u64) -> Vec<ComplianceRecord> {
    env.storage()
        .persistent()
        .get(&ComplianceAttestKey::Records(invoice_id))
        .unwrap_or_else(|| Vec::new(env))
}

fn load_officers(env: &Env) -> Vec<Address> {
    env.storage()
        .instance()
        .get(&ComplianceAttestKey::Officers)
        .unwrap_or_else(|| Vec::new(env))
}

fn save_officers(env: &Env, list: &Vec<Address>) {
    env.storage()
        .instance()
        .set(&ComplianceAttestKey::Officers, list);
}

fn require_admin(env: &Env, caller: &Address) {
    let admin: Address = env
        .storage()
        .instance()
        .get(&symbol_short!("admin"))
        .expect("contract not initialised");
    assert!(*caller == admin, "only admin can manage compliance officers");
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

#[contractimpl]
impl SplitContract {
    /// Register `officer` as a compliance officer (admin only).
    pub fn register_compliance_officer(env: Env, admin: Address, officer: Address) {
        require_not_paused(&env);
        admin.require_auth();
        require_admin(&env, &admin);

        let mut list = load_officers(&env);
        assert!(
            !list.contains(officer.clone()),
            "officer already registered"
        );
        list.push_back(officer.clone());
        save_officers(&env, &list);

        env.events().publish(
            (symbol_short!("split"), symbol_short!("co_add")),
            officer,
        );
    }

    /// Remove `officer` from the compliance officer set (admin only).
    pub fn remove_compliance_officer(env: Env, admin: Address, officer: Address) {
        require_not_paused(&env);
        admin.require_auth();
        require_admin(&env, &admin);

        let list = load_officers(&env);
        let mut new_list = Vec::new(&env);
        let mut found = false;
        for o in list.iter() {
            if o == officer {
                found = true;
            } else {
                new_list.push_back(o);
            }
        }
        assert!(found, "officer not registered");
        save_officers(&env, &new_list);

        env.events().publish(
            (symbol_short!("split"), symbol_short!("co_rem")),
            officer,
        );
    }

    /// Returns `true` if `officer` is a registered compliance officer.
    pub fn is_compliance_officer(env: Env, officer: Address) -> bool {
        load_officers(&env).contains(officer)
    }

    /// Submit a compliance record for `invoice_id`.
    ///
    /// - Caller must be a registered compliance officer.
    /// - An officer may submit at most one record per invoice per framework.
    /// - `framework` must be ≤ 32 bytes; `notes` (if provided) ≤ 512 bytes.
    pub fn submit_compliance_record(
        env: Env,
        officer: Address,
        invoice_id: u64,
        framework: String,
        passed: bool,
        notes: Option<String>,
    ) {
        require_not_paused(&env);
        officer.require_auth();

        let officers = load_officers(&env);
        assert!(officers.contains(officer.clone()), "caller is not a compliance officer");
        assert!(
            framework.len() > 0 && framework.len() <= MAX_FRAMEWORK_LEN,
            "invalid framework length"
        );
        if let Some(ref n) = notes {
            assert!(n.len() <= MAX_NOTES_LEN, "notes too long");
        }

        // Verify the invoice exists.
        load_invoice(&env, invoice_id);

        let mut records = load_records(&env, invoice_id);
        assert!(records.len() < MAX_RECORDS_PER_INVOICE, "compliance record limit reached");

        // Prevent the same officer from submitting duplicate framework records.
        assert!(
            !records
                .iter()
                .any(|r| r.officer == officer && r.framework == framework),
            "officer already submitted this framework"
        );

        records.push_back(ComplianceRecord {
            officer: officer.clone(),
            framework: framework.clone(),
            passed,
            notes,
            timestamp: env.ledger().timestamp(),
        });
        env.storage()
            .persistent()
            .set(&ComplianceAttestKey::Records(invoice_id), &records);

        env.events().publish(
            (symbol_short!("split"), symbol_short!("cmp_sub"), invoice_id),
            (officer, framework, passed),
        );
    }

    /// Returns all compliance records for `invoice_id`.
    pub fn get_compliance_records(env: Env, invoice_id: u64) -> Vec<ComplianceRecord> {
        load_records(&env, invoice_id)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ext_test_util::{fixture, new_invoice};
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{Address, String};

    fn fw(env: &soroban_sdk::Env, s: &str) -> String {
        String::from_str(env, s)
    }

    #[test]
    fn register_and_check_officer() {
        let f = fixture();
        let officer = Address::generate(&f.env);
        assert!(!f.c.is_compliance_officer(&officer));
        f.c.register_compliance_officer(&f.admin, &officer);
        assert!(f.c.is_compliance_officer(&officer));
    }

    #[test]
    fn remove_officer() {
        let f = fixture();
        let officer = Address::generate(&f.env);
        f.c.register_compliance_officer(&f.admin, &officer);
        f.c.remove_compliance_officer(&f.admin, &officer);
        assert!(!f.c.is_compliance_officer(&officer));
    }

    #[test]
    fn submit_and_get_record() {
        let f = fixture();
        let (id, _, _) = new_invoice(&f, 1_000);
        let officer = Address::generate(&f.env);
        f.c.register_compliance_officer(&f.admin, &officer);
        f.c.submit_compliance_record(&officer, &id, &fw(&f.env, "KYC"), &true, &None);
        let records = f.c.get_compliance_records(&id);
        assert_eq!(records.len(), 1);
        assert_eq!(records.get(0).unwrap().officer, officer);
        assert_eq!(records.get(0).unwrap().passed, true);
    }

    #[test]
    #[should_panic(expected = "caller is not a compliance officer")]
    fn non_officer_cannot_submit() {
        let f = fixture();
        let (id, _, _) = new_invoice(&f, 1_000);
        let stranger = Address::generate(&f.env);
        f.c.submit_compliance_record(&stranger, &id, &fw(&f.env, "KYC"), &true, &None);
    }

    #[test]
    #[should_panic(expected = "officer already submitted this framework")]
    fn duplicate_framework_per_officer_rejected() {
        let f = fixture();
        let (id, _, _) = new_invoice(&f, 1_000);
        let officer = Address::generate(&f.env);
        f.c.register_compliance_officer(&f.admin, &officer);
        f.c.submit_compliance_record(&officer, &id, &fw(&f.env, "AML"), &true, &None);
        f.c.submit_compliance_record(&officer, &id, &fw(&f.env, "AML"), &false, &None);
    }

    #[test]
    #[should_panic(expected = "only admin can manage compliance officers")]
    fn non_admin_cannot_register_officer() {
        let f = fixture();
        let stranger = Address::generate(&f.env);
        let officer = Address::generate(&f.env);
        f.c.register_compliance_officer(&stranger, &officer);
    }

    #[test]
    fn multiple_officers_same_invoice() {
        let f = fixture();
        let (id, _, _) = new_invoice(&f, 1_000);
        let o1 = Address::generate(&f.env);
        let o2 = Address::generate(&f.env);
        f.c.register_compliance_officer(&f.admin, &o1);
        f.c.register_compliance_officer(&f.admin, &o2);
        f.c.submit_compliance_record(&o1, &id, &fw(&f.env, "KYC"), &true, &None);
        f.c.submit_compliance_record(&o2, &id, &fw(&f.env, "AML"), &false, &None);
        let records = f.c.get_compliance_records(&id);
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn officer_can_submit_different_frameworks() {
        let f = fixture();
        let (id, _, _) = new_invoice(&f, 1_000);
        let officer = Address::generate(&f.env);
        f.c.register_compliance_officer(&f.admin, &officer);
        f.c.submit_compliance_record(&officer, &id, &fw(&f.env, "KYC"), &true, &None);
        f.c.submit_compliance_record(&officer, &id, &fw(&f.env, "AML"), &true, &None);
        let records = f.c.get_compliance_records(&id);
        assert_eq!(records.len(), 2);
    }
}
