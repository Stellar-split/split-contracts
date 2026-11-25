# Upgrade migration toolkit

Helpers and runbook for upgrading a deployed `split` contract. The upgrade flow itself is described in
[`UPGRADE_MIGRATION_GUIDE.md`](./UPGRADE_MIGRATION_GUIDE.md). This page covers the toolkit in
`contracts/split/src/upgrade_toolkit.rs`, the existing migrations, and how to script an upgrade.

## Migrations

`CURRENT_SCHEMA_VERSION` in `contracts/split/src/migrations.rs` is 3. `migrate(admin)` runs every step
between the stored version and that one, in order.

| Step | From → to | What it does | Idempotent because |
|---|---|---|---|
| `migration_v2` | 1 → 2 | Writes a default `InvoiceMeta` (`note = ""`, `priority = 0`) under `inv_meta` for every existing invoice id | Skips ids that already have a record |
| `migration_v3` | 2 → 3 | Moves each `InvoiceMeta` from `inv_meta` to `inv_note` and removes the old key | Only ids still under `inv_meta` are moved |

A contract that never wrote `schm_ver` reads as version 1. A fresh `initialize` stamps version 3, so new
deployments run no migration.

Per-invoice legacy records (no `version` field) are upgraded one at a time with
`migrate_invoice(admin, invoice_id)`, which is a no-op for an invoice that is already current.

## Utilities

All helpers are pure functions: pass in the values read from the contract views.

### `plan_migration(stored, target)`

Returns a `MigrationPlan` with the steps `migrate` will run, or an error:

- `Downgrade` if `target` is below `stored`. Migrations only go forward; see the rollback section of the
  guide.
- `UnknownTarget` if `target` is above `CURRENT_SCHEMA_VERSION` of the build you are using.

```rust
let plan = plan_migration(1, 3)?;
assert_eq!(plan.step_count(), 2);
assert_eq!(plan.step(0), Some(2)); // migration_v2 runs first
assert_eq!(plan.step(1), Some(3)); // then migration_v3
```

### `MigrationCursor`

Splits invoice ids `1..=last_id` into batches, for a migration step that is too large for one transaction.
`last_id` is the invoice counter (`peek_next_invoice_id` minus one). Store `next_id()` between calls and
rebuild the cursor with `MigrationCursor::resume`.

```rust
let mut cursor = MigrationCursor::new(last_id, DEFAULT_BATCH_SIZE)?;
while let Some((start, end)) = cursor.next_range() {
    // migrate ids start..=end in one transaction
}
```

### `next_action(&UpgradeSnapshot)`

Maps the contract state to the next upgrade call:

| State | `UpgradeAction` | Call |
|---|---|---|
| New WASM not deployed, no proposal | `Propose` | `propose_upgrade(admin, hash)` |
| Proposal pending, `now < eligible_at` | `WaitTimelock` | none, wait |
| Proposal pending, timelock elapsed | `Execute` | `execute_upgrade()` |
| New WASM deployed, `get_schema_version` behind | `Migrate` | `migrate(admin)` |
| Migrated, checkpoint still set | `Thaw` | `thaw_contract(admin)` |
| Otherwise | `Done` | run the post-upgrade checks |

## Automation

A script can drive an upgrade by reading the views, calling `next_action`, and making the matching call
until it returns `Done`. The view calls with the Stellar CLI:

```sh
stellar contract invoke --id "$CONTRACT" --network "$NETWORK" -- get_upgrade_proposal
stellar contract invoke --id "$CONTRACT" --network "$NETWORK" -- get_schema_version
stellar contract invoke --id "$CONTRACT" --network "$NETWORK" -- get_upgrade_checkpoint
```

The write calls:

```sh
stellar contract invoke --id "$CONTRACT" --network "$NETWORK" --source "$ADMIN" -- \
  propose_upgrade --admin "$ADMIN_ADDR" --new_wasm_hash "$NEW_HASH"
stellar contract invoke --id "$CONTRACT" --network "$NETWORK" --source "$ADMIN" -- execute_upgrade
stellar contract invoke --id "$CONTRACT" --network "$NETWORK" --source "$ADMIN" -- migrate --admin "$ADMIN_ADDR"
stellar contract invoke --id "$CONTRACT" --network "$NETWORK" --source "$ADMIN" -- thaw_contract --admin "$ADMIN_ADDR"
```

`upgraded` in the snapshot is known to the script: it is true once `execute_upgrade` has succeeded for the
new hash. Keep the previous WASM hash before proposing, as the guide's rollback section explains.

## Examples

### Upgrade from schema 1 with no freeze

```text
plan_migration(1, 3)              -> steps v2, v3
next_action(...)                  -> Propose      -> propose_upgrade
next_action(...)                  -> WaitTimelock (48 hours)
next_action(...)                  -> Execute      -> execute_upgrade
next_action(...)                  -> Migrate      -> migrate, returns 3
next_action(...)                  -> Done
```

### Frozen upgrade

```text
freeze_for_upgrade(admin, checkpoint)
propose_upgrade / wait / execute_upgrade
migrate(admin)                    -> schema current
next_action(...)                  -> Thaw         -> thaw_contract
next_action(...)                  -> Done
```

### Batching a new migration step

A new `migration_vN` that walks every invoice can take its id range from a cursor, with the cursor's
`next_id()` stored between calls and the schema version bumped only after the last range:

```text
MigrationCursor::new(last_id = 120, batch = 50)
  next_range() -> (1, 50)
  next_range() -> (51, 100)
  next_range() -> (101, 120)
  next_range() -> None, bump the schema version
```
