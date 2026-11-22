# Note to maintainers: Issue #1181 (upgrade path management)

The upgrade system requested here already exists on main in
`contracts/split/src/contract_upgrade.rs` (issue #1010):

- Versioning: `ContractVersion`, `get_version`, `init_version`.
- Upgrade manifest and validation: `propose_manifest`, `validate_upgrade`.
- Upgrade tracking: `record_upgrade`, `get_upgrade_history`.
- Rollback: `get_previous_version`, `get_previous_wasm_hash`,
  `assert_rollback_allowed`, rollback counter helpers.
- Migration logic: `contracts/split/src/migrations.rs`.

No new code was added for this issue, to avoid duplicating or conflicting
with that module. Remaining gaps, if the maintainers want them tracked
separately: end-to-end tests of upgrade-then-rollback preserving all
instance/persistent data ("no data loss"), and a documented runbook.
Nothing was compiled or tested in this change.
