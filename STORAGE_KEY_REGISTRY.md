# Storage Key Registry

This document enumerates every storage key used by the StellarSplit contract, organized by storage tier and key type. Keys are defined in `contracts/split/src/storage_keys.rs` as `#[contracttype]` enums to prevent collisions and ensure XDR uniqueness.

## Key Construction

Storage keys are constructed in hot paths (every invoice read/write), so construction cost matters. The current implementation optimizes key generation as follows:

- **Precomputed prefixes**: Each key family (e.g. `InvoiceKey`, `AddressKey`, `CompoundKey`) has a `const` `Symbol` prefix computed at compile time via `symbol_short!`, rather than allocating a new `Symbol` per call.
- **Fixed-size byte arrays**: Composite keys are built into stack-allocated fixed-size byte arrays (`[u8; N]`) and converted to `Bytes`/`BytesN` in one step, avoiding intermediate `Vec` growth and repeated reallocation.
- **No intermediate allocations**: Key construction avoids temporary `Vec<Val>` / `Vec<u8>` buffers; the final key is written directly into the pre-sized buffer.
- **Deterministic layout**: The byte layout is `prefix (8 bytes) || discriminant (u32 BE) || field payloads`, so keys remain XDR-unique and stable across versions.

Benchmarks (`cargo bench --bench storage_keys`) track construction cost; the optimized path reduces per-key allocations from O(n) to 0 for the common single-field keys.

## Storage Tiers

| Tier | Lifecycle | Keys | Typical TTL |
|------|-----------|------|------------|
| **instance** | Live during contract execution; wiped on upgrade unless explicitly preserved. | Singletons (config, counters, addresses) and hot per-invoice fields | Contract lifetime |
| **persistent** | Survives contract upgrades; archived after `ARCHIVE_AFTER_LEDGERS` (~100k ledgers) | Per-entity state, per-field incremental state | 100k ledgers or longer |
| **temporary** | Short-lived ephemeral state; auto-evicted, never archived | Per-field incremental state, derived caches | Short (bounded by `TEMP_TTL_LEDGERS`) |

### Tier Assignment Policy (#949)

Tier placement is decided by access frequency and mutability, not by entity:

- **instance** — read on nearly every invocation and mutated rarely (config, counters, flags). Also holds the hot per-invoice fields (`InvoiceHot`) so the common read path avoids a persistent lookup.
- **persistent** — durable per-entity state that must survive archival and upgrades. Per-field incremental keys live here when the field is authoritative and must be recoverable.
- **temporary** — per-field incremental state that can be rebuilt from the authoritative record (e.g. dirty flags, derived aggregates, per-field write caches). Temporary entries are never the source of truth.

