# Storage key naming conventions

This guide sets out how to name and build storage keys in the StellarSplit contracts. The full list
of keys is in [`STORAGE_KEY_REGISTRY.md`](../STORAGE_KEY_REGISTRY.md). This page explains the rules
behind it, so new keys follow the same pattern.

## Rules

1. **Key types are `#[contracttype]` enums.** One enum per key family, such as an invoice family or
   an address family. The variant name says what the key holds (`Record`, `Funded`).
2. **Unique discriminants.** Each variant is a distinct XDR tag, so keys from different families
   never collide, even when they hold the same fields.
3. **Fields are the identity.** Put only the fields that identify the entry in the key. Do not
   include values that change, such as a balance, or two writes would address two entries.
4. **One prefix per family.** The prefix is a `const` `Symbol` made with `symbol_short!`, so it is
   not allocated per call.
5. **Stable layout.** Once a key ships, its byte layout does not change. Add a new variant instead.

## Template

```rust
#[contracttype]
#[derive(Clone)]
pub enum InvoiceKey {
    /// Invoice record, keyed by ID.
    Record(u64),
    /// Total funded for an invoice.
    Funded(u64),
}
```

## Tier choice

Pick the tier by how the data is used (see the tier policy in `STORAGE_KEY_REGISTRY.md`):

- **instance**: singletons and flags read on almost every call.
- **persistent**: authoritative per-entity state that must survive archival.
- **temporary**: derived or rebuildable data. Never the only copy of anything.

## Examples

| Key | Tier | Why |
|---|---|---|
| `InvoiceKey::Record(id)` | persistent | Authoritative invoice state |
| `InvoiceKey::Funded(id)` | persistent | Authoritative, must match the record |
| admin address | instance | Read on most admin calls, rarely written |

## Checklist for a new key

- Is it a new family, or a new variant in an existing one?
- Are the identifying fields the only fields in the key?
- Is the tier chosen from the access pattern?
- Is the new variant added to `STORAGE_KEY_REGISTRY.md`?
