//! Contract security audit framework (issue #892).
//!
//! Provides an on-chain, auditor-driven framework for checking the security
//! posture of the contract and of individual invoices:
//!
//! - **Auditor registry** — the admin registers trusted auditor addresses.
//!   Only registered auditors (or the admin) may run audits or log security
//!   events.
//! - **Contract audit** — [`run_contract_audit`] evaluates contract-level
//!   invariants (admin configured, treasury configured, fee bounds, pause and
//!   circuit-breaker state) and persists a [`SecurityAuditReport`].
//! - **Invoice audit** — [`run_invoice_audit`] evaluates invoice-level
//!   invariants (recipient/amount consistency, positive amounts, duplicate
//!   recipients, funding bounds, split-rule and ratio shape).
//! - **Security event log** — a bounded, append-only log of security
//!   observations reported by auditors.
//!
//! Every audit and logged event emits an event so off-chain monitors can
//! alert on failures without polling storage.

use soroban_sdk::{contracttype, symbol_short, Address, Env, Map, Symbol, Vec};

use crate::events;
use crate::types::{AdminRole, Invoice};

/// Maximum number of entries kept in the security event log. Oldest entries
/// are evicted first once the cap is reached.
pub const MAX_SECURITY_EVENTS: u32 = 100;

/// Maximum number of registered auditors.
pub const MAX_AUDITORS: u32 = 20;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Severity attached to an audit check or a logged security event.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum AuditSeverity {
    Info = 0,
    Low = 1,
    Medium = 2,
    High = 3,
    Critical = 4,
}

impl AuditSeverity {
    /// Weight contributed to a report's `risk_score` when a check of this
    /// severity fails.
    pub fn weight(self) -> u32 {
        match self {
            AuditSeverity::Info => 0,
            AuditSeverity::Low => 1,
            AuditSeverity::Medium => 5,
            AuditSeverity::High => 20,
            AuditSeverity::Critical => 100,
        }
    }
}

/// Outcome of a single audit check.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecurityFinding {
    /// Short identifier of the check (e.g. `adm_set`).
    pub check: Symbol,
    /// Severity of the check if it fails.
    pub severity: AuditSeverity,
    /// `true` when the invariant holds.
    pub passed: bool,
}

/// Persisted result of one audit run.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecurityAuditReport {
    pub audit_id: u64,
    pub auditor: Address,
    /// `None` for a contract-level audit, `Some(id)` for an invoice audit.
    pub invoice_id: Option<u64>,
    pub timestamp: u64,
    pub ledger: u32,
    pub findings: Vec<SecurityFinding>,
    pub passed_count: u32,
    pub failed_count: u32,
    /// Number of failed checks with `Critical` severity.
    pub critical_failures: u32,
    /// Sum of [`AuditSeverity::weight`] over all failed checks.
    pub risk_score: u32,
}

/// One entry in the security event log.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecurityEvent {
    pub reporter: Address,
    pub kind: Symbol,
    pub severity: AuditSeverity,
    pub invoice_id: Option<u64>,
    pub timestamp: u64,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

fn auditors_key() -> Symbol {
    symbol_short!("sec_adtrs")
}

fn audit_counter_key() -> Symbol {
    symbol_short!("sec_cnt")
}

fn audit_report_key(audit_id: u64) -> (Symbol, u64) {
    (symbol_short!("sec_aud"), audit_id)
}

fn security_events_key() -> Symbol {
    symbol_short!("sec_evts")
}

// ---------------------------------------------------------------------------
// Auditor registry
// ---------------------------------------------------------------------------

pub fn get_auditors(env: &Env) -> Vec<Address> {
    env.storage()
        .persistent()
        .get(&auditors_key())
        .unwrap_or_else(|| Vec::new(env))
}

pub fn is_auditor(env: &Env, addr: &Address) -> bool {
    get_auditors(env).contains(addr)
}

pub fn add_auditor(env: &Env, auditor: &Address) {
    let mut auditors = get_auditors(env);
    if auditors.contains(auditor) {
        return;
    }
    assert!(auditors.len() < MAX_AUDITORS, "auditor limit reached");
    auditors.push_back(auditor.clone());
    env.storage().persistent().set(&auditors_key(), &auditors);
    events::security_auditor_updated(env, auditor, true);
}