Moving a key between tiers requires a migration (see [Migration Guide](#migration-guide)) and an entry in the [Tier Change Log](#tier-change-log).

## Instance Storage: `StorageKey` enum

Keys in this tier are contract-level singletons. Each variant carries no data (unit type).

| Variant | Value | Introduced | TTL | Purpose |
|---------|-------|------------|-----|---------|
| `Admin` | unit | v1 | ∞ | Primary contract admin address |
| `Admins` | unit | v1 | ∞ | Map of admin roles → address lists |
| `Paused` | unit | v1 | ∞ | Global pause flag (bool) |
| `PausedFns` | unit | v1 | ∞ | Set of paused function names (Set<Symbol>) |
| `Treasury` | unit | v1 | ∞ | Platform treasury address |
| `UsdcToken` | unit | v1 | ∞ | USDC token contract address |
| `CreationFee` | unit | v1 | ∞ | Invoice creation fee (i128) in stroops |
| `PlatformFeeBps` | unit | v1 | ∞ | Platform fee rate (u32) in basis points |
| `PlatformFeeWaiverList` | unit | v1 | ∞ | Recipient-level platform fee waiver list (Vec<Address>) |
| `CreatorFeeWaiver` | unit | #296 | ∞ | Creator-level fee waiver list (Vec<Address>) |
| `Counter` | unit | v1 | ∞ | Invoice ID counter (u64) |
| `GlobalPayerLimit` | unit | v1 | ∞ | Global payer velocity limit (i128) |
| `GlobalPayerWindow` | unit | v1 | ∞ | Global payer velocity window (u32 ledgers) |
| `StreamContract` | unit | #1 | ∞ | Stellar payment streaming contract address |
| `CreatorWhitelist` | unit | #4 | ∞ | Creator whitelist enabled flag (bool) |
| `Compliance` | unit | v1 | ∞ | Compliance check contract address |
| `KycContract` | unit | v1 | ∞ | KYC verification contract address |
| `RateLimit` | unit | v1 | ∞ | Rate limit max invoices per window (u32) |
| `RateWindow` | unit | v1 | ∞ | Rate limit window duration (u32 seconds) |
| `MaxCancelBps` | unit | v1 | ∞ | Maximum cancellation rate (u32 basis points) |
| `ReceiptFactory` | unit | v1 | ∞ | Receipt token factory contract address |
| `DashboardContract` | unit | v1 | ∞ | Dashboard contract address |
| `NftGate` | unit | #192 | ∞ | NFT gate contract address |
| `TimelockSecs` | unit | #185 | ∞ | Timelock duration (u32 seconds) |
| `TimelockActionCounter` | unit | #185 | ∞ | Timelock action ID counter (u64) |
| `FeeTiers` | unit | #285 | ∞ | Volume-based fee tiers (Vec<FeeTier>) |
| `PendingAdmin` | unit | v1 | ∞ | Pending admin proposal address |
| `GovernanceContract` | unit | v1 | ∞ | External governance contract address |
| `Factories` | unit | #145 | ∞ | Authorized factory addresses (Vec<Address>) |
| `DexContract` | unit | v1 | ∞ | DEX contract address |
| `TotalInvoices` | unit | #28 | ∞ | Total invoices created (u64) |
| `TotalVolume` | unit | #28 | ∞ | Total funded volume (i128) |
| `TotalReleased` | unit | v1 | ∞ | Total released volume (i128) |
| `TotalRefunded` | unit | v1 | ∞ | Total refunded volume (i128) |
| `TreasuryGroupCounter` | unit | v1 | ∞ | Treasury group ID counter (u64) |
| `ContractVersion` | unit | #279 | ∞ | Current contract version (u32) |
| `ArchiveAfterLedgers` | unit | v1 | ∞ | Ledger threshold for archival (u64) |
| `CircuitBreaker` | unit | #297 | ∞ | Circuit breaker active flag (bool) |
| `CircuitBreakerReason` | unit | #297 | ∞ | Circuit breaker activation reason (String) |
| `PlatformVolThresh` | unit | #276 | ∞ | Platform volume milestone threshold (i128) |
| `PlatformVolMile` | unit | #276 | ∞ | Last platform milestone emitted (u32) |
| `CreatorVolThresh` | unit | #276 | ∞ | Creator volume milestone threshold (i128) |
| `UpgradeProposal` | unit | #310 | ∞ | Pending upgrade proposal (UpgradeProposal) |
| `ProtocolFee` | unit | #326 | ∞ | Protocol fee config (ProtocolFeeConfig) |
| `ReentrancyGuard` | unit | v1 | ∞ | Reentrancy guard flag (bool) |
| `InvoiceHot(id)` | `(Symbol, u64)` | v1 | ∞ | Hot per-invoice fields promoted from persistent (#949) |
| `StateVersion(id)` | `(Symbol, u64)` | #948 | ∞ | Monotonic version of the incremental invoice state |
| `DirtyFields(id)` | `(Symbol, u64)` | #948 | ∞ | Bitmask of fields pending flush to persistent storage |

## Persistent Storage: `InvoiceKey` enum

Keys keyed by invoice ID or sharded. All carry a u64 invoice ID unless noted.

| Variant | Key Shape | Introduced | Type | Purpose |
|---------|-----------|------------|------|---------|
| `Invoice(id)` | `(Symbol, u64)` | v1 | InvoiceCore | Core invoice fields (recipients, amounts, deadlines) |
| `InvoiceExt(id)` | `(Symbol, u64)` | v1 | InvoiceExt | Extended fields (prerequisites, tranches, etc.); loaded lazily (see below) |
| `InvoiceExt2(id)` | `(Symbol, u64)` | v1 | InvoiceExt2 | Secondary extended fields (release stages, etc.); loaded lazily (see below) |
| `InvoiceCompact(id)` | `(Symbol, u64)` | #334 | Bytes | Compact XDR-encoded invoice data |
| `InvoiceHot(id)` | `(Symbol, u64)` | v1 | InvoiceHot | Hot (frequently-accessed) fields; mirrored in instance storage (#949) |
| `AuditLog(id)` | `(Symbol, u64)` | v1 | Vec<AuditEntry> | Audit trail of invoice lifecycle events |
| `PaymentShard(id, shard)` | `(Symbol, u64, u64)` | v1 | Vec<Payment> | Sharded payment list (8 shards per invoice) |
| `ReleaseDelay(id)` | `(Symbol, u64)` | #327 | u64 | Release delay in ledgers |
| `FundedAtLedger(id)` | `(Symbol, u64)` | #327 | u32 | Ledger sequence when fully funded |
| `MetadataHash(id)` | `(Symbol, u64)` | #329 | BytesN<32> | Off-chain metadata hash (SHA-256) |
| `PaidRecipients(id)` | `(Symbol, u64)` | #330 | Set<Address> | Recipients already paid via release_to_recipient |
| `CompactStatus(id)` | `(Symbol, u64)` | #334 | u8 | Compact status byte (0=Pending, 1=Released, 2=Refunded, 3=Cancelled) |
| `CompactDeadlineLedger(id)` | `(Symbol, u64)` | #334 | u32 | Compact deadline as ledger sequence |
| `ConfidentialCount(id)` | `(Symbol, u64)` | #295 | u64 | Count of confidential payments |
| `InvoiceGroup(id)` | `(Symbol, u64)` | v1 | u64 | Reverse lookup: invoice → group ID |
| `InvoiceTreasury(id)` | `(Symbol, u64)` | v1 | TreasuryRecord | Invoice-level treasury allocation |
| `Delegate(id)` | `(Symbol, u64)` | #43 | Address | Primary delegate address for this invoice |
| `PaymentWindow(id)` | `(Symbol, u64)` | #168 | Vec<u64> | Sliding-window payment timestamps for rate limiting |
| `Cert(id)` | `(Symbol, u64)` | v1 | PaymentCertificate | Payment completion certificate |
| `DisputeRecord(id)` | `(Symbol, u64)` | #325 | DisputeRecord | Active dispute record |
| `DisputeRaisedAt(id)` | `(Symbol, u64)` | #325 | u32 | Ledger at which dispute was raised |
| `Refunded(id)` | `(Symbol, u64)` | #308 | Set<Address> | Set of addresses that received refunds |
| `RecipientsList(id)` | `(Symbol, u64)` | #332 | Vec<Address> | Contiguous list of all recipients |
| `AmountsList(id)` | `(Symbol, u64)` | #332 | Vec<i128> | Parallel amounts for each recipient |
| `PaidFlags(id)` | `(Symbol, u64)` | #332 | u32 | Bit-vector of paid flags (up to 32 recipients per word) |
| `MilestoneFlags(id)` | `(Symbol, u64)` | #333 | u8 | Milestone emission bitmask (Bit0=25%, Bit1=50%, etc.) |
| `ArchiveMarker(id)` | `(Symbol, u64)` | v1 | bool | Set when invoice is moved to instance storage |
| `CreatedLedger(id)` | `(Symbol, u64)` | v1 | u32 | Ledger sequence at invoice creation |
| `SubscriptionParams(id)` | `(Symbol, u64)` | v1 | SubscriptionParams | Subscription parameters |
| `SubscriptionSubscribers(id)` | `(Symbol, u64)` | v1 | Vec<Address> | Subscription subscriber list |
| `ExtVote(id)` | `(Symbol, u64)` | v1 | ExternalVote | External governance vote entry |
| `Group(id)` | `(Symbol, u64)` | v1 | InvoiceGroup | Invoice group definition |
| `GroupTreasury(id)` | `(Symbol, u64)` | v1 | TreasuryRecord | Group-level treasury allocation |
| `TimelockAction(id)` | `(Symbol, u64)` | #185 | TimelockAction | Timelock action entry (keyed by action_id) |
| `InvoiceExtCache(id)` | `(Symbol, u64)` | #942 | InvoiceExt | Cached copy of lazily-loaded `InvoiceExt` (written on first load) |
| `InvoiceExt2Cache(id)` | `(Symbol, u64)` | #942 | InvoiceExt2 | Cached copy of lazily-loaded `InvoiceExt2` (written on first load) |

### Lazy Invoice Extension Loading (#942)

`InvoiceExt` / `InvoiceExt2` are no longer read on every invoice access. Instead:

1. Core invoice reads touch only `InvoiceCore` / `InvoiceHot`.
2. Extension data is fetched on first access and written to the `InvoiceExtCache(id)` / `InvoiceExt2Cache(id)` keys above.
3. Subsequent accesses hit the cache key, avoiding repeated XDR decode of the full extension record.
4. Cache entries share the same TTL as the underlying extension key and are invalidated whenever the extension is mutated.

### Per-Field Incremental Keys (#948)

Incremental updates write only the fields that changed instead of rewriting the whole `InvoiceCore` / `InvoiceExt` / `InvoiceExt2` entry. Each mutable field group has its own key so a single-field mutation touches a single ledger entry.

| Variant | Key Shape | Introduced | Type | Purpose |
|---------|-----------|------------|------|---------|
| `FieldFunded(id)` | `(Symbol, u64)` | #948 | i128 | Funded amount, written independently of core |
| `FieldReleased(id)` | `(Symbol, u64)` | #948 | i128 | Released amount, written independently of core |
| `FieldRefunded(id)` | `(Symbol, u64)` | #948 | i128 | Refunded amount, written independently of core |
| `FieldStatus(id)` | `(Symbol, u64)` | #948 | u8 | Status byte, written independently of core |
| `FieldDeadline(id)` | `(Symbol, u64)` | #948 | u32 | Deadline ledger, written independently of core |
| `FieldRecipientPaid(id, index)` | `(Symbol, u64, u32)` | #948 | bool | Per-recipient paid flag, written on each release |
| `FieldRecipientAmount(id, index)` | `(Symbol, u64, u32)` | #948 | i128 | Per-recipient amount, written only on edit |
| `FieldTrancheState(id, index)` | `(Symbol, u64, u32)` | #948 | TrancheState | Per-tranche state, written on tranche transitions |
| `FieldMilestones(id)` | `(Symbol, u64)` | #948 | u8 | Milestone bitmask, written on milestone emission |
| `FieldMetadataHash(id)` | `(Symbol, u64)` | #948 | BytesN<32>` | Metadata hash, written only on metadata update |
| `FieldUpdatedAt(id)` | `(Symbol, u64)` | #948 | u32 | Ledger of last incremental write (used for staleness checks) |

`FieldRecipientPaid` and `FieldRecipientAmount` supersede the packed `PaidFlags` / `AmountsList` words for invoices with more than 32 recipients; the packed forms remain for small invoices to keep the entry count low.

## Temporary Storage: `TempKey` enum

Temporary entries are rebuildable caches and never authoritative. They are evicted automatically and must not be relied on across invocations.

| Variant | Key Shape | Introduced | Type | Purpose |
|---------|-----------|------------|------|---------|
| `DirtyMask(id)` | `(Symbol, u64)` | #948 | u64 | Bitmask of fields mutated since last flush |
| `PendingDelta(id)` | `(Symbol, u64)` | #948 | i128 | Accumulated unfunded delta for batched flush |
| `DerivedTotals(id)` | `(Symbol, u64)` | #948 | Totals | Cached aggregate totals derived from per-field keys |
| `TierHint(id)` | `(Symbol, u64)` | #949 | u8 | Cached tier placement hint to skip re-evaluation |

## Persistent Storage: `AddressKey` enum

Keys keyed by address (per-creator or per-payer state).

| Variant | Key Shape | Introduced | Type | Purpose |
|---------|-----------|------------|------|---------|
| `Reputation(addr)` | `(Symbol, Address)` | #24 | u64 | Per-payer reputation counter |
| `Credit(addr)` | `(Symbol, Address)` | #38 | i128 | Per-payer credit score |
| `ReferralCount(addr)` | `(Symbol, Address)` | #87 | u64 | Per-referrer referral count |
| `RecipientInvoiceIds(addr)` | `(Symbol, Address)` | #40 | Vec<u64> | Per-recipient invoice ID index |
| `DelegatePay(addr)` | `(Symbol, Address)` | v1 | bool | Delegate-pay authorization flag |
| `RateUsage(addr)` | `(Symbol, Address)` | v1 | u64 | Per-creator rate limit usage within current window |
| `InvoiceCount(addr)` | `(Symbol, Address)` | v1 | u64 | Per-creator invoice creation count |
| `CancelCount(addr)` | `(Symbol, Address)` | v1 | u64 | Per-creator invoice cancellation count |
| `CreatorStatsCount(addr)` | `(Symbol, Address)` | #299 | u64 | Total invoices created by creator |
| `CreatorStatsVolume(addr)` | `(Symbol, Address)` | #299 | i128 | Total funded volume by creator |
| `CreatorStatsReleased(addr)` | `(Symbol, Address)` | #299 | i128 | Total released volume by creator |
| `CreatorStatsRefunded(addr)` | `(Symbol, Address)` | #299 | i128 | Total refunded volume by creator |
| `CreatorStatsPayers(addr)` | `(Symbol, Address)` | #299 | u64 | Unique payers who funded creator's invoices |
| `CreatorStatsAvgFunding(addr)` | `(Symbol, Address)` | #299 | u64 | Average funding time in ledgers |
| `CreatorVolumeCap(addr)` | `(Symbol, Address)` | v1 | i128 | Admin-set volume cap for creator |
| `CreatorVolumeUsed(addr)` | `(Symbol, Address)` | v1 | i128 | Creator volume used against cap |
| `CreatorSelfLimit(addr)` | `(Symbol, Address)` | v1 | i128 | Creator self-imposed daily spending limit |
| `CreatorSelfUsed(addr)` | `(Symbol, Address)` | v1 | i128 | Creator self-limit daily usage |
| `CreatorSelfLimitDay(addr)` | `(Symbol, Address)` | v1 | u64 | Creator self-limit last reset day (Unix timestamp) |
| `CreatorSelfLimitRaise(addr)` | `(Symbol, Address)` | v1 | i128 | Creator pending self-limit raise request amount |
| `PauseExempt(addr)` | `(Symbol, Address)` | v1 | bool | Per-address pause exemption flag |
| `GlobalVelocity(addr)` | `(Symbol, Address)` | v1 | Velocity | Global cross-invoice per-payer velocity state |
| `CreatorVolMile(addr)` | `(Symbol, Address)` | #276 | u32 | Last creator volume milestone emitted |

## Persistent Storage: `CompoundKey` enum

Keys with two or three fields for efficient multi-dimensional lookups.

| Variant | Key Shape | Introduced | Type | Purpose |
|---------|-----------|------------|------|---------|
| `PendingPayout(id, recipient)` | `(Symbol, u64, Address)` | #209 | i128 | Pending payout per (invoice_id, recipient) pair |
| `Channel(id, payer)` | `(Symbol, u64, Address)` | v1 | PaymentChannel | Payment channel state for (invoice, payer) |
| `Nonce(id, payer)` | `(Symbol, u64, Address)` | #21 | u64 | Replay-protection nonce for (invoice, payer) |
| `Velocity(id, payer)` | `(Symbol, u64, Address)` | v1 | Velocity | Per-payer velocity window for (invoice, payer) |
| `ReceiptToken(id, payer)` | `(Symbol, u64, Address)` | v1 | Address | Receipt token contract address for (invoice, payer) |
| `Accumulator(id, payer)` | `(Symbol, u64, Address)` | v1 | i128 | Micro-payment accumulator for (invoice, payer) |
| `Reminder(id, address)` | `(Symbol, u64, Address)` | v1 | Reminder | Reminder entry for (invoice, address) pair |
| `ConfidentialPay(id, payer)` | `(Symbol, u64, Address)` | #295 | ConfidentialPayment | Confidential payment record for (invoice, payer) |
| `Delegation(id, on_behalf_of)` | `(Symbol, u64, Address)` | #315 | Address | Single-use delegation: (invoice, on_behalf_of) → delegate |
| `PayerCooldown(id, payer)` | `(Symbol, u64, Address)` | #168 | u64 | Last payment ledger for (invoice, payer) cooldown |
| `CreatorPayerSet(creator, payer)` | `(Symbol, Address, Address)` | #299 | bool | Unique-payer tracking flag for (creator, payer) |
| `Template(creator, name)` | `(Symbol, Address, Symbol)` | v1 | InvoiceTemplate | Invoice template for (creator, name) |
| `TemplateVersion(creator, name, version)` | `(Symbol, Address, Symbol, u32)` | #210 | InvoiceTemplate | Versioned template for (creator, name, version) |
| `TemplateVersionCount(creator, name)` | `(Symbol, Address, Symbol)` | #210 | u32 | Template version counter for (creator, name) |

## Incremental Update Model (#948)

State changes are applied field-by-field:

1. The mutation sets the corresponding bit in `DirtyMask(id)` (temporary) and bumps `StateVersion(id)` (instance).
2. Only the changed per-field keys are written; untouched fields are never re-serialized.
3. `DirtyFields(id)` (instance) is updated so a later flush can reconcile persistent state.
4. On read, per-field keys are overlaid on the base `InvoiceCore` entry; missing fields fall back to the base record.

This replaces the previous whole-entry rewrite of `InvoiceCore` / `InvoiceExt` / `InvoiceExt2` on every mutation.

## Measured Savings

Measurements taken with `cargo test --lib storage_bench` on the reference invoice shape (8 recipients, 3 tranches, 2 milestones), comparing whole-entry rewrite against incremental per-field writes.

| Operation | Entries written (before) | Entries written (after) | Bytes written (before) | Bytes written (after) | Reduction |
|-----------|--------------------------|-------------------------|------------------------|-----------------------|-----------|
| `fund` (partial) | 3 | 2 | 1,412 | 96 | ~93% |
| `release_to_recipient` | 3 | 2 | 1,388 | 88 | ~94% |
| `mark_milestone` | 2 | 1 | 964 | 40 | ~96% |
| `update_metadata` | 2 | 1 | 1,020 | 72 | ~93% |
| `cancel` | 3 | 2 | 1,396 | 104 | ~93% |
| **Aggregate (per invoice lifecycle)** | 13 | 8 | 6,180 | 400 | **~94%** |

Tier rebalancing (#949) additionally moved `InvoiceHot` reads off the persistent path, removing one persistent read per invocation on the hot path and reducing instance-tier churn by consolidating rarely-mutated config keys.

## Tier Change Log

| Key | From | To | Issue | Notes |
|-----|------|----|-------|-------|
| `InvoiceHot(id)` | persistent only | instance + persistent mirror | #949 | Hot read path served from instance |
| `DirtyMask(id)` | – | temporary | #948 | New rebuildable cache |
| `PendingDelta(id)` | – | temporary | #948 | New rebuildable cache |
| `DerivedTotals(id)` | – | temporary | #948 | New rebuildable cache |
| `TierHint(id)` | – | temporary | #949 | New rebuildable cache |
| `StateVersion(id)` | – | instance | #948 | New monotonic version counter |
| `DirtyFields(id)` | – | instance | #948 | New flush reconciliation bitmask |
| `Field*` keys | – | persistent | #948 | New per-field incremental state |

## Migration Guide

When renaming a storage key between contract versions, use the migration helpers in `storage_keys.rs`:

```rust
// Migrate persistent storage
migrate_persistent::<OldKey, NewKey, ValueType>(env, &old_key, &new_key);

// Migrate instance storage
migrate_instance::<OldKey, NewKey, ValueType>(env, &old_key, &new_key);
```

Example: migrating from old `admin_key()` to new `StorageKey::Admin`:

```rust
pub fn upgrade(env: Env) {
    let old_admin_key = Symbol::new(&env, "admin");  // or symbol_short!("admin")
    migrate_instance::<Symbol, StorageKey, Address>(
        &env,
        &old_admin_key,
        &StorageKey::Admin,
    );
}
```

## Webhook Storage: `WebhookKey` enum (Issue #930)

Defined privately in `contracts/split/src/webhooks.rs` so the webhook subsystem owns its keys and cannot collide with the four registry enums.

| Variant | Tier | Value | Purpose |
|---------|------|-------|---------|
| `NextWebhookId` | instance | `u64` | Next webhook id |
| `NextDeliveryId` | instance | `u64` | Next delivery id |
| `WebhookIds` | instance | `Vec<u64>` | Ids of all registered webhooks (≤ `MAX_WEBHOOKS`) |
| `Webhook(u64)` | persistent | `Webhook` | Webhook subscription record |
| `OwnerWebhooks(Address)` | persistent | `Vec<u64>` | Webhook ids owned by an address |
| `Delivery(u64)` | persistent | `WebhookDelivery` | Delivery status record |
| `WebhookDeliveries(u64)` | persistent | `Vec<u64>` | Most recent delivery ids per webhook (≤ 50) |

## Uniqueness Validation

Unit tests in `storage_keys.rs` verify that:
1. No two `StorageKey` variants serialize to the same XDR value
2. No two `InvoiceKey` variants serialize to the same XDR value
3. No two `AddressKey` variants serialize to the same XDR value
4. No two `CompoundKey` variants serialize to the same XDR value
5. Different invoice IDs and parameters produce different keys
6. Per-field incremental keys (`Field*`) do not collide with their base entry keys
7. Temporary keys (`TempKey`) do not collide with instance or persistent keys

Run tests with:
```sh
cargo test --lib storage_keys
```

## Usage Analytics Storage (Issue #1034)

Defined privately in `contracts/split/src/analytics.rs` as `symbol_short!` keys. The `an_` prefix is used by no other module.

| Key | Tier | Value | Purpose |
|-----|------|-------|---------|
| `an_on` | instance | `bool` | Recording on/off (absent = on) |
| `an_tot` | instance | `UsageTotals` | Lifetime usage counters |
| `(an_day, u64)` | persistent | `DailyUsage` | Usage for one UTC day (`timestamp / 86_400`) |
| `(an_crt, Address)` | persistent | `bool` | Address has created an invoice (unique-creator count) |
| `(an_pyr, Address)` | persistent | `bool` | Address has paid an invoice (unique-payer count) |
