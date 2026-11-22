# ADR 0001: Storage tiers by access pattern

- **Status:** accepted (describes the current policy)
- **Context:** The contracts store many keys. Their tier (instance, persistent, temporary) decides
  how long they live, what they cost, and whether they survive an upgrade.

## Decision

Tier placement follows how a key is used, not which entity it belongs to:

- **instance** for values read on nearly every call and written rarely: config, counters, flags, and
  hot per-invoice fields that the common read path needs.
- **persistent** for authoritative per-entity state that must outlive archival and upgrades.
- **temporary** for data that can be rebuilt from the authoritative record. Temporary entries are never
  the only copy of anything.

The full key list and the tier table are in [`STORAGE_KEY_REGISTRY.md`](../../STORAGE_KEY_REGISTRY.md).

## Consequences

- Reads on the hot path avoid a persistent lookup, which keeps the instruction budget down.
- Instance storage is wiped on upgrade unless preserved, so any singleton that must survive an
  upgrade needs an explicit migration step.
- Temporary entries can be evicted at any time. Code that reads them must handle absence by rebuilding
  from the authoritative record.

## Alternatives considered

- **One tier for everything (persistent).** Simple, but pays archival and rent costs on hot singletons
  that do not need them.
- **Grouping keys by entity.** Keeps an invoice's keys together, but puts hot and cold data in the
  same tier, which the access-frequency rule avoids.
