# ADR 0004: Split invoice records across several keys, with a hot-path cache

- **Status:** accepted (describes the current data model)
- **Context:** An invoice carries many optional features: co-signers, oracles, tranches, split rules,
  priorities and more. Storing it as one entry would exceed Soroban's per-entry size limit and make
  every `pay()` read and rewrite the whole record.

## Decision

- Store an invoice as `InvoiceCore` plus extension records (`InvoiceExt`, `InvoiceExt2`, …) under the
  same id. `get_invoice` reads them together.
- Keep a small `InvoiceHot` record (`status`, `funded`, `total`, `recipients`) that the payment fast
  path reads and writes.
- Store payments in `SHARD_COUNT` shards per invoice so no single entry grows without bound.
- For invoices with many recipients, keep per-field lists (recipients, amounts, paid flags).

## Consequences

- Payments touch small entries, which keeps fees and instruction counts down.
- Adding an optional feature means adding fields to an extension record, not to the core.
- Readers must go through the getters so the parts stay consistent. The storage key snapshot test
  catches accidental key changes.
- Refunds aggregate across shards, so their cost grows with the number of shards.

## Alternatives considered

- **One record per invoice.** Simple, but hits the size limit and rewrites everything on every payment.
- **One key per field everywhere.** Smallest writes, but many reads to rebuild an invoice.
