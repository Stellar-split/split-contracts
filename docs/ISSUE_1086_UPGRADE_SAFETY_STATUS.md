# Issue #1086: Contract upgrade safety (status)

**Status:** partly covered. The storage-migration guard is wired in and works. The version, manifest,
validation and history code in `contract_upgrade.rs` is declared but not called by the upgrade entry
points, so those requirements are not met in practice.

## Requirements against the code

| Requirement | Where it is | Result |
|---|---|---|
| Upgrade system | `propose_upgrade(admin, new_wasm_hash)`, `execute_upgrade()`, `cancel_upgrade(admin)`, `get_upgrade_proposal()` in `contracts/split/src/lib.rs`. A proposal has a 48 hour timelock. | Present. |
| Migration guards | `migrations::require_schema_current` is called first by the shared guards (`require_admin`, `require_not_paused` and the like). A contract left on an old schema panics with `MigrationRequired`. `migrate(admin)` (SuperAdmin) is the only exempt entry point. | Present and enforced. |
| Version checks | `contract_upgrade::ContractVersion::is_upgrade_of` and the "version is increasing" check in `contract_upgrade::validate_upgrade` | Not enforced. See below. |
| Data migration | `migrations::run_pending_migrations` runs v1 to v2 and v2 to v3 in order and stores the version after each step, so an interrupted run resumes. `compact_migrate(invoice_id)` migrates one invoice. | Present. |
| Upgrade history | `contract_upgrade::record_upgrade` and `get_upgrade_history` | Not recorded. See below. |
| Freeze during upgrade | `freeze_for_upgrade(admin, checkpoint_hash)`, `thaw_contract(admin)`, `get_upgrade_checkpoint()` | Present. SuperAdmin only. |

## What is not wired

`contract_upgrade.rs` (issue #1010) describes manifests, a validation step, a rollback WASM hash and a
history log. Its own comment says `record_upgrade` is "called immediately after
`env.deployer().update_current_contract_wasm(...)` succeeds in `lib.rs`". Nothing in `lib.rs` calls it:

- `propose_upgrade` does not require a manifest, and does not call `validate_upgrade`.
- `execute_upgrade` calls `update_current_contract_wasm` and emits `upgrade_executed`, but does not call
  `record_upgrade`, so `get_upgrade_history` and `get_previous_wasm_hash` stay empty.
- `validate_upgrade` also never blocks anything. Its doc says the caller decides whether to enforce `passed`.

`execute_upgrade` is callable by anyone once the timelock has passed. The new code does not run until
the SuperAdmin calls `migrate`, and until then every guarded entry point panics with `MigrationRequired`,
which is what keeps the window safe.

## What this change does

Records the status above. It changes no code. The behaviour was read, not run, and no tests were added.
The "upgrades safe" and "history tracked" criteria need the wiring described above before they hold.
