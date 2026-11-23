# Incident response playbook

What to do when something goes wrong with a deployed `split` contract: who acts, which entry points to
call, and how to recover. Every call below exists in `contracts/split/src/lib.rs` today.

## Roles

| Role | Can do in an incident |
|---|---|
| Admin (the `admin_key` address) | `pause`, `unpause`, `propose_upgrade`, `cancel_upgrade`, `propose_admin` |
| Operator | `pause` and `unpause` (when no single admin is stored), `pause_function`, `unpause_function`, `set_pause_exempt` |
| SuperAdmin | The Operator calls above, plus `freeze_for_upgrade`, `thaw_contract`, `migrate`, `add_admin`, `remove_admin`, `set_sweep_timeout`, `sweep_unclaimed_funds` |

Keep the keys for at least two SuperAdmins reachable at all times. `remove_admin` refuses to remove the
last SuperAdmin.

## Procedure

Every incident follows the same five steps.

1. **Detect.** An alert, a user report, or an unexpected event. Note the time, ledger, and transaction
   hashes.
2. **Contain.** Stop the damage first, investigate second. Use the smallest switch that does it:

   | Scope | Call | Effect |
   |---|---|---|
   | One entry point | `pause_function(admin, fn_name)` | That function panics with `function paused` |
   | All guarded writes | `pause(admin)` | Entry points that check the pause flag fail with `ContractPaused` |
   | Everything except admin actions, during an upgrade | `freeze_for_upgrade(admin, checkpoint_hash)` | Writes that call `require_not_frozen` fail |

3. **Assess.** Work out which invoices, tokens, and addresses are affected, and whether funds left the
   contract. Read-only calls such as `is_paused`, `get_schema_version`, `get_failed_payouts` and
   `get_upgrade_proposal` work while the contract is paused.
4. **Recover.** Follow the matching scenario below.
5. **Review.** Unpause, watch the contract for a while, and write up the incident (see the template at the
   end).

## Scenarios

### 1. Bug in a payment or release path

- **Signs:** wrong amounts paid out, a release that should have failed, an invariant broken.
- **Contain:** `pause_function(admin, "<function>")` for the affected entry point, or `pause(admin)` if the
  bug can be reached from several.
- **Recover:**
  1. Fix the bug and test the new build (`cargo test --workspace`).
  2. Upgrade as described in [`UPGRADE_MIGRATION_GUIDE.md`](./UPGRADE_MIGRATION_GUIDE.md). The 48 hour
     timelock still applies, so the contract stays paused meanwhile.
  3. After `execute_upgrade`, run `migrate(admin)` if the schema changed.
  4. `unpause_function` / `unpause`.

### 2. Compromised admin key

- **Signs:** admin calls nobody on the team made, an unexpected `propose_upgrade` or `propose_admin`.
- **Contain:** from a SuperAdmin key that is still safe, `pause(admin)`. If an upgrade was proposed, call
  `cancel_upgrade(admin)` before its `eligible_at`.
- **Recover:**
  1. `remove_admin(admin, compromised)` from a safe SuperAdmin.
  2. If the compromised address is the stored admin, move it with `propose_admin(admin, new_admin)` and
     `accept_admin()` signed by the new address.
  3. Check `set_treasury`, `add_allowed_token`, `add_trusted_caller` and fee settings for changes the
     attacker made, and revert them.
  4. `unpause(admin)`.

### 3. Failed payouts to recipients

- **Signs:** `get_failed_payouts(invoice_id)` returns addresses, usually accounts that did not exist on the
  ledger at release time.
- **Contain:** nothing to stop; the funds stay in the contract.
- **Recover:**
  1. Ask the recipient to create the account, then call `retry_failed_payout(invoice_id, recipient)`.
  2. If the recipient never appears, a SuperAdmin calls `sweep_unclaimed_funds(admin, invoice_id)` once the
     sweep timeout has passed since the last failure (default 120 960 ledgers, set with
     `set_sweep_timeout`).

### 4. Upgrade went wrong

- **Signs:** every call panics with `MigrationRequired`, reads panic on decode, or the new build misbehaves.
- **Contain:** `pause(admin)`, or `freeze_for_upgrade` if not already frozen.
- **Recover:**
  1. `MigrationRequired`: a SuperAdmin calls `migrate(admin)` and checks `get_schema_version()`.
  2. `migrate` keeps failing: a step is too large for one transaction. See "A migration must fit in one
     transaction" in the upgrade guide.
  3. The new build misbehaves: propose the previous WASM hash, as in the guide's rollback section.
  4. `thaw_contract(admin)` and `unpause(admin)` once the checks pass.

### 5. Problem with a token

- **Signs:** a token contract misbehaves, is upgraded maliciously, or is reported as compromised.
- **Contain:** `remove_allowed_token(admin, token)` so no new invoice uses it. Pause the payment entry
  points if existing invoices in that token are at risk.
- **Recover:** settle or refund the affected invoices once the token is safe again, then add it back with
  `add_allowed_token` if wanted.

## Automation

All calls go through the Stellar CLI, so a runbook can be scripted. Set the variables once:

```sh
CONTRACT=<contract id>
NETWORK=mainnet
ADMIN=<CLI identity of the admin key>
ADMIN_ADDR=$(stellar keys address "$ADMIN")

invoke() {
  stellar contract invoke --id "$CONTRACT" --network "$NETWORK" --source "$ADMIN" -- "$@"
}
```

Contain:

```sh
invoke pause --admin "$ADMIN_ADDR"
invoke pause_function --admin "$ADMIN_ADDR" --function release
invoke cancel_upgrade --admin "$ADMIN_ADDR"
```

Check state:

```sh
invoke is_paused
invoke get_schema_version
invoke get_upgrade_proposal
invoke get_failed_payouts --invoice_id 42
```

Recover:

```sh
invoke migrate --admin "$ADMIN_ADDR"
invoke retry_failed_payout --invoice_id 42 --recipient <address>
invoke thaw_contract --admin "$ADMIN_ADDR"
invoke unpause_function --admin "$ADMIN_ADDR" --function release
invoke unpause --admin "$ADMIN_ADDR"
```

Contract events (`contract_paused`, `contract_unpaused`, upgrade and admin-transfer events) can be watched
with `stellar events --id "$CONTRACT"` to alert on admin actions nobody on the team made.

## Checklist

- [ ] Incident time, ledger and transaction hashes written down
- [ ] Contained with the smallest switch that stops the damage
- [ ] Affected invoices, tokens and addresses listed
- [ ] Fix tested on a copy of the data before mainnet
- [ ] Contract unpaused / thawed and watched afterwards
- [ ] Write-up done

## Write-up template

```text
Title:
Detected at (time, ledger):
Contained at (time, call):
Resolved at:
What happened:
Impact (invoices, amounts, users):
Root cause:
Recovery steps taken:
Follow-ups:
```
