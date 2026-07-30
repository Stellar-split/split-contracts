# Storage Key Registry

This document enumerates every storage key used by the StellarSplit contract, organized by storage tier and key type. Keys are defined in `contracts/split/src/storage_keys.rs` as `#[contracttype]` enums to prevent collisions and ensure XDR uniqueness.

## Storage Tiers

| Tier | Lifecycle | Keys | Typical TTL |
|------|-----------|------|------------|
| **instance** | Live during contract execution; wiped on upgrade unless explicitly preserved. | Singletons (config, counters, addresses) | Contract lifetime |
| **persistent** | Survives contract upgrades; archived after `ARCHIVE_AFTER_LEDGERS` (~100k ledgers) | Per-entity state | 100k ledgers or longer |
| **temporary** | Short-lived ephemeral state (not yet used) | None | – |

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

## Persistent Storage: `InvoiceKey` enum

Keys keyed by invoice ID or sharded. All carry a u64 invoice ID unless noted.

| Variant | Key Shape | Introduced | Type | Purpose |
|---------|-----------|------------|------|---------|
| `Invoice(id)` | `(Symbol, u64)` | v1 | InvoiceCore | Core invoice fields (recipients, amounts, deadlines) |
| `InvoiceExt(id)` | `(Symbol, u64)` | v1 | InvoiceExt | Extended fields (prerequisites, tranches, etc.) |
| `InvoiceExt2(id)` | `(Symbol, u64)` | v1 | InvoiceExt2 | Secondary extended fields (release stages, etc.) |
| `InvoiceCompact(id)` | `(Symbol, u64)` | #334 | Bytes | Compact XDR-encoded invoice data |
| `InvoiceHot(id)` | `(Symbol, u64)` | v1 | InvoiceHot | Hot (frequently-accessed) fields; also in instance storage |
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

## Uniqueness Validation

Unit tests in `storage_keys.rs` verify that:
1. No two `StorageKey` variants serialize to the same XDR value
2. No two `InvoiceKey` variants serialize to the same XDR value
3. No two `AddressKey` variants serialize to the same XDR value
4. No two `CompoundKey` variants serialize to the same XDR value
5. Different invoice IDs and parameters produce different keys

Run tests with:
```sh
cargo test --lib storage_keys
```
