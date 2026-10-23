# ADR 0003: Record usage analytics from the lifecycle event helpers

- **Status:** accepted (issue #1034, extended by issue #1033)
- **Context:** Usage analytics need to see every invoice creation, payment, release and refund. These
  happen on many code paths (plain, batched, on-behalf-of, converted and streamed payments; several
  release and refund variants), and new paths keep being added.

## Decision

- Call `analytics::record` from the event helpers in `events.rs` (`invoice_created`,
  `payment_received`, `invoice_released`, `invoice_refunded`, `invoice_refunded_with_reason`), not from
  each entry point.
- Store aggregates on-chain: lifetime totals in instance storage and one bucket per UTC day in
  persistent storage.
- Recording must never publish events and must never fail: counters saturate.
- Monitoring (`monitoring::on_usage`) runs at the end of `analytics::record`, so alert rules see the
  updated aggregates.

## Consequences

- Any path that emits a lifecycle event is counted, including future ones, with no extra code.
- The event stream that indexers and tests depend on is unchanged.
- Each tracked event costs one instance write and one persistent write, plus one persistent write the
  first time an address is seen. The admin can switch recording off.
- Analytics depend on the event helpers being called. A path that changes state without emitting the
  event is not counted, which would also be an event bug.

## Alternatives considered

- **Off-chain only (indexer).** No on-chain cost, but contracts and keepers cannot read the numbers,
  and alert rules could not run on-chain.
- **Hooks at every entry point.** Explicit, but easy to miss on new paths and repeated in many places.
