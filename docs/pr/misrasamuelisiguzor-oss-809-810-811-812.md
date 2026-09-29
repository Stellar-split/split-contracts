# Auto-release condition, recipient metrics, dependency-link event, config snapshot

Each issue gets its core piece plus tests in `contracts/split/src/test.rs`. The remaining requirements are listed under "Not done in this PR".

## #809 - configurable auto-release conditions

Already on `main`: `scheduled_release_at` + `trigger_scheduled_release` (#207) and ledger-based `release_delay_ledgers` (#327).

**Done**
- `AutoReleaseCondition` enum (`types.rs`) with `AtTimestamp(u64)`, stored per invoice under `(auto_cnd, id)`.
- `set_auto_release_condition(creator, invoice_id, condition)`: creator-only, invoice must be Pending, timestamp must be in the future. `get_auto_release_condition(invoice_id)`.
- While the condition is unmet, full funding through `pay` holds the funds (added to the existing auto-release guard) instead of releasing immediately.
- `trigger_auto_release(invoice_id)`: callable by anyone once the condition is met. It runs through `release_invoice`, so every other guard (approval, prerequisite, co-signers, ...) still applies, then clears the condition and emits `AutoReleaseTriggered` (`split`, `auto_rel`, id).
- Tests: funds held until the condition is met; trigger before the timestamp panics; trigger at the timestamp releases, pays the recipient and emits the event; trigger with no condition panics; non-creator can't set; past timestamp rejected.

**Not done in this PR**
- `DelayAfterFunding(seconds)` and `OnReputation(minScore)` variants.
- `auto_release_condition` field on `InvoiceOptions` (the struct is at the 40-field `#[contracttype]` limit; needs an `InvoiceOptions2` slot).
- The hold-on-full-funding guard is only added to the main `pay` path, matching where `scheduled_release_at` is checked today.

## #810 - recipient performance metrics

**Done**
- `RecipientMetrics { invoices_received_count }` stored under `(rcp_met, recipient)`.
- `_release` (shared by every release path) increments the count once per distinct recipient when an invoice transitions to `Released`.
- `get_recipient_metrics(recipient)` (zeroed when never paid out).
- Tests: count accumulates across two released invoices; a partly paid, unreleased invoice doesn't count.

**Not done in this PR**
- `avg_payout_time_hours` and `success_rate_bps`.

## #811 - invoice dependency linking

Already on `main`: single-predecessor dependency via `InvoiceOptions.prerequisite_id` (#22), which blocks release with "prerequisite not released"; `DependencyNotMet` / `CircularDependency` error codes exist.

**Done**
- `InvoiceDependencyLinked` event (`split`, `dep_link`, invoice_id; data = prerequisite id) emitted at creation when a prerequisite is set.
- `get_invoice_dependency(invoice_id) -> Option<u64>`.
- Test: no event and `None` without a prerequisite; event carries the prerequisite id and the getter returns it.

**Not done in this PR**
- Multiple dependencies per invoice, linking/unlinking after creation, a reverse "dependents" index, and wiring up circular-dependency detection.

## #812 - contract state snapshot export

`storage_snapshot.rs` on `main` is a storage-key encoding test, not a state export.

**Done**
- `export_config_snapshot() -> ConfigSnapshot { admin, treasury, usdc_token, paused, platform_fee_bps, invoice_count, schema_version }` (read-only).
- Test: snapshot matches values set by `initialize`, then reflects a pause and a created invoice.

**Not done in this PR**
- Per-invoice state export, import/restore, a snapshot event, and migration tooling.

## Verification

- `cargo test --workspace`: all pass (`split` 344 -> 354 tests, other crates unchanged).
- `cargo clippy -p split`: no warnings in the new code (the existing warnings are elsewhere in `lib.rs`, `calc.rs`, `stats.rs`, `storage.rs`, `validation.rs`).
- `cargo fmt --check`: no diffs in the new code (the files already had pre-existing unformatted code on `main`, left untouched).

Closes #809
Closes #810
Closes #811
Closes #812
