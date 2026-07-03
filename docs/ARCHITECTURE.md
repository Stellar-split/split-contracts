# Contract Architecture

Overview of the StellarSplit Soroban contract's internal design, storage model, and invoice lifecycle.

## Directory Layout

```
contracts/split/src/
├── lib.rs              # All public entry points (~7 500 lines)
├── types.rs            # #[contracttype] structs and enums
├── events.rs           # Event emission helpers
├── storage_keys.rs     # Storage key constructor functions
├── storage_snapshot.rs # XDR snapshot test
├── error.rs            # Error types
├── factory.rs          # Factory pattern helpers
├── test.rs             # Integration tests
└── tests.rs            # Additional test modules
tests/
└── snapshots/
    └── storage_keys.json  # Committed XDR baseline for storage key regression tests
```

---

## Storage Tiers

Soroban has three storage tiers with different rent / eviction behaviour.

| Tier | Used For | Key examples |
|------|----------|-------------|
| `instance` | Contract-level singletons that live as long as the contract | `admin_key`, `paused_key`, `usdc_token_key`, `treasury_key`, `platform_fee_bps_key`, `circuit_breaker_key` |
| `persistent` | Per-entity data that needs long retention (invoice bodies, payer records) | `invoice_key`, `invoice_ext_key`, `invoice_hot_key`, `confidential_pay_key`, `vel_key` |
| `temporary` | Short-lived ephemeral state (not currently used in production paths) | — |

Instance keys are never bumped — they inherit the contract's own TTL. Persistent keys are bumped on every read/write in hot paths.

---

## Invoice Data Model

To stay within Soroban's per-entry size limits an invoice is split across several storage keys that are always read together by `get_invoice`:

```
invoice_key(id)        → InvoiceCore    — creator, recipients, amounts, funded, status, payments, tranches, …
invoice_ext_key(id)    → InvoiceExt     — co-signers, split rules, oracle, penalty, velocity, allowed_payers, …
invoice_ext2_key(id)   → InvoiceExt2    — Wave 5/6 fields: priorities, refund_grace_secs, scheduled_release_at, …
invoice_hot_key(id)    → InvoiceHot     — { status, funded, total, recipients } — read on every pay() fast-path
invoice_compact_key(id)→ CompactInvoice — XDR-compressed overlay for read-heavy SDK queries
```

Wave 6 also added separate optimisation keys for invoices with many recipients:

```
recipients_list_key(id)       → Vec<Address>   — recipient addresses
amounts_list_key(id)          → Vec<i128>       — owed amounts
paid_flags_key(id)            → Vec<bool>       — per-recipient paid flags
milestone_flags_key(id)       → Vec<bool>       — per-milestone release flags
compact_status_key(id)        → InvoiceStatus  — cached status for fast reads
compact_deadline_ledger_key(id)→ u32            — deadline in ledgers for fast expiry checks
```

### Key Types Summary

| Key function | Parameters | Stores |
|---|---|---|
| `invoice_key` | `id: u64` | `InvoiceCore` |
| `invoice_ext_key` | `id: u64` | `InvoiceExt` |
| `invoice_ext2_key` | `id: u64` | wave-5 extended fields |
| `invoice_hot_key` | `id: u64` | `InvoiceHot` (fast-path cache) |
| `creator_stats_count_key` | `creator: &Address` | `u64` invoice count |
| `creator_stats_volume_key` | `creator: &Address` | `u64` lifetime volume |
| `creator_stats_released_key` | `creator: &Address` | `u64` lifetime released |
| `vel_key` | `(id, payer)` | `i128` rolling payer velocity |
| `confidential_pay_key` | `(id, payer)` | `ConfidentialPayment` |
| `pay_shard_key` | `(id, shard_index)` | `Payment` shard |

---

## InvoiceStatus Lifecycle

```
                     create_invoice()
                           │
                           ▼
                       ┌─────────┐
                       │ Pending │◄──────────────────┐
                       └────┬────┘                   │
                            │                     resume_invoice()
               pay() fills funded                     │
                            │                 ┌───────────────┐
                            ▼                 │    Paused     │
                  ┌──────────────────┐        └───────────────┘
                  │ Fully funded?    │──No──► deadline passed?
                  └──────┬───────────┘              │
                         │ Yes                   refund()
                         ▼                           │
                  guards cleared?                    ▼
                  (co-signers,               ┌───────────────┐
                   prerequisite,             │  Refunded     │
                   oracle, tranche,          └───────────────┘
                   scheduled_at…)
                         │ Yes
                         ▼
                     release()
                         │
                         ▼
                  ┌───────────────┐
                  │   Released    │
                  └───────────────┘
                         │
                  archive_invoice_storage()
                  (after archive_after_ledgers)
```

`Cancelled` is a terminal state reachable via admin action.

---

## Guards on Release

`_release` checks these conditions in order before transferring funds:

1. **Circuit breaker** — if active, panics with `ContractPaused`
2. **Contract pause** — if paused, only exempt addresses proceed
3. **Status** — must be `Pending`
4. **Prerequisite** — referenced invoice must be `Released`
5. **Co-signers** — must have `required_signatures` approvals
6. **Oracle condition** — `condition_met` must be `true` if `oracle_address` is set
7. **Min funding** — `funded * 10_000 / total >= min_funding_bps`
8. **Tranches** — all tranches must be unlocked by timestamp
9. **Release stages** — if `release_stages` is set, only the currently active stage amount is released
10. **Scheduled release** — `scheduled_release_at` must be in the past
11. **Release delay** — `release_delay_ledgers` post-full-funding ledger lock must have elapsed

---

## Fee Flow

```
payer sends amount
       │
       ├─► platform_fee = amount * platform_fee_bps / 10_000
       │         (waived if creator on fee_waiver list)
       │         (reduced if creator qualifies for a fee tier)
       ├─► tax = amount * tax_bps / 10_000  →  tax_authority
       ├─► insurance = amount * insurance_premium_bps / 10_000  →  insurance_fund
       └─► net to invoice funded balance
```

At release:
```
funded balance
       │
       ├─► swap via DEX (if swap_tokens configured per recipient)
       ├─► split by split_rules[] or amounts[]
       └─► transfer to each recipient
```

---

## Event Model

All events use a three-part topic: `(namespace, event_name, entity_id)`.

- Namespace is always `symbol_short!("split")`
- Entity ID is the `invoice_id` for invoice events, or omitted for contract-level events

See [`docs/EVENTS.md`](./EVENTS.md) for the complete event reference.

---

## Storage Key Snapshot

Every storage key is XDR-serialised and compared against a committed baseline in `tests/snapshots/storage_keys.json` by the `storage_key_snapshot` test. This prevents accidental key renames or format changes from silently breaking on-chain data compatibility.

Adding, renaming, or removing any key **intentionally fails the test** — update the baseline and include a migration note in the PR.

```bash
# Run the snapshot test
cargo test -p split storage_snapshot

# Update the baseline after an intentional key change
cargo test -p split storage_snapshot 2>&1 \
  | grep -A 9999 "EXPECTED (generated)" \
  | tail -n +2 | head -n -1 \
  > tests/snapshots/storage_keys.json
```
