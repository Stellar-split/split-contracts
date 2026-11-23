# Issue #1144: Creator reputation scoring

**Status:** design only. The contract already keeps reputation counters (issues #24, #349). This
document proposes a creator-focused score built on those counters, with history and queries.

## Goal

Measure, on-chain, how reliably a creator delivers on their invoices, in a way that other contracts
and the frontend can read cheaply and that a creator cannot easily game.

## Relevant existing code

All in `contracts/split/src/lib.rs` unless noted.

| Item | What it does |
|---|---|
| `RepScore` (`types.rs`) | Four `u32` counters per address: `paid_on_time`, `late_pays`, `invoices_released`, `invoices_refunded`. |
| `rep_key(address)` → `(rep, address)` | Persistent storage for the counters. |
| `update_rep_internal` | Applies a change, saves it, and emits `rep_updated` with a derived score: `(paid_on_time·10 + invoices_released·5) − (late_pays·5 + invoices_refunded·2)`, floored at 0. |
| Payment path | Increments the **payer's** `paid_on_time` or `late_pays`. |
| Release path | Increments the **creator's** `invoices_released`. |
| Refund path | Increments the **creator's** `invoices_refunded`. |
| `get_rep(address)` | Returns the raw counters. |
| `get_reputation(address)` | Returns `paid_on_time + late_pays` (payment count only). |
| `min_payer_rep` on invoices | Gates payment on the payer's `paid_on_time`. |
| `get_creator_rating(creator)` | Separate payer-submitted star rating: `(sum, count)`. |

### Problems with the current state

1. **Payer and creator signals are mixed.** One `RepScore` per address holds both payer counters and
   creator counters. An address that pays a lot looks like a reliable creator.
2. **The derived score is never stored or queryable.** It exists only in the `rep_updated` event, and
   `get_reputation` returns a different number.
3. **No history.** Only the current counters exist.
4. **Refunds are all treated the same.** A refund because the creator cancelled and a refund because
   the deadline passed without full funding both count against the creator.
5. **Easy to inflate.** A creator can create small invoices, pay them from their own second address
   and release them, raising `invoices_released` for the cost of fees.

## Proposed design

### Data model

A separate record for creators, so payer counters are left alone:

```rust
#[contracttype]
pub struct CreatorReputation {
    pub released: u32,           // invoices fully released
    pub released_volume: i128,   // normalised amount released
    pub creator_cancelled: u32,  // cancelled or refunded by the creator after funding started
    pub expired_unfunded: u32,   // refunded because the deadline passed without full funding
    pub disputes_lost: u32,      // disputes resolved against the creator
    pub late_releases: u32,      // released after the invoice deadline
    pub score: u32,              // 0..=1000, recomputed on every update
    pub updated_at: u64,
    pub version: u32,            // increases on every update; indexes the history
}
```

Storage:

| Key | Value | Storage |
|---|---|---|
| `(crep, creator)` | `CreatorReputation` | persistent |
| `(crep_h, creator, version)` | `ReputationEvent` | persistent |
| `crep_cfg` | `ReputationWeights` | instance |

The existing `RepScore` stays as the payer reputation, unchanged, so `min_payer_rep` keeps working.

### Scoring

The score is in `0..=1000` and is computed in integer basis points so the result is deterministic.

```text
outcomes      = released + creator_cancelled + disputes_lost + expired_unfunded
success_bps   = released · 10_000 / outcomes                   (10_000 if outcomes = 0)
penalty_bps   = (creator_cancelled · w_cancel
                + disputes_lost    · w_dispute
                + late_releases    · w_late) · 10_000 / outcomes
confidence    = min(outcomes, N_full) · 10_000 / N_full
raw           = max(success_bps − penalty_bps, 0)
score         = (raw · confidence + NEUTRAL · (10_000 − confidence)) / 10_000 / 10
```

How each term keeps the score fair:

- **A ratio, not a count.** Size alone does not buy reputation, and one failure costs a small creator
  as much, proportionally, as a large one.
- **Confidence weighting.** A new creator starts at `NEUTRAL` (500) and moves towards their real
  ratio as outcomes accumulate, reaching full weight at `N_full` (default 20). One lucky release is
  not a perfect score.
- **Different penalties by cause.** `expired_unfunded` lowers the success ratio but carries no extra
  penalty, because a lack of payers is not the same as the creator failing to deliver. Lost disputes
  carry the largest penalty.
- **Inflation resistance.** An outcome counts only when the invoice was funded by at least one
  address other than the creator and co-creators, and its funded amount is at least
  `min_counted_amount`. Self-funded test invoices then change nothing.

Default weights (`ReputationWeights`, admin-settable, version-stamped):

| Weight | Default |
|---|---|
| `w_cancel` | 1.0 (10 000 bps) |
| `w_dispute` | 2.0 (20 000 bps) |
| `w_late` | 0.25 (2 500 bps) |
| `N_full` | 20 |
| `min_counted_amount` | admin-set, normalised units |

### Updates

Updates happen only inside existing lifecycle functions. There is no public entry point that writes a
score.

| Lifecycle event | Change |
|---|---|
| Release completes | `released += 1`, `released_volume += funded`; `late_releases += 1` if after the deadline |
| Creator cancels a funded invoice | `creator_cancelled += 1` |
| Refund after deadline, not fully funded | `expired_unfunded += 1` |
| Dispute resolved against the creator | `disputes_lost += 1` |

Each update recomputes `score`, increases `version`, writes a history entry and emits
`creator_rep_updated(creator, old_score, new_score, reason)`.

A weight change does not rewrite stored scores. Each record is recomputed with the new weights the
next time it is updated, or by a permissionless `refresh_creator_reputation(creator)`.

### History

```rust
#[contracttype]
pub struct ReputationEvent {
    pub reason: Symbol,      // released, cancelled, expired, dispute, late, refresh
    pub invoice_id: u64,     // 0 for refresh
    pub score_before: u32,
    pub score_after: u32,
    pub timestamp: u64,
}
```

Each entry is stored under its own key, `(crep_h, creator, version)`, so writing one is a single
ledger write however long the history becomes. Persistent entries can be archived by the network. The
event stream is the permanent record, and on-chain history is for recent lookups.

### Queries

| Function | Cost |
|---|---|
| `get_creator_reputation(creator) -> CreatorReputation` | 1 read |
| `get_creator_score(creator) -> u32` | 1 read |
| `get_creator_reputation_history(creator, from_version, limit) -> Vec<ReputationEvent>` | `limit` reads, limit at most 50 |
| `meets_creator_reputation(creator, min_score) -> bool` | 1 read, for other contracts |

`get_reputation` and `get_rep` keep their current behaviour for compatibility.

## Migration

Existing `RepScore.invoices_released` and `invoices_refunded` cannot be separated by cause or checked
for self-funding. Creators start with an empty `CreatorReputation`, which is the neutral score. No
backfill is done, because backfilling with the old counters would carry the problems listed above
into the new score.

## Test plan

| Area | Test |
|---|---|
| Scoring | A new creator scores 500. |
| Scoring | 20 releases and no failures scores 1000; 10 releases and 10 cancellations scores 0. |
| Scoring | `expired_unfunded` lowers the score less than `creator_cancelled`. |
| Fairness | A self-funded invoice and an invoice below `min_counted_amount` change nothing. |
| Fairness | With the same ratio, more outcomes moves the score further from 500. |
| Separation | Paying invoices does not change an address's creator score. |
| Updates | Every lifecycle event increases `version` by one and writes one history entry. |
| Updates | A weight change takes effect on `refresh_creator_reputation`. |
| History | Pages are returned in order, and `limit` above 50 panics. |
| Compatibility | `get_reputation`, `get_rep` and `min_payer_rep` behave as before. |
| Overflow | Counters at `u32::MAX` saturate without panicking. |

## Open questions

- Should older outcomes count less over time (decay), or should a score cover only a rolling window?
- Should disputes count when opened or only when resolved? This design counts them when resolved.
- Should co-creators share the creator's reputation?
- Should `get_creator_rating` (payer star ratings) feed into the score, or stay separate?
