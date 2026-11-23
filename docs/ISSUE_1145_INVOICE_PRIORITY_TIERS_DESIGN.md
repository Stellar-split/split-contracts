# Issue #1145: Invoice priority tiers

**Status:** design. The tier model, queues and queries already exist in
`contracts/split/src/priority.rs` (issue #918). This document records that design and lists the
changes needed to meet the acceptance criteria, mainly so that priority actually controls processing
order.

## Goal

Each open invoice can carry a priority. Work that processes invoices in bulk (keepers, batch release)
handles higher-priority invoices first, and the order is the same for everyone.

## Existing design (issue #918)

### Tiers

A score in `0..=1000` maps to exactly one tier. The weight is the relative share of processing
capacity a tier should receive, for example keeper batch slots.

| Tier | Score range | Weight |
|---|---|---|
| `Low` | 0 – 249 | 1 |
| `Normal` | 250 – 499 | 2 |
| `High` | 500 – 749 | 4 |
| `Critical` | 750 – 1000 | 8 |

An invoice with no priority record is treated as score 0 (`Low`).

### Ordering

Higher score first. Equal scores go in ascending invoice id order. Ids are allocated in increasing
order, so this is first-in, first-out within a score.

### Who can set a priority

- The creator or a co-creator may set any score below `Critical`.
- An admin (any role in the `admins_key()` map) may set any score, including `Critical`, on any
  invoice.
- Only invoices in `Pending` or `PartiallyReleased` status can be prioritised.

### Storage

| Key | Value | Storage |
|---|---|---|
| `(ipri_rec, invoice_id)` | `InvoicePriority { score, tier, set_by, updated_at }` | persistent |
| `(ipri_q, tier)` | `Vec<(invoice_id, score)>`, kept in processing order | persistent |

Each tier queue holds at most `MAX_PRIORITY_QUEUE_LEN` (200) entries. Inserts use binary search, so
an insert costs O(log n) comparisons plus one vector write.

### Entry points

| Function | Purpose |
|---|---|
| `set_invoice_priority(caller, invoice_id, score)` | Create or change a priority. Moves the invoice between queues when its tier changes. |
| `clear_invoice_priority(caller, invoice_id)` | Remove the record and the queue entry. |
| `get_invoice_priority(invoice_id)` | Read one record. |
| `get_priority_queue(tier)` | Invoice ids in one tier, in processing order. |
| `get_next_priority_invoice()` | Head of the most urgent non-empty tier. |
| `sort_invoices_by_priority(ids)` | Sort up to 50 ids into processing order. |
| `get_priority_tier_for_score(score)`, `get_priority_tier_weight(tier)` | Pure helpers. |
| `prune_priority_queue(tier)` | Permissionless cleanup of closed invoices. |

## Gaps against the acceptance criteria

1. **Order is not respected on-chain.** No on-chain processing path reads the priority. The batch
   processor (`batch_processor.rs`) and batch release run in the order the caller supplies. Priority
   is advisory only.
2. **Stale queue heads.** An invoice that is released, refunded or cancelled stays in its queue until
   someone calls `prune_priority_queue`. Until then `get_next_priority_invoice` can return a closed
   invoice.
3. **No processing-order record.** Nothing records when an invoice was processed relative to its
   priority, so there is no way to check after the fact that the order was respected.
4. **A full queue blocks updates.** At 200 entries `set_invoice_priority` panics, including when the
   only entries left are stale ones.

## Proposed changes

### 1. Remove invoices from queues when they close

On every transition out of `Pending` / `PartiallyReleased` (release, refund, cancel, delete), call
`remove_from_queue` for the invoice's tier and delete its `ipri_rec`. This costs one extra read and
write per closing invoice that has a priority record, and nothing for invoices without one.

`prune_priority_queue` stays as a repair tool for invoices closed before this change.

### 2. Enforce order in batch processing

Add an opt-in check to the batch entry points (`batch_release`, the batch processor):

```rust
pub fn batch_release_by_priority(env: Env, caller: Address, max: u32) -> Vec<u64>
```

- Takes invoices from the queue heads, `Critical` first, up to `max` (bounded by
  `MAX_PRIORITY_SORT_BATCH`).
- Allocates the batch by tier weight: of 15 slots with all tiers non-empty, 8 go to `Critical`, 4 to
  `High`, 2 to `Normal` and 1 to `Low`. Slots a tier cannot fill go to the next tier down. The
  weights guarantee that `Low` invoices are not starved indefinitely.
- Skips (and removes) any entry that is not releasable, so one bad entry cannot block the batch.

Existing batch functions that take an explicit list keep working unchanged. Callers that want the
contract to choose the order use the new function.

### 3. Processing-order tracking

When an invoice is processed through the priority path, emit:

```text
priority_processed(invoice_id, tier, score, sequence)
```

`sequence` comes from a counter stored under `ipri_seq` (instance) that increases by one on each
processed invoice. An indexer can check that no invoice was processed before a higher-ranked one that
was already queued.

### 4. Handle full queues

When a queue is full, `insert_into_queue` first drops entries for closed invoices from that queue.
It panics only if the queue is still full afterwards.

### 5. Events for updates

No change needed. `set_invoice_priority` already emits `invoice_priority_set` (with old score, new
score and tier), and `clear_invoice_priority` emits `invoice_priority_cleared`. Both are in
`events.rs`.

## Test plan

| Area | Test |
|---|---|
| Tiers | Every boundary score (0, 249, 250, 499, 500, 749, 750, 1000) maps to the right tier; 1001 panics. |
| Authorisation | A creator cannot set `Critical`; an admin can; a stranger cannot set any score. |
| Updates | Changing the score across a tier boundary moves the invoice between queues; changing it within a tier re-sorts the queue. |
| Order | Equal scores are ordered by ascending id. |
| Order | `batch_release_by_priority` processes `Critical` before `High`, and so on down. |
| Fairness | With all tiers non-empty, a batch includes at least one `Low` invoice. |
| Cleanup | Releasing, refunding or cancelling an invoice removes it from its queue. |
| Cleanup | A full queue made up of stale entries accepts a new insert. |
| Tracking | `priority_processed` sequence numbers increase by one per invoice. |
| Queries | `sort_invoices_by_priority` with 51 ids panics; with 50 it succeeds. |

## Open questions

- Should priority raise the fee? A paid `Critical` tier would discourage everyone from marking their
  invoices urgent.
- Should a priority expire, for example falling one tier after N ledgers, so old `High` invoices do
  not stay there forever?
- Should the queue limit be per creator as well as per tier, so one creator cannot fill a tier?
