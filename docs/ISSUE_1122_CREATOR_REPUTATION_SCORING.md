# Issue #1122: Creator reputation scoring

**Status:** design only. No reputation scoring module for creators is implemented.

---

## Goal

Give every creator an on-chain reputation score derived entirely from their
existing activity counters, so payers, integrators, and governance tooling can
assess a creator's track record without trusting an off-chain source.

The design re-uses the per-creator counters already written by
`update_creator_stats_on_creation`, `update_creator_stats_on_payment`, and
`update_creator_stats_on_release` — no new hot-path bookkeeping is required.

---

## Background: existing signals

The contract already accumulates these per-creator persistent-storage counters
on every relevant action:

| Storage key (via `AddressKey`) | Type | Incremented by |
|---|---|---|
| `CreatorStatsCount` | `u64` | `update_creator_stats_on_creation` on `create_invoice` |
| `CreatorStatsVolume` | `u64` | `update_creator_stats_on_payment` on `pay` |
| `CreatorStatsReleased` | `u64` | `update_creator_stats_on_release` on `release` |
| `CreatorStatsRefunded` | `u64` | (refund path) |
| `CreatorStatsPayers` | `u64` | `update_creator_payers` — unique payer set size |
| `CreatorStatsAvgFunding` | `u64` | running average funding time in ledgers |

`get_creator_stats` already reads all six into `CreatorStats`. The reputation
score is a pure function of those values, so computing it costs one persistent
read per field (six reads total) and no writes on the calling path.

