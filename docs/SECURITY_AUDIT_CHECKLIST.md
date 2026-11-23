# Contract security audit checklist

A checklist for reviewing the contracts in this workspace before a release or after a
significant change. Each item has an ID, a severity, and a verification step. It builds on
[`SECURITY_BEST_PRACTICES.md`](SECURITY_BEST_PRACTICES.md), which explains the reasoning behind
most of these items.

## How to use this checklist

1. Copy the [template](#audit-record-template) into the PR, issue or audit report.
2. Set the scope: which workspace members (see the root `Cargo.toml`) and which commit.
3. Work through each section. For every item, run the verification step and record
   `Pass`, `Fail` or `N/A` with a short note (file and line, command output, test name).
4. Any `Fail` on a `Critical` or `High` item blocks the release until it is fixed or
   explicitly accepted by a maintainer, with the reason written down.
5. Attach the filled-in record to the release notes or the PR.

Severities follow `AuditSeverity` in `contracts/split/src/security_audit.rs`:
`Critical`, `High`, `Medium`, `Low`, `Info`.

## 1. Authorization

| ID | Severity | Item | Verification |
|----|----------|------|--------------|
| AUTH-1 | Critical | Every entry point that moves funds or changes privileged state calls `require_auth()` on the acting address before any storage write or token transfer. | For each `pub fn` in the `#[contractimpl]` block that writes storage or calls `token::Client::transfer`, confirm a `require_auth()` (or `require_admin`) call comes first. `grep -n "pub fn" contracts/<name>/src/lib.rs` lists the entry points. |
| AUTH-2 | Critical | Address parameters that claim a role (admin, arbiter, creator) are compared with stored state, not trusted as passed. | Check the role check reads the stored address (e.g. `arbiter_key()` in `invoice-escrow`) and compares it to the parameter before `require_auth()`. |
| AUTH-3 | High | Admin-only functions share one helper rather than repeating the check. | Confirm admin functions call `require_admin` (both `split` and `invoice-escrow` have one). |
| AUTH-4 | High | Privileged singleton roles use a two-step handover or are admin-replaceable. | Review `transfer_admin` / `accept_admin` / `cancel_transfer` in `invoice-escrow`; apply the same check to any new privileged address. |
| AUTH-5 | Medium | Negative tests exist: a non-authorized caller is rejected. | Find a test per privileged entry point that expects a panic or `Err` for the wrong caller. |

## 2. Input validation

| ID | Severity | Item | Verification |
|----|----------|------|--------------|
| VAL-1 | High | Amounts are checked to be positive where zero or negative values make no sense. | Trace each amount parameter to an explicit `> 0` check or a validator in `contracts/split/src/validation.rs`. |
| VAL-2 | High | Basis-point values are bounded to `10_000`. | Confirm bps inputs go through `assert_valid_bps` (split) or an equivalent check. |
| VAL-3 | Medium | Caller-supplied lists (recipients, payers, rules) have an upper bound. | Look for a `MAX_*` constant checked before the list is stored or iterated. |
| VAL-4 | Medium | Duplicate entries are rejected where they would double-count (e.g. recipients). | Check for a duplicate check, or the `inv_dupe` finding in `run_invoice_audit`. |
| VAL-5 | Medium | All preconditions are checked before the first mutation. | In each fund-moving function, confirm every `Err`/`panic!` return sits above the first `storage().set` or `transfer`. |

## 3. Arithmetic

| ID | Severity | Item | Verification |
|----|----------|------|--------------|
| ARI-1 | Critical | Balance, counter and fee math uses `checked_*` or saturating operations. | `grep -nE "[a-z_]+ (\+|-|\*) [a-z_]+" contracts/<name>/src/lib.rs` and review each hit that touches an amount. |
| ARI-2 | High | `overflow-checks = true` is still set in the workspace release profile. | Check `[profile.release]` in the root `Cargo.toml`. |
| ARI-3 | High | Split and fee rounding never pays out more than was funded. | Review `contracts/split/src/calc.rs` and run the `fuzz_split_arithmetic` and `fuzz_fee_deduct` targets. |
| ARI-4 | Medium | Division by a caller-controlled value is guarded against zero. | Find each `/` and `checked_div` on an input-derived divisor and confirm a zero check. |

## 4. State and fund flow

| ID | Severity | Item | Verification |
|----|----------|------|--------------|
| ST-1 | Critical | Status is updated and saved before tokens are transferred (checks-effects-interactions). | In release, refund and dispute paths, confirm `save_invoice` (or equivalent) precedes `transfer`. |
| ST-2 | Critical | Funds cannot be released or refunded twice. | Confirm terminal statuses cannot be left (see the transition rules in `contracts/split/src/lib.rs`) and that a double-refund test exists (e.g. `double_refund_is_rejected`). |
| ST-3 | High | Disputed invoices block pay, release, refund and cancel. | Check each of those entry points for a disputed-status guard. |
| ST-4 | High | The sum of payouts equals the funded amount minus fees. | Trace the release path and confirm a test asserts token balances after release. |
| ST-5 | Medium | Time windows use ledger sequence where validator time drift matters. | Review new deadlines or windows and compare with `APPEAL_WINDOW_LEDGERS` in `invoice-escrow`. |

## 5. Storage

| ID | Severity | Item | Verification |
|----|----------|------|--------------|
| STO-1 | High | Storage keys are unique and do not collide across modules. | Cross-check new keys against [`STORAGE_KEY_CONVENTIONS.md`](STORAGE_KEY_CONVENTIONS.md) and `STORAGE_KEY_REGISTRY.md`. |
| STO-2 | Medium | Persistent entries that must outlive the default TTL are extended. | Check `extend_ttl` calls on long-lived records. |
| STO-3 | Medium | Per-invoice data growth is bounded. | Confirm the storage quota (`DEFAULT_INVOICE_STORAGE_QUOTA` in split) or a list cap covers new data. |
| STO-4 | Low | Instance storage holds only small, contract-wide values. | Review any new `storage().instance().set` call. |

## 6. Emergency controls and upgrades

| ID | Severity | Item | Verification |
|----|----------|------|--------------|
| EMR-1 | High | New fund-moving entry points respect the global pause. | Confirm `require_not_paused` (split) is called at the top of each new entry point. |
| EMR-2 | High | Pause, unpause and upgrade are admin-only. | Review `pause`, `unpause`, `pause_function`, `unpause_function` and `contracts/split/src/contract_upgrade.rs`. |
| EMR-3 | Medium | Storage layout changes ship with a migration. | Check `contracts/split/src/migrations.rs` and [`UPGRADE_MIGRATION_GUIDE.md`](UPGRADE_MIGRATION_GUIDE.md). |

## 7. Events and observability

| ID | Severity | Item | Verification |
|----|----------|------|--------------|
| EVT-1 | Medium | Every state change emits an event. | For each new mutation, find the matching `events::` or `emit_*` call. |
| EVT-2 | Low | Events do not leak data meant to stay private (e.g. encrypted metadata in plain form). | Review event payloads in `contracts/split/src/events.rs` for new fields. |

## 8. Error handling

| ID | Severity | Item | Verification |
|----|----------|------|--------------|
| ERR-1 | Medium | No `.unwrap()` / `.expect()` on values derived directly from caller input. | `grep -n "unwrap()\|expect(" contracts/<name>/src/lib.rs` and check each new hit. |
| ERR-2 | Low | New error variants are documented. | Confirm [`ERROR_CODES.md`](ERROR_CODES.md) / [`ERROR_CODES_SPLIT.md`](ERROR_CODES_SPLIT.md) list them. |

## 9. Testing

| ID | Severity | Item | Verification |
|----|----------|------|--------------|
| TST-1 | High | The workspace test suite passes. | `cargo test --workspace` |
| TST-2 | Medium | Fuzz targets run without new crashes. | `cargo fuzz run <target>` for the targets in `fuzz/fuzz_targets/` (e.g. `fuzz_pay`, `fuzz_refund`, `fuzz_release`). |
| TST-3 | Medium | Changed code paths have a regression test. | Check the PR adds or updates tests next to the changed contract. |

## 10. On-chain audit run (split contract)

| ID | Severity | Item | Verification |
|----|----------|------|--------------|
| OCA-1 | High | The contract-level audit reports no failed `Critical` or `High` checks. | A registered auditor (`add_auditor`) calls `run_contract_audit` and reviews the stored `SecurityAuditReport`. |
| OCA-2 | Medium | Sampled invoices pass the invoice-level audit. | Call `run_invoice_audit` on a sample of live invoices and review the findings. |

## Customizing the checklist

- **Scope by contract.** Mark items that do not apply to a contract as `N/A` with a reason
  (for example, OCA items only apply to `contracts/split`).
- **Add project items.** Append rows to the relevant section using the next free ID
  (e.g. `ST-6`). Keep the same columns so records stay comparable between audits.
- **Adjust severity.** A team may raise an item's severity for a release; lowering one
  should be recorded in the audit record with the reason.
- **Change the blocking rule.** The default blocks on failed `Critical` and `High` items.
  Record any different rule at the top of the audit record.

## Audit record template

```text
Audit: <release or PR>
Commit: <sha>
Scope: <contracts>
Auditor: <name>
Date: <yyyy-mm-dd>
Blocking rule: Critical + High (default)

ID      | Result | Notes
--------|--------|------------------------------
AUTH-1  |        |
AUTH-2  |        |
...     |        |

Accepted risks:
- <ID>: <reason, approver>
```
