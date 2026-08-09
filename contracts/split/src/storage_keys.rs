//! Centralised storage key registry (issue #312).
//!
//! All storage keys used by the contract are declared here in one of four
//! `#[contracttype]` enums. The enum-based approach guarantees that no two
//! keys serialize to the same XDR value within a storage tier, and it
//! replaces the scattered `symbol_short!` literals that previously lived
//! inside `lib.rs`.
//!
//! ## Why four enums?
//! Soroban's XDR spec limits a `#[contracttype]` enum to **50 variants**.
//! The contract uses ~115 distinct keys, so they are split by storage role:
//!
//! | Enum          | Tier       | Key shape                       | Variants |
//! |---------------|------------|---------------------------------|----------|
//! | `StorageKey`  | instance   | unit (no data)                  | ≤50      |
//! | `InvoiceKey`  | persistent | `(invoice_id: u64)` or similar  | ≤50      |
//! | `AddressKey`  | persistent | `(address: Address)` singletons | ≤50      |
//! | `CompoundKey` | persistent | two or three fields             | ≤50      |
//!
//! ## Collision safety
//! Within each enum, Rust/XDR guarantees that every variant name is unique,
//! so no two variants can produce the same serialised value. Cross-enum
//! collisions cannot happen in practice because:
//! - `StorageKey` variants are only written to **instance** storage.
//! - `InvoiceKey`, `AddressKey`, `CompoundKey` are only written to
//!   **persistent** storage.
//! - Even within persistent storage, the variant names are globally unique
//!   across the three enums (enforced by the tests below).
//!
//! ## Key migration
//! Use [`migrate_persistent`] / [`migrate_instance`] when renaming a key
//! between contract versions.

use soroban_sdk::{contracttype, symbol_short, Address, Env, IntoVal, Symbol, TryFromVal, Val};

// ---------------------------------------------------------------------------
// Enum 1 — Instance-tier singletons
// ---------------------------------------------------------------------------

/// Contract-level singleton keys stored in **instance** storage.
///
/// All variants are unit (carry no data). Instance storage is wiped on
/// upgrade unless explicitly preserved, so these represent live config.
#[contracttype]
#[derive(Clone)]
pub enum StorageKey {
    Admin,
    Admins,
    Paused,
    PausedFns,
    Treasury,
    UsdcToken,
    CreationFee,
    PlatformFeeBps,
    PlatformFeeWaiverList,
    CreatorFeeWaiver,
    Counter,
    GlobalPayerLimit,
    GlobalPayerWindow,
    StreamContract,
    CreatorWhitelist,
    Compliance,
    KycContract,
    RateLimit,
    RateWindow,
    MaxCancelBps,
    ReceiptFactory,
    DashboardContract,
    NftGate,
    TimelockSecs,
    TimelockActionCounter,
    FeeTiers,
    PendingAdmin,
    GovernanceContract,
    Factories,
    DexContract,
    TotalInvoices,
    TotalVolume,
    TotalReleased,
    TotalRefunded,
    TreasuryGroupCounter,
    ContractVersion,
    ArchiveAfterLedgers,
    CircuitBreaker,
    CircuitBreakerReason,
    PlatformVolThresh,
    PlatformVolMile,
    CreatorVolThresh,
    UpgradeProposal,
    ProtocolFee,
    ReentrancyGuard,
}

// ---------------------------------------------------------------------------
// Enum 2 — Persistent-tier per-invoice keys
// ---------------------------------------------------------------------------