pub fn remove_auditor(env: &Env, auditor: &Address) {
    let auditors = get_auditors(env);
    let mut kept: Vec<Address> = Vec::new(env);
    let mut removed = false;
    for a in auditors.iter() {
        if a == *auditor {
            removed = true;
        } else {
            kept.push_back(a);
        }
    }
    assert!(removed, "auditor not registered");
    env.storage().persistent().set(&auditors_key(), &kept);
    events::security_auditor_updated(env, auditor, false);
}

/// Require that `caller` authorised the call and is either a registered
/// auditor or the contract admin.
pub fn require_auditor(env: &Env, caller: &Address) {
    caller.require_auth();
    let admin: Option<Address> = env.storage().instance().get(&crate::admin_key());
    let is_admin = admin.map(|a| a == *caller).unwrap_or(false);
    assert!(
        is_admin || is_auditor(env, caller),
        "caller is not a registered auditor"
    );
}

// ---------------------------------------------------------------------------
// Checks
// ---------------------------------------------------------------------------

fn finding(check: Symbol, severity: AuditSeverity, passed: bool) -> SecurityFinding {
    SecurityFinding {
        check,
        severity,
        passed,
    }
}

/// Evaluate contract-level security invariants without writing to storage.
pub fn contract_findings(env: &Env) -> Vec<SecurityFinding> {
    let mut findings: Vec<SecurityFinding> = Vec::new(env);

    let admin_set = env.storage().instance().has(&crate::admin_key());
    findings.push_back(finding(
        symbol_short!("adm_set"),
        AuditSeverity::Critical,
        admin_set,
    ));

    let roles_set = env
        .storage()
        .instance()
        .get::<_, Map<Address, AdminRole>>(&crate::admins_key())
        .map(|m| !m.is_empty())
        .unwrap_or(false);
    findings.push_back(finding(
        symbol_short!("adm_roles"),
        AuditSeverity::High,
        roles_set,
    ));

    let treasury_set = env.storage().instance().has(&crate::treasury_key());
    findings.push_back(finding(
        symbol_short!("trsy_set"),
        AuditSeverity::High,
        treasury_set,
    ));

    let fee_bps: u32 = env
        .storage()
        .instance()
        .get(&crate::platform_fee_bps_key())
        .unwrap_or(0);
    findings.push_back(finding(
        symbol_short!("fee_bnd"),
        AuditSeverity::High,
        fee_bps <= 10_000,
    ));

    let cb_active: bool = env
        .storage()
        .persistent()
        .get(&crate::circuit_breaker_key())
        .unwrap_or(false);
    findings.push_back(finding(
        symbol_short!("cb_off"),
        AuditSeverity::Medium,
        !cb_active,
    ));

    findings.push_back(finding(
        symbol_short!("not_psd"),
        AuditSeverity::Info,
        !crate::is_paused(env),
    ));

    findings
}

/// Evaluate invoice-level security invariants without writing to storage.
pub fn invoice_findings(env: &Env, invoice: &Invoice) -> Vec<SecurityFinding> {
    let mut findings: Vec<SecurityFinding> = Vec::new(env);
    let n = invoice.recipients.len();

    findings.push_back(finding(
        symbol_short!("inv_rcpt"),
        AuditSeverity::Critical,
        n > 0 && n == invoice.amounts.len(),
    ));

    let amounts_positive = invoice.amounts.iter().all(|a| a > 0);
    findings.push_back(finding(
        symbol_short!("inv_amts"),
        AuditSeverity::High,
        amounts_positive,
    ));

    let mut no_dupes = true;
    for i in 0..n {
        let a = invoice.recipients.get(i).unwrap();
        for j in (i + 1)..n {
            if invoice.recipients.get(j).unwrap() == a {
                no_dupes = false;
            }
        }
    }
    findings.push_back(finding(
        symbol_short!("inv_dupe"),
        AuditSeverity::High,
        no_dupes,
    ));

    let mut total: i128 = 0;
    let mut total_ok = true;
    for a in invoice.amounts.iter() {
        match total.checked_add(a) {
            Some(t) => total = t,
            None => total_ok = false,
        }
    }
    findings.push_back(finding(
        symbol_short!("inv_ovfl"),
        AuditSeverity::Critical,
        total_ok,
    ));

    findings.push_back(finding(
        symbol_short!("inv_fund"),
        AuditSeverity::Medium,
        invoice.funded >= 0 && (!total_ok || invoice.funded <= total),
    ));

    findings.push_back(finding(
        symbol_short!("inv_rule"),
        AuditSeverity::Medium,
        invoice.split_rules.is_empty() || invoice.split_rules.len() == n,
    ));

    findings.push_back(finding(
        symbol_short!("inv_ratio"),
        AuditSeverity::Medium,
        invoice.ratios.is_empty() || invoice.ratios.len() == n,
    ));

    findings
}