The payer-side `RepScore` struct (`types.rs`, issue #349) is a parallel
concept: it records `paid_on_time`, `late_pays`, `invoices_released`, and
`invoices_refunded` for payers. This issue defines the creator analogue.

---

## Proposed model

### Score formula

```
release_rate  = total_released  / max(total_invoices, 1)   -- ratio 0–1 scaled ×10 000
refund_rate   = total_refunded  / max(total_invoices, 1)   -- ratio 0–1 scaled ×10 000

score = (release_rate_bps  * WEIGHT_RELEASE
       + payer_depth_score * WEIGHT_PAYERS
       + volume_score      * WEIGHT_VOLUME)
       .saturating_sub(refund_rate_bps * WEIGHT_REFUND)
```

Where:

- **`release_rate_bps`** — `total_released * 10_000 / max(total_invoices, 1)`.
  A creator who releases every invoice scores 10 000 here.
- **`payer_depth_score`** — `min(total_payers, PAYER_DEPTH_CAP) * 10_000 / PAYER_DEPTH_CAP`.
  Saturates once payer count exceeds `PAYER_DEPTH_CAP` (default 500), so whale
  creators do not dominate the signal.
- **`volume_score`** — `min(total_volume_stroops, VOLUME_CAP) * 10_000 / VOLUME_CAP`.
  Saturates at `VOLUME_CAP` (admin-configurable; default 1 000 000 XLM in
  stroops). Volume alone is not sufficient — it is weighted lower than release
  rate.
- **`refund_rate_bps`** — `total_refunded * 10_000 / max(total_invoices, 1)`.
  Subtracted from the score; a creator who refunds every invoice receives the
  maximum penalty.

Default weights (all sum to 100):

| Constant | Default | Meaning |
|---|---|---|
| `WEIGHT_RELEASE` | 50 | Release-rate contribution |
| `WEIGHT_PAYERS` | 25 | Payer-depth contribution |
| `WEIGHT_VOLUME` | 15 | Volume contribution |
| `WEIGHT_REFUND` | 10 | Refund-rate deduction |

The final score is in `[0, 10 000]` (basis-point scale). Saturating arithmetic
is used throughout so a stale or extreme input cannot cause a panic or overflow.

### Minimum-invoice gate

Creators with fewer than `MIN_SCORED_INVOICES` total invoices (default 3)
return a `CreatorReputation` with `score = None` and `is_scored = false`.
This prevents a single released invoice from producing a misleading score of
10 000 for a new creator.

---

## New type

```rust
/// Issue #1122: Computed reputation for a creator.
#[contracttype]
pub struct CreatorReputation {
    pub creator: Address,
    /// Computed score in [0, 10_000], or None when below the minimum-invoice gate.
    pub score: Option<u32>,
    /// False when total_invoices < MIN_SCORED_INVOICES.
    pub is_scored: bool,
    /// release_rate_bps component (0–10_000).
    pub release_rate_bps: u32,
    /// refund_rate_bps component (0–10_000).
    pub refund_rate_bps: u32,
    /// payer_depth_score component (0–10_000).
    pub payer_depth_score: u32,
    /// volume_score component (0–10_000).
    pub volume_score: u32,
    /// Snapshot of the stats used in this computation.
    pub stats: CreatorStats,
}
```

---

## Storage

No new storage keys are required by the read path. If the admin wants to
persist a snapshot for indexers (optional), one new instance key is added:

| Key | Tier | Type | Description |
|---|---|---|---|
| `CreatorRepConfig` (`StorageKey` variant) | instance | `CreatorRepConfig` | Weight constants and caps; set by admin. |

```rust
/// Issue #1122: Admin-configurable weights and caps for creator reputation scoring.
#[contracttype]
pub struct CreatorRepConfig {
    pub weight_release: u32,
    pub weight_payers: u32,
    pub weight_volume: u32,
    pub weight_refund: u32,
    pub payer_depth_cap: u32,
    pub volume_cap: i128,
    pub min_scored_invoices: u32,
    pub version: u32,
}
```

`CreatorRepConfig` is optional. When absent the defaults above apply, so
`get_creator_reputation` works on a contract that has never called
`set_creator_rep_config`.

---

## Entry points

| Function | Access | Description |
|---|---|---|
| `get_creator_reputation(creator)` | anyone | Compute and return a `CreatorReputation`. Reads six persistent keys and one optional instance key. No writes. |
| `set_creator_rep_config(admin, config)` | SuperAdmin | Replace the weight/cap/gate config. Emits `crep_cfg`. |
| `get_creator_rep_config()` | anyone | Return the current `CreatorRepConfig`, or the defaults if not set. |

`get_creator_reputation` is intentionally read-only and permissionless. Any
payer or indexer can call it without auth. Because it makes no writes it also
cannot revert an in-progress payment.

---

## Events

| Topic tuple | Data | When |
|---|---|---|
| `("split", "crep_cfg")` | `(version, timestamp)` | `set_creator_rep_config` succeeds |

No event is emitted for `get_creator_reputation` because it is a pure read.

---

## Implementation sketch

The score computation belongs in a new module `creator_reputation.rs` following
the same pattern as `creator_fee_tiers.rs`:

```
contracts/split/src/creator_reputation.rs
```

`lib.rs` declares it with `mod creator_reputation;` and delegates:

```rust
pub fn get_creator_reputation(env: Env, creator: Address) -> CreatorReputation {
    creator_reputation::compute(&env, &creator)
}
```

`compute` reads `get_creator_stats` (already public in `lib.rs`), loads the
optional config from instance storage, applies the formula, and returns the
`CreatorReputation` struct. No calls into other modules; no cross-contract
invocations.

### Integration with fee tiers

`get_creator_reputation` is independent of `creator_fee_tiers.rs`. The two
systems use different signals and different storage keys. A future issue could
gate fee-tier promotions on a minimum reputation score, but that is out of
scope here.

### Integration with payer `RepScore`

The existing `RepScore` struct and `update_rep_internal` function (issue #349)
are for payers. `CreatorReputation` is for creators. Neither reads the other's
storage.

---

## Constraints

- Computation is `O(1)`: six persistent reads, one optional instance read.
- Saturating arithmetic prevents overflow without panicking.
- The module adds no writes to the payment hot path.
- The `AddressKey::Reputation` variant already exists in `storage_keys.rs`
  (enum 3). That variant stores payer `RepScore` data; the creator reputation
  computation is derived on the fly and not separately persisted, so no new
  `AddressKey` variant is needed. The `StorageKey::CreatorRepConfig` variant
  must be appended to `StorageKey` (enum 1) when implemented — verify that
  the enum is below the 50-variant Soroban limit before merging.

---

## Open questions

- Whether the score should be cached in persistent storage to save re-computation on repeated reads.
  Given the formula is `O(1)` and the six stats are already in persistent storage, caching adds write
  overhead without clear benefit. Revisit if profiling shows otherwise.
- Whether `VOLUME_CAP` should be denominated in USDC stroops or in a
  token-agnostic unit. Currently `CreatorStatsVolume` accumulates in the
  invoice's funding token, which is USDC in the common case. A multi-token
  cap would require an oracle, which is out of scope.
- Whether a floor score should be provided to creators below the
  `MIN_SCORED_INVOICES` gate (e.g. `score = 0`) rather than `None`, for easier
  downstream comparison.

---

## Not done

No code, no tests, no migration. The `CreatorReputation` type, the
`creator_reputation` module, the `StorageKey::CreatorRepConfig` variant, and
the `get_creator_reputation` / `set_creator_rep_config` / `get_creator_rep_config`
entry points all remain to be implemented.
