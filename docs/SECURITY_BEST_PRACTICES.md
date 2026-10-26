# StellarSplit Smart Contract Security Best Practices Guide

> **Issue**: `#1036`  
> **Scope**: `contracts/split`, `contracts/invoice-escrow`, and supporting Soroban operational tooling  
> **Target Audience**: Core protocol developers, smart contract auditors, integrators, and DevOps/security responders

---

## Table of Contents

1. [Overview & Security Philosophy](#1-overview--security-philosophy)
2. [Core Security Concerns & Threat Model](#2-core-security-concerns--threat-model)
3. [Soroban & StellarSplit Security Best Practices](#3-soroban--stellarsplit-security-best-practices)
4. [Vulnerability Guide & Mitigation Catalog](#4-vulnerability-guide--mitigation-catalog)
5. [Comprehensive Security Checklists](#5-comprehensive-security-checklists)
6. [On-Chain Security Auditing & Continuous Verification](#6-on-chain-security-auditing--continuous-verification)
7. [Incident Response & Emergency Operations Runbook](#7-incident-response--emergency-operations-runbook)
8. [Security References & Resources](#8-security-references--resources)

---

## 1. Overview & Security Philosophy

StellarSplit processes multi-party invoice settlements, escrow releases, recurring subscriptions, cross-currency FX swaps, yield deposits, and cross-chain payments on the Stellar network using Soroban smart contracts. Because these workflows custody and route real-value SAC (Stellar Asset Contract) tokens, security is enforced through **defense-in-depth**:

1. **Explicit Authorization at Every Boundary**: Every state-mutating entrypoint verifies caller identity with `Address::require_auth()` and role-based permissions (`Role`, `Permission`) before reading or mutating sensitive state.
2. **Deterministic Conservation of Value**: Every payment, fee deduction, partial refund, penalty, and recipient payout must preserve strict mathematical invariants (`sum(shares) + fees == total_inflow`), verified continuously by on-chain invariant checks.
3. **Fail-Closed Validation & Circuit Breakers**: Invalid inputs, expired deadlines, stale oracle quotes, and anomalous transaction volumes fail immediately with typed `ContractError` variants and can automatically trip the on-chain circuit breaker.
4. **Tamper-Evident Auditability**: High-sensitivity actions emit both Soroban events and hash-chained on-chain `AuditLogEntry` records (`contracts/split/src/audit_logging.rs`).

---

## 2. Core Security Concerns & Threat Model

### 2.1 Trust Boundaries & Actors

| Actor | Capabilities | Trust Assumption | Primary Risks |
|-------|-------------|------------------|---------------|
| **Invoice Creator** | Creates invoices, configures recipients/splits/milestones, cancels prior to funding | Untrusted external user | Malicious recipient lists, dust-amount griefing, self-dealing disputes |
| **Payer / Contributor** | Funds invoices directly, via meta-tx intent, DEX swap, or bridge relay | Untrusted external user | Replay of signed intents, underpayment, front-running deadline expiry |
| **Recipient** | Receives released payouts, claims tax/compliance credits | Untrusted external user | Blocking payouts via rogue contract callbacks or frozen trustlines |
| **Arbitrator / Arbiter** | Votes on or resolves disputed invoices | Semi-trusted role-gated actor | Collusion, Sybil voting, stalled quorum |
| **Oracle / FX Provider** | Supplies token exchange rates (`OracleRate`) | Semi-trusted external feed | Stale rates, flash manipulation, zero/negative rate injection |
| **Contract Admin / Governance** | Configures fees, roles, emergency pause, and WASM upgrades | Privileged multi-sig / timelocked authority | Key compromise, malicious upgrade, accidental misconfiguration |

### 2.2 Key Security Concerns in Payment Splitting Contracts

1. **Unauthorized Fund Movement**: Calling `release`, `refund`, `sweep_dust`, or `execute_upgrade` without proper caller authentication or state preconditions.
2. **Cross-Contract Reentrancy**: Invoking external SAC token contracts (`token::Client::new(&env, &token).transfer(...)`) or external yield/DEX contracts before updating internal invoice balances and status flags.
3. **Arithmetic Overflow, Underflow & Rounding Drift**: Splitting odd amounts across $N$ recipients using basis points (`bps`) or percentages can leave unallocated dust or over-allocate funds if rounding remainders are not explicitly handled.
4. **Signature & Proof Replay**: Re-submitting a valid Ed25519 `PaymentIntent` or cross-chain `BridgeProof` multiple times to credit an invoice without transferring new funds.
5. **Soroban Resource Exhaustion (DoS)**: Unbounded `Vec` growth in recipients, payers, milestones, or audit logs exceeding Soroban CPU instruction or ledger read/write byte limits, permanently bricking an invoice.
6. **Storage Key Collision & TTL Expiration**: Overlapping XDR storage keys corrupting unrelated contract records, or persistent storage entries expiring from the ledger before long-dated invoices settle.

---

## 3. Soroban & StellarSplit Security Best Practices

### 3.1 Authentication & Role-Based Access Control (RBAC)

- **Always call `require_auth()` first**: Authenticate the immediate actor (`payer.require_auth()`, `creator.require_auth()`, `admin.require_auth()`) before performing storage writes or external calls.
- **Use fine-grained RBAC instead of a single monolithic admin key**:
  - StellarSplit defines explicit roles (`Role::SuperAdmin`, `Role::FinancialAdmin`, `Role::EmergencyAdmin`, `Role::UpgradeAdmin`, `Role::Operator`, `Role::Auditor`, `Role::FeeAdmin`) and permissions (`Permission::PauseContract`, `Permission::ManageFees`, `Permission::ProposeUpgrade`, etc.).
  - Always check permissions via `require_permission(&env, &caller, Permission::...)` rather than ad-hoc address comparisons.
- **Require multi-sig / quorum for critical operations**:
  - Emergency recovery (`approve_emergency_recovery`), contract upgrades (`approve_upgrade`), and multi-sig invoice operations (`OperationType`) enforce multi-party approval thresholds and timelock delays.

```rust
// GOOD: Explicit authentication + RBAC permission + pause check
pub fn set_fee_bps(env: Env, admin: Address, fee_bps: u32) {
    admin.require_auth();
    Self::require_not_paused(&env);
    Self::require_permission(&env, &admin, Permission::ManageFees);
    if fee_bps > MAX_FEE_BPS {
        panic_with_error!(&env, ContractError::FeeTooHigh);
    }
    env.storage().instance().set(&storage_keys::fee_bps_key(), &fee_bps);
}
```

### 3.2 Reentrancy Protection & Checks-Effects-Interactions (CEI)

Even though Soroban's host environment restricts direct recursive re-invocation of the same contract frame, **logical reentrancy across multi-contract call graphs** (such as custom token hooks, DEX routers, or yield vaults calling back into secondary view/mutating functions) remains a critical concern.

- **Follow Checks-Effects-Interactions (CEI)**:
  1. **Checks**: Validate pause state, invoice status, deadlines, caller auth, and amounts.
  2. **Effects**: Update `invoice.status` (e.g. `InvoiceStatus::Released` or `InvoiceStatus::Refunded`), zero out pending balances, and persist the updated struct to `env.storage().persistent()`.
  3. **Interactions**: Execute external `token_client.transfer(...)` calls last.
- **Use the Contract Reentrancy Guard**:
  - Acquire the storage-backed `ReentrancyGuard` (`storage_keys::reentrancy_key()`) around external token and DeFi interactions.

### 3.3 Safe Arithmetic & Conservation of Value

- **Never use unchecked `+`, `-`, `*`, `/` on financial quantities (`i128`, `u64`)**:
  - Always enable `overflow-checks = true` in `Cargo.toml` release profiles and use explicit `checked_add`, `checked_sub`, `checked_mul`, and `checked_div` returning `ContractError::Overflow`.
- **Enforce strictly positive amounts**:
  - Reject `amount <= 0` immediately with `ContractError::InvalidAmount` (`#10`). In `i128` token accounting, a negative payment amount without validation acts as an unauthorized withdrawal.
- **Handle rounding remainders deterministically**:
  - When splitting an invoice total across basis-point shares (`10_000 bps = 100%`), compute `N - 1` shares via `total.checked_mul(bps)?.checked_div(10_000)?` and allocate or track the exact remainder so `sum(distributed) + remainder == total`.
- **Run post-condition invariant verification**:
  - Use `verify_invoice_invariant(&env, invoice_id)` to confirm that total collected funds match recorded payer contributions and that released invoices hold zero stranded balance.

### 3.4 Cryptographic Signature & Replay Protection

- **Domain-separate all signed payloads**:
  - Every off-chain signature (`PaymentIntent`, `BridgeProof`, meta-transaction) must serialize the contract environment / chain identifier, invoice ID, amount, deadline, and a unique nonce or transaction hash before calling `env.crypto().ed25519_verify(&pk, &msg, &sig)`.
- **Persist consumed nonces and transaction hashes**:
  - For meta-transactions, increment the per-payer nonce (`storage_keys::nonce_key(&payer)`) prior to execution.
  - For cross-chain bridge proofs, record the source transaction hash in persistent storage and reject duplicates with `ContractError::AlreadyConsumed`.

### 3.5 Bounded Storage, Gas Safety & TTL Management

- **Enforce strict collection bounds (`ValidationRules`)**:
  - Cap recipient counts (`MAX_RECIPIENTS`), description byte lengths (`max_string_len`), metadata keys (`max_metadata_entries`), and batch sizes (`MAX_BATCH_SIZE`) at ingress (`contracts/split/src/validation.rs`).
- **Centralize all storage keys in `storage_keys.rs`**:
  - Never inline ad-hoc `Symbol::new(&env, "...")` keys inside business logic. Register every key constructor in `contracts/split/src/storage_keys.rs` and `contracts/split/src/storage_snapshot.rs`, and verify zero XDR collisions via `cargo test -p split storage_snapshot`.
- **Extend TTLs proactively**:
  - Whenever an invoice, dispute, escrow, or compliance record is created or mutated, extend its persistent storage TTL (`env.storage().persistent().extend_ttl(...)`) beyond the invoice deadline plus dispute/appeal windows.

---

## 4. Vulnerability Guide & Mitigation Catalog

### V-01: Missing or Misplaced `require_auth()` (CWE-862)
- **Severity**: Critical
- **Description**: An entrypoint accepts an `Address` parameter (such as `admin`, `creator`, or `payer`) and trusts it without calling `address.require_auth()`, allowing any caller to impersonate that address.
- **Vulnerable Pattern**:
  ```rust
  pub fn cancel_invoice(env: Env, creator: Address, invoice_id: u64) {
      let invoice = Self::load_invoice(&env, invoice_id);
      assert_eq!(invoice.creator, creator); // BUG: creator never signed the transaction!
  }
  ```
- **Safe Mitigation**:
  ```rust
  pub fn cancel_invoice(env: Env, creator: Address, invoice_id: u64) {
      creator.require_auth(); // Host verifies cryptographic signature / auth tree
      let invoice = Self::load_invoice(&env, invoice_id);
      if invoice.creator != creator {
          panic_with_error!(&env, ContractError::Unauthorized);
      }
  }
  ```
- **On-Chain Audit Detection**: Flagged under `AuditCategory::AccessControl` in `security_audit.rs`.

### V-02: State Mutation After External Token Transfer (Reentrancy / CEI Violation)
- **Severity**: Critical
- **Description**: Calling `token_client.transfer()` before marking `invoice.status = InvoiceStatus::Released` or deducting the claimable balance allows a malicious token hook or cross-contract callback to trigger a second payout or refund.
- **Safe Mitigation**: Mutate and save `invoice.status` to persistent storage *before* invoking `token_client.transfer()`, and wrap external calls with `ReentrancyGuard`.
- **On-Chain Audit Detection**: Flagged under `AuditCategory::Reentrancy` (`check_reentrancy_guard_stuck`) in `security_audit.rs`.

### V-03: Negative `i128` Amount Injection (CWE-20)
- **Severity**: Critical
- **Description**: Soroban's standard token interface uses signed `i128` for amounts. Passing `0` or a negative `i128` into payment, deposit, or split functions can corrupt accounting totals or invert transfer directions.
- **Safe Mitigation**: Validate `amount > 0` at the top of every entrypoint (`ValidationRules::min_amount`) and panic with `ContractError::InvalidAmount`.

### V-04: Signature & Relay Proof Replay (CWE-294)
- **Severity**: High
- **Description**: A signed `PaymentIntent` or cross-chain `BridgeProof` lacks a monotonic nonce, expiration timestamp, or consumed-hash registry, allowing an attacker to replay the same proof repeatedly.
- **Safe Mitigation**:
  - Verify `env.ledger().timestamp() <= intent.deadline`.
  - Check and increment `nonce_key(&payer)` or mark `bridge_tx_key(&tx_hash)` as consumed before crediting the invoice.

### V-05: Stale or Manipulated Oracle FX Rates (CWE-345)
- **Severity**: High
- **Description**: Accepting an `OracleRate` with `rate <= 0` or an outdated `updated_at` timestamp allows payers to settle multi-currency invoices at a fraction of their true value.
- **Safe Mitigation**:
  - Enforce `rate > 0` (`ContractError::InvalidOracleRate`).
  - Enforce `env.ledger().timestamp().saturating_sub(rate.updated_at) <= max_staleness` (`ContractError::OracleRateStale`).
  - Enforce `slippage_bps <= max_slippage_bps` (`ContractError::SlippageExceeded`).

### V-06: Unbounded Loop Resource Exhaustion (DoS)
- **Severity**: High
- **Description**: Iterating over an unbounded `Vec` of recipients, payers, or audit logs in a single transaction exceeds Soroban's per-transaction CPU instruction or ledger entry limits, making `release()` or `refund()` permanently uncallable.
- **Safe Mitigation**:
  - Enforce `MAX_RECIPIENTS` and `MAX_BATCH_SIZE` on creation.
  - Use paginated queries (`offset`, `limit`) and ring-buffer log rotation (`max_audit_entries`) for historical data.

### V-07: Storage Key Collision Across Modules
- **Severity**: High
- **Description**: Two modules independently construct a tuple storage key such as `(Symbol::new(&env, "dispute"), id)`, silently overwriting each other's persistent storage entries.
- **Safe Mitigation**: Define all storage keys in `contracts/split/src/storage_keys.rs` and enforce uniqueness via the XDR snapshot test (`tests/snapshots/storage_keys.json`).

### V-08: Unchecked Contract Upgrade / Missing Rollback Window
- **Severity**: High
- **Description**: Upgrading contract WASM directly via a single key without version monotonicity, approval quorum, or rollback state capture can brick the contract or introduce backdoors.
- **Safe Mitigation**: Use the staged upgrade pipeline in `contract_upgrade.rs` (`propose_upgrade` -> multi-admin `approve_upgrade` -> timelocked `execute_upgrade` with automatic `RollbackData` snapshot and `rollback_upgrade` support).

---

## 5. Comprehensive Security Checklists

### 5.1 Pre-Development & Architecture Checklist
- [ ] **Threat Model Defined**: Are all new actors, roles, and trust assumptions documented?
- [ ] **Storage Keys Registered**: Are all new storage keys added to `storage_keys.rs` and `storage_snapshot.rs` with zero XDR collisions?
- [ ] **Error Codes Assigned**: Are failure states mapped to explicit `ContractError` variants with recovery suggestions in `error.rs`?
- [ ] **Resource Limits Bounded**: Does every `Vec`, `Map`, and `String` input have an explicit maximum size enforced in `validation.rs`?

### 5.2 Implementation & Code Review Checklist
- [ ] **Auth Verification**: Does every state-changing entrypoint call `require_auth()` on the acting address and verify RBAC `Permission` where applicable?
- [ ] **Emergency Pause Integration**: Does every financial or state-mutating entrypoint call `require_not_paused(&env)` and check function-specific pause flags (`EmergencyPauseManager::is_function_paused`)?
- [ ] **Rate Limiting**: Are user-facing mutating endpoints protected by `RateLimiter::check_and_record` where spam or griefing is possible?
- [ ] **Checked Math**: Are all `+`, `-`, `*`, `/` operations replaced with `checked_add`, `checked_sub`, `checked_mul`, `checked_div` or saturating equivalents?
- [ ] **Positive Amounts**: Are all token amounts and fee parameters validated against `<= 0` and upper bounds (`MAX_FEE_BPS`)?
- [ ] **CEI Ordering**: Are storage state transitions (`Released`, `Refunded`, `Resolved`, consumed nonces/hashes) committed *before* external token transfers?
- [ ] **Audit Logging & Events**: Do sensitive operations emit structured events and record an `AuditLogEntry` via `audit_logging::log_event`?

### 5.3 Pre-Deployment & Release Checklist
- [ ] `RUSTUP_TOOLCHAIN=1.94.1 cargo test --workspace` passes with 100% green unit, property, state-machine, and concurrency tests.
- [ ] `RUSTUP_TOOLCHAIN=1.94.1 cargo clippy --all-targets -- -D warnings` passes with zero warnings.
- [ ] `cargo test -p split storage_snapshot` confirms `tests/snapshots/storage_keys.json` is up to date and collision-free.
- [ ] On-chain self-audit (`run_contract_audit`) achieves a `SecurityScore >= 90` with `critical_count == 0` and `high_count == 0`.
- [ ] Upgrade simulation (`simulate_upgrade`) passes compatibility checks on staging/testnet before proposing on mainnet.

---

## 6. On-Chain Security Auditing & Continuous Verification

StellarSplit includes an embedded on-chain security auditing engine (`contracts/split/src/security_audit.rs`) and invariant verifier that can be invoked directly by auditors, monitoring bots, or governance tooling:

| Function | Purpose | Key Output |
|----------|---------|------------|
| `run_contract_audit(env)` | Audits global contract configuration (admin initialization, RBAC roles, fee caps, reentrancy lock state, pause state, rate-limit rules, circuit breaker) | `SecurityAuditReport` with `security_score` (`0..=100`) and categorized `AuditFinding` list |
| `run_invoice_audit(env, invoice_id)` | Audits a specific invoice for balance conservation, deadline consistency, recipient share sum, and dispute lock validity | `InvoiceAuditResult` (`passed`, `findings`, `balance_discrepancy`) |
| `verify_invoice_invariant(env, invoice_id)` | Verifies mathematical conservation of funds for a single invoice | `bool` / `InvariantCheckResult` |
| `verify_all_invariants(env, max_invoices)` | Batch-verifies conservation invariants across active invoices | `Vec<InvariantCheckResult>` |
| `verify_audit_integrity(env)` | Verifies the cryptographic SHA-256 hash chain across all stored `AuditLogEntry` records | `bool` (`true` if tamper-free) |

---

## 7. Incident Response & Emergency Operations Runbook

When an anomaly, exploit attempt, or critical bug is detected in production, responders must follow this five-phase runbook.

```
[Phase 1: Detect & Triage]
       │
       ▼
[Phase 2: Contain (Emergency Pause / Function Pause / Circuit Breaker)]
       │
       ▼
[Phase 3: Forensics (Audit Trail Export + Invariant & Invoice Audits)]
       │
       ▼
[Phase 4: Remediate (Staged Upgrade / Emergency Recovery / Rollback)]
       │
       ▼
[Phase 5: Verify Invariants & Unpause]
```

### Phase 1: Detection & Triage (Target SLA: < 5 Minutes)
1. **Inspect Monitoring Dashboard & Active Alerts**:
   - Query `get_monitoring_dashboard(env)` and `get_active_alerts(env)` (`contracts/split/src/monitoring.rs`) to identify error rate spikes, high-value transfer anomalies, or circuit breaker trips.
2. **Run Automated Contract Audit**:
   - Invoke `run_contract_audit(env)` to check for stuck reentrancy guards (`ReentrancyGuardStuck`), unauthorized config changes, or abnormal fee settings.
3. **Classify Severity**:
   - **SEV-1 (Critical)**: Active fund drainage, invariant violation (`balance_discrepancy != 0`), or compromised admin/bridge authority key.
   - **SEV-2 (High)**: Single feature malfunction (e.g., stale oracle feed, stuck dispute, bridge relay failure) with funds still locked in contract.
   - **SEV-3 (Medium/Low)**: Elevated rate-limit rejections or non-critical UI/query degradation.

### Phase 2: Immediate Containment (Target SLA: < 10 Minutes)
- **For SEV-1 (Global Freeze)**:
  - Any `EmergencyAdmin` or configured guardian invokes `emergency_pause(env, admin, reason, auto_unpause_after)` (`contracts/split/src/emergency_pause.rs`).
  - All state-mutating entrypoints (`create_invoice`, `pay`, `release`, `refund`, `bridge_pay`) are immediately halted.
- **For SEV-2 (Surgical Function Isolation)**:
  - Invoke `pause_function(env, admin, function_name, reason)` to freeze only the affected entrypoint (e.g., `Symbol::new(&env, "bridge_pay")`) while allowing normal invoice settlements to continue.
- **For Compromised Bridge or Oracle Authority**:
  - Rotate the `bridge_authority` public key or disable the compromised oracle feed immediately.

### Phase 3: Forensics & Impact Assessment
1. **Verify Audit Log Chain**:
   - Call `verify_audit_integrity(env)` to confirm that on-chain logs have not been tampered with.
   - Export affected logs via `export_audit_trail(env, start_time, end_time)` and filter by `AuditSeverity::Critical` or `AuditCategory::SecurityIncident`.
2. **Audit Affected Invoices**:
   - Run `verify_all_invariants(env, limit)` and `run_invoice_audit(env, invoice_id)` on all open/funded invoices to quantify any accounting discrepancy.

### Phase 4: Remediation, Recovery & Upgrade
- **If a code fix is required**:
  1. Build and verify the patched WASM binary (`cargo test --workspace` + `storage_snapshot`).
  2. Propose the upgrade via `propose_upgrade(env, proposer, new_version, wasm_hash, description, migration_required)`.
  3. Collect multi-admin approvals via `approve_upgrade(env, approver, proposal_id)`.
  4. Validate compatibility via `simulate_upgrade(env, proposal_id)` and execute via `execute_upgrade(env, executor, proposal_id)`.
  5. If post-upgrade anomalies occur within the rollback window, immediately invoke `rollback_upgrade(env, admin)`.
- **If stranded funds require emergency recovery**:
  1. Initiate multi-sig timelocked recovery via `initiate_emergency_recovery(env, admin, token, recipient, amount, reason)`.
  2. Collect required guardian approvals via `approve_emergency_recovery(env, approver, request_id)`.
  3. Execute recovery after the mandatory timelock elapses.

### Phase 5: Service Restoration & Post-Mortem
1. Re-run `run_contract_audit(env)` and `verify_all_invariants(env, limit)` to confirm `critical_count == 0` and `balance_discrepancy == 0`.
2. Lift function pauses (`unpause_function`) and global pause (`unpause`), verifying all required `unpause_approvals` are recorded.
3. Publish a post-mortem report detailing root cause, timeline, affected invoices, and regression tests added to the test suite.

---

## 8. Security References & Resources

### 8.1 Internal Codebase Security Modules

| Module | Path | Security Responsibility |
|--------|------|------------------------|
| Security Audit Engine | [`contracts/split/src/security_audit.rs`](../contracts/split/src/security_audit.rs) | Automated on-chain vulnerability scanning, security scoring (`0..=100`), and invoice invariant checks |
| Emergency Pause & Circuit Breaker | [`contracts/split/src/emergency_pause.rs`](../contracts/split/src/emergency_pause.rs) | Global pause, granular per-function pause, auto-trip circuit breaker, and multi-sig timelocked recovery |
| Tamper-Evident Audit Logging | [`contracts/split/src/audit_logging.rs`](../contracts/split/src/audit_logging.rs) | SHA-256 hash-chained operational and security audit log storage, querying, and integrity verification |
| Input Validation & Signatures | [`contracts/split/src/validation.rs`](../contracts/split/src/validation.rs) | Bounds checking, Ed25519 signature verification, batch validation, and string sanitization |
| Rate Limiting & Anti-Spam | [`contracts/split/src/rate_limiting.rs`](../contracts/split/src/rate_limiting.rs) | Sliding-window per-address rate limiting, global throughput caps, and dynamic violation penalties |
| Staged Contract Upgrades | [`contracts/split/src/contract_upgrade.rs`](../contracts/split/src/contract_upgrade.rs) | Multi-admin upgrade proposals, version validation, state migrations, and rollback snapshots |
| Storage Key Registry | [`contracts/split/src/storage_keys.rs`](../contracts/split/src/storage_keys.rs) & [`STORAGE_KEY_REGISTRY.md`](../STORAGE_KEY_REGISTRY.md) | Centralized storage key constructors and XDR collision prevention |
| Error Catalog & Recovery | [`contracts/split/src/error.rs`](../contracts/split/src/error.rs) & [`docs/ERROR_CODES.md`](./ERROR_CODES.md) | Standardized `ContractError` definitions and programmatic recovery suggestions |

### 8.2 External Security Standards & Documentation
- [Stellar Soroban Security Best Practices (Official Stellar Docs)](https://developers.stellar.org/docs/build/security-docs)
- [Soroban Authorization Framework (`require_auth` & `require_auth_for_args`)](https://developers.stellar.org/docs/learn/fundamentals/contract-development/authorization)
- [Soroban Storage & State Archival (TTL Management)](https://developers.stellar.org/docs/learn/fundamentals/contract-development/storage/state-archival)
- [Rust Secure Code Guidelines (ANSSI / RustSec Advisory Database)](https://rustsec.org/)
- [OWASP Smart Contract Top 10 & CWE Smart Contract Weakness Classification](https://owasp.org/www-project-smart-contract-top-10/)