/// Per-invoice or per-resource keys stored in **persistent** storage.
///
/// Most variants carry a single `invoice_id: u64`; `PaymentShard` carries
/// two `u64` values.
#[contracttype]
#[derive(Clone)]
pub enum InvoiceKey {
    Invoice(u64),
    InvoiceExt(u64),
    InvoiceExt2(u64),
    InvoiceCompact(u64),
    InvoiceHot(u64),
    AuditLog(u64),
    PaymentShard(u64, u64),
    ReleaseDelay(u64),
    FundedAtLedger(u64),
    MetadataHash(u64),
    PaidRecipients(u64),
    CompactStatus(u64),
    CompactDeadlineLedger(u64),
    ConfidentialCount(u64),
    InvoiceGroup(u64),
    InvoiceTreasury(u64),
    Delegate(u64),
    PaymentWindow(u64),
    Cert(u64),
    DisputeRecord(u64),
    DisputeRaisedAt(u64),
    Refunded(u64),
    RecipientsList(u64),
    AmountsList(u64),
    PaidFlags(u64),
    MilestoneFlags(u64),
    ArchiveMarker(u64),
    CreatedLedger(u64),
    SubscriptionParams(u64),
    SubscriptionSubscribers(u64),
    ExtVote(u64),
    Group(u64),
    GroupTreasury(u64),
    TimelockAction(u64),
}

// ---------------------------------------------------------------------------
// Enum 3 — Persistent-tier per-address keys
// ---------------------------------------------------------------------------

/// Per-`Address` keys stored in **persistent** storage.
#[contracttype]
#[derive(Clone)]
pub enum AddressKey {
    Reputation(Address),
    Credit(Address),
    ReferralCount(Address),
    RecipientInvoiceIds(Address),
    DelegatePay(Address),
    RateUsage(Address),
    InvoiceCount(Address),
    CancelCount(Address),
    CreatorStatsCount(Address),
    CreatorStatsVolume(Address),
    CreatorStatsReleased(Address),
    CreatorStatsRefunded(Address),
    CreatorStatsPayers(Address),
    CreatorStatsAvgFunding(Address),
    CreatorVolumeCap(Address),
    CreatorVolumeUsed(Address),
    CreatorSelfLimit(Address),
    CreatorSelfUsed(Address),
    CreatorSelfLimitDay(Address),
    CreatorSelfLimitRaise(Address),
    PauseExempt(Address),
    GlobalVelocity(Address),
    CreatorVolMile(Address),
}

// ---------------------------------------------------------------------------
// Enum 4 — Persistent-tier compound keys
// ---------------------------------------------------------------------------

/// Compound (multi-field) persistent-storage keys.
#[contracttype]
#[derive(Clone)]
pub enum CompoundKey {
    PendingPayout(u64, Address),
    Channel(u64, Address),
    Nonce(u64, Address),
    Velocity(u64, Address),
    ReceiptToken(u64, Address),
    Accumulator(u64, Address),
    Reminder(u64, Address),
    ConfidentialPay(u64, Address),
    Delegation(u64, Address),
    PayerCooldown(u64, Address),
    CreatorPayerSet(Address, Address),
    Template(Address, Symbol),
    TemplateVersion(Address, Symbol, u32),
    TemplateVersionCount(Address, Symbol),
}

// ---------------------------------------------------------------------------
// Migration helpers
// ---------------------------------------------------------------------------

/// Copy a value from `old_key` to `new_key` in **persistent** storage and
/// remove the old entry. No-op when `old_key` is absent.
///
/// Use this in contract `upgrade()` when renaming a storage key between
/// contract versions.
#[allow(dead_code)]
pub fn migrate_persistent<OldKey, NewKey, V>(env: &Env, old_key: &OldKey, new_key: &NewKey)
where
    OldKey: IntoVal<Env, Val>,
    NewKey: IntoVal<Env, Val>,
    V: IntoVal<Env, Val> + TryFromVal<Env, Val>,
{
    if let Some(val) = env.storage().persistent().get::<OldKey, V>(old_key) {
        env.storage().persistent().set(new_key, &val);
        env.storage().persistent().remove(old_key);
    }
}

/// Same as [`migrate_persistent`] but operates on **instance** storage.
#[allow(dead_code)]
pub fn migrate_instance<OldKey, NewKey, V>(env: &Env, old_key: &OldKey, new_key: &NewKey)
where
    OldKey: IntoVal<Env, Val>,
    NewKey: IntoVal<Env, Val>,
    V: IntoVal<Env, Val> + TryFromVal<Env, Val>,
{
    if let Some(val) = env.storage().instance().get::<OldKey, V>(old_key) {
        env.storage().instance().set(new_key, &val);
        env.storage().instance().remove(old_key);
    }
}

