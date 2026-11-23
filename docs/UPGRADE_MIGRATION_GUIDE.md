# Contract upgrade and migration guide

How to upgrade a deployed `split` contract without losing data, how to add a migration, and what can go
wrong. It describes the code as it is today: the two-step upgrade in `lib.rs` and the schema migrations in
`contracts/split/src/migrations.rs`.

## How an upgrade works

1. **Upload the new WASM** to the network and note its hash.
2. **Propose** it: `propose_upgrade(admin, new_wasm_hash)`. The proposal is stored with an eligibility time
   48 hours ahead. `get_upgrade_proposal()` shows it.
3. **Wait** for the timelock. `execute_upgrade()` panics with `upgrade timelock still active` until then.
   `cancel_upgrade(admin)` removes a proposal that should not go ahead.
4. **Execute** with `execute_upgrade()`. Anyone can call it after the timelock. It clears the proposal and
   swaps the contract's WASM for the new one. Storage is kept.
5. **Migrate**: a SuperAdmin calls `migrate(admin)`. Until it has run, every guarded entry point panics with
   `MigrationRequired` (54), so nobody reads or writes data in the wrong shape.
6. **Verify**: `get_schema_version()` returns the version the new build expects, and a few reads and one
   write behave as before.

Between steps 4 and 5 the contract is effectively read-only for the guarded entry points. Run `migrate` in
the same session as `execute_upgrade`.

### Freezing during an upgrade

`freeze_for_upgrade(admin, checkpoint_hash)` (SuperAdmin) blocks the write entry points that call
`require_not_frozen` and stores the checkpoint hash, which `get_upgrade_checkpoint()` returns. Use it
when the work needs a quiet contract, for example a long migration. `thaw_contract(admin)` ends it.

## Before you upgrade: safety checks

- [ ] The new build passes `cargo test --workspace`, including `cargo test -p split storage_snapshot`.
- [ ] The storage key snapshot changed only where you meant it to, see
      [`ARCHITECTURE.md`](./ARCHITECTURE.md#34-storage-key-snapshot).
- [ ] If a stored `#[contracttype]` struct changed shape, `CURRENT_SCHEMA_VERSION` was bumped and a
      migration was added. Without it, old records fail to decode.
- [ ] You know how many invoices exist (`peek_next_invoice_id`), and have estimated the cost of the
      migration. See "A migration must fit in one transaction" below.
- [ ] The new WASM hash is the one you tested. Compare the hash in the proposal with your build.
- [ ] You have a way back, see the rollback section.
- [ ] The contract is paused or frozen if the migration must not race with writes.

## Adding a migration

`migrations.rs` holds the steps. The existing ones are an example of each kind:

- `migration_v2` backfills a record for every existing invoice (`InvoiceMeta`), skipping an invoice that
  already has one.
- `migration_v3` renames a storage key: read under the old key, write under the new one, remove the old one.

To add schema version N+1:

1. Raise `CURRENT_SCHEMA_VERSION` by one.
2. Write `migration_vN+1(env)`. Make it **idempotent**: running it twice must leave the same result, so a
   repeat call after a failure is harmless. Check for the new record before writing it.
3. Add a step to `run_pending_migrations`, after the previous one:

   ```rust
   if version < N+1 {
       migration_vN+1(env);
       version = N+1;
       set_schema_version(env, version);
   }
   ```
4. Add a test that builds data in the old shape, runs `migrate`, and checks the backfilled values, as the
   `CONTRIBUTING.md` checklist asks.
5. Describe the migration in the PR, including how long it takes for a large contract.

A fresh deployment calls `initialize`, which stamps the current version, so new contracts never run a
migration.

## A migration must fit in one transaction

`run_pending_migrations` runs each step in a single call. `migration_v2` and `migration_v3` loop over every
invoice id from 1 to the counter. The comment on the function says an interrupted run resumes, but a step
that runs out of budget fails as a whole and its writes are discarded, including the version bump. It will
fail the same way on the next call. So:

- Estimate the cost of a step for the largest contract you operate before upgrading.
- For a very large contract, split the step: process a range of ids per call and store a cursor, then bump
  the version only after the last range. That needs a new entry point and is not in the code today.
- Do not rely on the resume behaviour for a long single step.

## Rollback

There is no one-call rollback in the contract. `contract_upgrade.rs` describes a rollback hash and history
log, but `propose_upgrade` and `execute_upgrade` do not use it, see
[`ISSUE_1086_UPGRADE_SAFETY_STATUS.md`](./ISSUE_1086_UPGRADE_SAFETY_STATUS.md). The way back is:

1. Keep the hash of the previous WASM before you propose the new one.
2. If the new build misbehaves, `propose_upgrade` the previous hash. It also waits 48 hours, so freeze or
   pause the contract meanwhile if the new build is unsafe.
3. A migration is not undone by installing the old WASM. If the data shape changed, the old build may not
   read the migrated records. Only upgrade over a migration when the migration is safe to leave in place, or
   write a reverse migration for it.

Because of step 3, test the upgrade on a copy of the data first, and prefer migrations that only add data
over ones that rename or delete it.

## Example: a safe upgrade, start to finish

```text
1. Build and test the new WASM; record its hash and the previous hash.
2. propose_upgrade(admin, new_hash)
3. Announce the 48 hour window; run the checks above against a testnet copy.
4. At the end of the window: pause the contract.
5. execute_upgrade()
6. migrate(admin)                      -> returns the new schema version
7. get_schema_version()                -> equals CURRENT_SCHEMA_VERSION
8. Read a few invoices, make one test payment on a throwaway invoice.
9. Unpause.
```

## What can go wrong

| Symptom | Cause | Fix |
|---|---|---|
| Every call panics with `MigrationRequired` | `migrate` has not run since the upgrade | SuperAdmin calls `migrate` |
| `migrate` fails and keeps failing | A step is too large for one transaction | Split the step, see above |
| Reads return old values or panic on decode | A stored struct changed shape without a migration | Roll back, add the migration, upgrade again |
| `upgrade timelock still active` | Called before 48 hours | Wait, or cancel and re-propose |
| `no upgrade proposal` | Nothing proposed, or it was cancelled | Propose again |
| `contract is frozen for upgrade` | `freeze_for_upgrade` was not thawed | SuperAdmin calls `thaw_contract` |
