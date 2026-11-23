# Issue #1146: Volume-based fee tier system

**Status:** design. Most of the tier logic already exists (issue #919), but no tier rate is charged
at release yet. This document describes what exists, what is missing, and the changes needed to meet
the acceptance criteria.

## Goal

Creators with a high lifetime volume pay a lower platform fee. The discount grows with volume, the
admin can change the tier table, and the contract records how much discount each creator received.

## Relevant existing code

There are two tier tables in the `split` contract today. This design keeps one of them.

| Table | Location | Storage | State |
|---|---|---|---|
| `CreatorFeeTier` table (issue #919) | `contracts/split/src/creator_fee_tiers.rs` | `cfee_cfg` (instance), `(cfee_trk, creator)` (persistent) | Validated, versioned, tested. Not used at release. |
| `FeeTier` table (older) | `set_fee_tiers` / `get_applicable_fee` in `contracts/split/src/lib.rs` | `fee_tiers_key()` (instance) | Unvalidated in release builds. Used only to size the early-bird discount. |

What the #919 module already does:

- `set_creator_fee_tiers`, `update_creator_fee_tier_bps` and `clear_creator_fee_tiers` are restricted
  to admins. Each update increases `CreatorFeeTierConfig.version`.
- `validate_tiers` enforces 1 to 10 tiers, `tier_id` equal to the tier's position, a first tier at
  `min_volume == 0`, strictly ascending `min_volume`, `fee_bps` that never increases from one tier
  to the next, and `fee_bps <= MAX_CREATOR_FEE_BPS` (1 000, i.e. 10%).
- `get_creator_fee_tier` finds a creator's tier by binary search over the table, using the creator's
  lifetime volume (`creator_stats_volume_key`, stored as a normalised `u64`).
- `calc_creator_fee` returns a `CreatorFeeQuote` (gross amount, fee, net amount). The fee is rounded
  down.
- `update_creator_stats_on_payment` calls `sync_creator_fee_tier` after every payment, which writes
  a `CreatorTierRecord` (tier, rate, config version, `since`).

## Gaps against the acceptance criteria

1. **Tiers are not enforced.** The release paths (`lib.rs`, the `calc_platform_fee` calls near lines
   10743, 11301 and 12026) read the global `platform_fee_bps_key()`. No creator tier is consulted.
2. **No discount is tracked.** No storage or event records how much a creator saved.
3. **Two tables use one concept.** `get_applicable_fee` reads the older `FeeTier` table, so the
   early-bird discount (issue #489) uses different rates from `calc_creator_fee`.
4. **Storage type mismatch.** Creation-fee code in `lib.rs` (around line 6647) reads
   `fee_tiers_key()` from **persistent** storage as `Vec<(i128, u32)>`. `set_fee_tiers` writes it to
   **instance** storage as `Vec<FeeTier>`. The read never finds a value, so the creation-fee discount
   is always 0.
5. **`set_fee_tiers` is not validated in release builds.** Its checks use `debug_assert!`, which is
   compiled out of the release WASM.

## Proposed design

### Rate resolution

Add one internal function and use it everywhere a platform fee rate is needed:

```rust
/// Rate for `creator`: the tier rate if a tier table is set, else the global rate.
/// Never higher than the global rate.
fn effective_platform_fee_bps(env: &Env, creator: &Address) -> u32
```

- With no `cfee_cfg`, it returns `platform_fee_bps_key()`, so behaviour is unchanged until an admin
  publishes a table.
- With a table, it returns `min(tier.fee_bps, global_bps)`. A tier can lower the fee, never raise
  it. This is the "fair pricing" rule.
- `get_applicable_fee` becomes a thin wrapper around this function, so the early-bird discount and
  release use the same rate.

### Lock the rate at invoice creation

Store the resolved rate on the invoice when it is created (the existing `creator_fee_bps_key` slot or
a new `(inv_feebp, invoice_id)` key). Release reads the stored rate, not the creator's current tier.

Reasons:

- A creator who crosses a tier boundary partway through an invoice cannot get a retroactive discount
  on contributions already made.
- An admin who changes the table cannot change the fee on invoices that are already open.

Invoices created before this change have no stored rate and fall back to the global rate.

### Discount tracking

At each release, compute:

```text
standard_fee   = calc_platform_fee(amount, global_bps)
charged_fee    = calc_platform_fee(amount, invoice_rate_bps)
discount       = standard_fee - charged_fee      // always >= 0
```

Record it in three places:

| What | Key | Storage |
|---|---|---|
| Per-invoice discount | `(fee_disc, invoice_id)` → `i128` | persistent |
| Per-creator lifetime discount | `(cfee_dsum, creator)` → `i128` | persistent |
| Event | `fee_tier_discount(invoice_id, creator, tier_id, standard_fee, charged_fee)` | event |

New read entry points: `get_invoice_fee_discount(invoice_id) -> i128` and
`get_creator_fee_discount_total(creator) -> i128`.

### Tier updates

- `set_creator_fee_tiers` and `update_creator_fee_tier_bps` keep their current validation and
  versioning. Both emit `creator_fee_tiers_updated(version, tier_count)`.
- `CreatorTierRecord.config_version` already lets a client see that a creator's record was computed
  against an older table. `sync_creator_fee_tier` stays permissionless, so anyone can refresh a stale
  record.
- An update applies only to invoices created after it, because the rate is locked at creation.

### Retiring the old table

1. `set_fee_tiers` becomes a deprecated wrapper that converts `Vec<FeeTier>` into a
   `CreatorFeeTier` table and calls `set_creator_fee_tiers`. Real validation replaces the
   `debug_assert!` checks.
2. The creation-fee code reads its discount through `effective_platform_fee_bps`. That removes the
   `Vec<(i128, u32)>` read and fixes gap 4.
3. A migration in `migrations.rs` copies any stored `fee_tiers_key()` table into `cfee_cfg`, then
   removes the old key.

## Worked example

Global rate 250 bps (2.5%). Tier table:

| tier_id | name | min_volume | fee_bps |
|---|---|---|---|
| 0 | `bronze` | 0 | 250 |
| 1 | `silver` | 100 000 | 200 |
| 2 | `gold` | 1 000 000 | 150 |

A creator with lifetime volume 250 000 is `silver`. They create an invoice, which stores the rate
200 bps. A 10 000 release charges 200 instead of 250 and records a discount of 50.

## Test plan

| Area | Test |
|---|---|
| Enforcement | Release charges the stored tier rate, not the global rate. |
| Enforcement | No table set: release charges the global rate (regression). |
| Fairness | A tier rate above the global rate is capped at the global rate. |
| Locking | A creator who crosses a tier mid-invoice keeps the rate stored at creation. |
| Locking | A table update after creation does not change that invoice's fee. |
| Tracking | Per-invoice and per-creator discount totals equal `standard_fee - charged_fee`, summed over partial releases. |
| Tracking | A zero discount writes no key and emits no event. |
| Updates | The version increases on each update, and a stale `CreatorTierRecord` is refreshed by `sync_creator_fee_tier`. |
| Legacy | `set_fee_tiers` rejects an unsorted table in a release build. |
| Legacy | The creation-fee discount is applied (regression test for gap 4). |
| Migration | An old `fee_tiers_key()` table is copied into `cfee_cfg`. |

## Open questions

- Should volume count only released funds instead of all payments? Today a payment that is later
  refunded still counts towards a tier.
- Should volume decay or use a rolling window, so a creator who stops trading eventually drops a tier?
- Is 10 tiers and a 10% cap the right limit?