// ---------------------------------------------------------------------------
// Reports
// ---------------------------------------------------------------------------

fn next_audit_id(env: &Env) -> u64 {
    let id: u64 = env
        .storage()
        .persistent()
        .get(&audit_counter_key())
        .unwrap_or(0u64)
        + 1;
    env.storage().persistent().set(&audit_counter_key(), &id);
    id
}

fn build_and_store_report(
    env: &Env,
    auditor: &Address,
    invoice_id: Option<u64>,
    findings: Vec<SecurityFinding>,
) -> SecurityAuditReport {
    let mut passed_count: u32 = 0;
    let mut failed_count: u32 = 0;
    let mut critical_failures: u32 = 0;
    let mut risk_score: u32 = 0;
    for f in findings.iter() {
        if f.passed {
            passed_count += 1;
        } else {
            failed_count += 1;
            risk_score = risk_score.saturating_add(f.severity.weight());
            if f.severity == AuditSeverity::Critical {
                critical_failures += 1;
            }
        }
    }

    let report = SecurityAuditReport {
        audit_id: next_audit_id(env),
        auditor: auditor.clone(),
        invoice_id,
        timestamp: env.ledger().timestamp(),
        ledger: env.ledger().sequence(),
        findings,
        passed_count,
        failed_count,
        critical_failures,
        risk_score,
    };
    env.storage()
        .persistent()
        .set(&audit_report_key(report.audit_id), &report);
    events::security_audit_completed(
        env,
        report.audit_id,
        auditor,
        invoice_id,
        failed_count,
        risk_score,
    );
    report
}

pub fn run_contract_audit(env: &Env, auditor: &Address) -> SecurityAuditReport {
    require_auditor(env, auditor);
    let findings = contract_findings(env);
    build_and_store_report(env, auditor, None, findings)
}

pub fn run_invoice_audit(env: &Env, auditor: &Address, invoice_id: u64) -> SecurityAuditReport {
    require_auditor(env, auditor);
    let invoice = crate::load_invoice(env, invoice_id);
    let findings = invoice_findings(env, &invoice);
    build_and_store_report(env, auditor, Some(invoice_id), findings)
}

pub fn get_report(env: &Env, audit_id: u64) -> Option<SecurityAuditReport> {
    env.storage().persistent().get(&audit_report_key(audit_id))
}

pub fn get_audit_count(env: &Env) -> u64 {
    env.storage()
        .persistent()
        .get(&audit_counter_key())
        .unwrap_or(0u64)
}

// ---------------------------------------------------------------------------
// Security event log
// ---------------------------------------------------------------------------

pub fn get_security_events(env: &Env) -> Vec<SecurityEvent> {
    env.storage()
        .persistent()
        .get(&security_events_key())
        .unwrap_or_else(|| Vec::new(env))
}

pub fn log_security_event(
    env: &Env,
    reporter: &Address,
    kind: Symbol,
    severity: AuditSeverity,
    invoice_id: Option<u64>,
) {
    require_auditor(env, reporter);
    let mut log = get_security_events(env);
    while log.len() >= MAX_SECURITY_EVENTS {
        log.pop_front();
    }
    log.push_back(SecurityEvent {
        reporter: reporter.clone(),
        kind: kind.clone(),
        severity,
        invoice_id,
        timestamp: env.ledger().timestamp(),
    });
    env.storage().persistent().set(&security_events_key(), &log);
    events::security_event_logged(env, reporter, &kind, severity as u32, invoice_id);
}