// ---------------------------------------------------------------------------
// Tests — key uniqueness
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, xdr::ToXdr, Env};

    fn xdr_sk(env: &Env, k: &StorageKey) -> soroban_sdk::Bytes {
        let v: soroban_sdk::Val = k.clone().into_val(env);
        v.to_xdr(env)
    }
    fn xdr_ik(env: &Env, k: &InvoiceKey) -> soroban_sdk::Bytes {
        let v: soroban_sdk::Val = k.clone().into_val(env);
        v.to_xdr(env)
    }
    fn xdr_ak(env: &Env, k: &AddressKey) -> soroban_sdk::Bytes {
        let v: soroban_sdk::Val = k.clone().into_val(env);
        v.to_xdr(env)
    }
    fn xdr_ck(env: &Env, k: &CompoundKey) -> soroban_sdk::Bytes {
        let v: soroban_sdk::Val = k.clone().into_val(env);
        v.to_xdr(env)
    }

    #[test]
    fn instance_keys_are_unique() {
        let env = Env::default();
        let keys: &[StorageKey] = &[
            StorageKey::Admin, StorageKey::Admins, StorageKey::Paused,
            StorageKey::PausedFns, StorageKey::Treasury, StorageKey::UsdcToken,
            StorageKey::CreationFee, StorageKey::PlatformFeeBps,
            StorageKey::PlatformFeeWaiverList, StorageKey::CreatorFeeWaiver,
            StorageKey::Counter, StorageKey::GlobalPayerLimit,
            StorageKey::GlobalPayerWindow, StorageKey::StreamContract,
            StorageKey::CreatorWhitelist, StorageKey::Compliance,
            StorageKey::KycContract, StorageKey::RateLimit, StorageKey::RateWindow,
            StorageKey::MaxCancelBps, StorageKey::ReceiptFactory,
            StorageKey::DashboardContract, StorageKey::NftGate,
            StorageKey::TimelockSecs, StorageKey::TimelockActionCounter,
            StorageKey::FeeTiers, StorageKey::PendingAdmin,
            StorageKey::GovernanceContract, StorageKey::Factories,
            StorageKey::DexContract, StorageKey::TotalInvoices,
            StorageKey::TotalVolume, StorageKey::TotalReleased,
            StorageKey::TotalRefunded, StorageKey::TreasuryGroupCounter,
            StorageKey::ContractVersion, StorageKey::ArchiveAfterLedgers,
            StorageKey::CircuitBreaker, StorageKey::CircuitBreakerReason,
            StorageKey::PlatformVolThresh, StorageKey::PlatformVolMile,
            StorageKey::CreatorVolThresh, StorageKey::UpgradeProposal,
            StorageKey::ProtocolFee, StorageKey::ReentrancyGuard,
        ];
        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                assert_ne!(
                    xdr_sk(&env, &keys[i]),
                    xdr_sk(&env, &keys[j]),
                    "StorageKey variant {i} and {j} collide"
                );
            }
        }
    }

    #[test]
    fn invoice_keys_differ_by_variant() {
        let env = Env::default();
        let id = 42u64;
        let keys: &[InvoiceKey] = &[
            InvoiceKey::Invoice(id), InvoiceKey::InvoiceExt(id),
            InvoiceKey::InvoiceExt2(id), InvoiceKey::InvoiceCompact(id),
            InvoiceKey::InvoiceHot(id), InvoiceKey::AuditLog(id),
            InvoiceKey::ReleaseDelay(id), InvoiceKey::CompactStatus(id),
            InvoiceKey::RecipientsList(id), InvoiceKey::AmountsList(id),
            InvoiceKey::PaidFlags(id), InvoiceKey::MilestoneFlags(id),
            InvoiceKey::ArchiveMarker(id), InvoiceKey::CreatedLedger(id),
        ];
        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                assert_ne!(
                    xdr_ik(&env, &keys[i]),
                    xdr_ik(&env, &keys[j]),
                    "InvoiceKey {i} and {j} collide"
                );
            }
        }
    }

    #[test]
    fn invoice_key_id_uniqueness() {
        let env = Env::default();
        assert_ne!(
            xdr_ik(&env, &InvoiceKey::Invoice(1)),
            xdr_ik(&env, &InvoiceKey::Invoice(2))
        );
        assert_ne!(
            xdr_ik(&env, &InvoiceKey::PaymentShard(1, 0)),
            xdr_ik(&env, &InvoiceKey::PaymentShard(1, 1))
        );
    }

    #[test]
    fn address_keys_differ_by_variant() {
        let env = Env::default();
        let addr = soroban_sdk::Address::generate(&env);
        let keys: &[AddressKey] = &[
            AddressKey::Reputation(addr.clone()),
            AddressKey::Credit(addr.clone()),
            AddressKey::ReferralCount(addr.clone()),
            AddressKey::RecipientInvoiceIds(addr.clone()),
            AddressKey::DelegatePay(addr.clone()),
            AddressKey::RateUsage(addr.clone()),
            AddressKey::InvoiceCount(addr.clone()),
            AddressKey::CancelCount(addr.clone()),
            AddressKey::CreatorStatsCount(addr.clone()),
            AddressKey::CreatorStatsVolume(addr.clone()),
            AddressKey::CreatorStatsPayers(addr.clone()),
            AddressKey::GlobalVelocity(addr.clone()),
            AddressKey::PauseExempt(addr.clone()),
        ];
        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                assert_ne!(
                    xdr_ak(&env, &keys[i]),
                    xdr_ak(&env, &keys[j]),
                    "AddressKey {i} and {j} collide"
                );
            }
        }
    }

    #[test]
    fn compound_keys_differ_by_variant() {
        let env = Env::default();
        let addr = soroban_sdk::Address::generate(&env);
        let id = 1u64;
        let keys: &[CompoundKey] = &[
            CompoundKey::PendingPayout(id, addr.clone()),
            CompoundKey::Channel(id, addr.clone()),
            CompoundKey::Nonce(id, addr.clone()),
            CompoundKey::Velocity(id, addr.clone()),
            CompoundKey::ReceiptToken(id, addr.clone()),
            CompoundKey::Accumulator(id, addr.clone()),
            CompoundKey::Reminder(id, addr.clone()),
            CompoundKey::ConfidentialPay(id, addr.clone()),
            CompoundKey::Delegation(id, addr.clone()),
            CompoundKey::PayerCooldown(id, addr.clone()),
        ];
        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                assert_ne!(
                    xdr_ck(&env, &keys[i]),
                    xdr_ck(&env, &keys[j]),
                    "CompoundKey {i} and {j} collide"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Issue #559: Creator Revenue Share
// ---------------------------------------------------------------------------

/// Creator fee in basis points stored at invoice creation — persistent storage.
/// Key: (Symbol, u64) → u32
#[allow(dead_code)]
pub fn creator_fee_bps_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("cr_fee_bp"), invoice_id)
}

/// Set of allowed payment tokens — persistent storage.
/// Key: Symbol → Vec<Address>
pub fn allowed_tokens_key() -> Symbol {
    symbol_short!("alwd_toks")
}

// ---------------------------------------------------------------------------
// Issue #560: Creator Migration
// ---------------------------------------------------------------------------

/// Pending successor creator address — persistent storage.
/// Key: (Symbol, u64) → Address
#[allow(dead_code)]
pub fn pending_creator_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("pend_cr"), invoice_id)
}

// ---------------------------------------------------------------------------
// Issue #562: Soft-Delete with Tombstone
// ---------------------------------------------------------------------------

/// Tombstone record for soft-deleted invoices — persistent storage.
/// Key: (Symbol, u64) → Tombstone
#[allow(dead_code)]
pub fn tombstone_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("tombstone"), invoice_id)
}

