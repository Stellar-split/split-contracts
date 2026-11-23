# Issue #1103: Breadcrumb trail logging (status)

**Status:** the per-invoice audit log already works as a breadcrumb trail for successful operations.
A trail for failed transactions cannot be kept on chain, and the debug logging design is not built.

## Requirements against the code

| Requirement | Where it is |
|---|---|
| Breadcrumb system | `append_audit_entry(env, invoice_id, action, actor)` in `contracts/split/src/lib.rs` appends an `AuditEntry { action, actor, timestamp }`. It is called from about 90 places, covering creation, payments, pausing, releases, disputes, archival and restore. |
| Logging | The entry is written in the same transaction as the operation. For example a payment adds `pay`, a release adds `ms_rel` and a failed payout adds `pay_fail`. |
| Breadcrumb queries | `get_audit_log(invoice_id)` returns the whole trail in order. The bounded `get_security_events()` returns reporter, kind, severity, invoice id and timestamp, and a registered auditor adds to it with `log_security_event`. |
| Filtering | Not present for the audit log. A client filters the returned list by `action` or `actor`. |
| Stats | `get_usage_totals` and `get_usage_report` count creations, payments, releases and refunds. There are no counts per breadcrumb action. |

## Notes

- Operations that fail leave no breadcrumb, because the whole transaction reverts. For failed calls use
  the transaction result and the simulation diagnostics.
- Debug-mode logging is a design only, see [`ISSUE_960_DEBUG_LOGGING.md`](./ISSUE_960_DEBUG_LOGGING.md).
- The log grows with every action on an invoice and is not bounded, see
  [`ISSUE_1087_AUDIT_LOGGING_STATUS.md`](./ISSUE_1087_AUDIT_LOGGING_STATUS.md).

## What this change does

Records the status above. It changes no code. The behaviour was read, not run, and no tests were added.
