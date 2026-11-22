# Issue #968: Type safety with newtypes (plan)

**Status:** plan only. No newtypes are defined yet.

## Problem

Invoice IDs, amounts and fee rates are all plain integers (`u64`, `i128`, `u32`). The compiler cannot
catch a call that passes a fee rate where an amount belongs, or an invoice ID where a deadline belongs.

## Proposed newtypes

```rust
#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct InvoiceId(pub u64);

#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Stroops(pub i128);

#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Bps(pub u32); // 0..=10_000, checked on construction
```

`Bps::new` returns an error above 10 000. `Stroops` is non-negative in the places that need it.

## Compatibility

- `#[contracttype]` changes the XDR encoding of any struct or enum that stores these types. Changing
  a stored field's type is a storage migration, so apply the newtypes to new fields first, and leave
  stored fields as plain integers until a migration is written.
- Public function signatures would change, which breaks the generated client. Keep the integer
  signatures and convert inside, or accept a version bump.

## Open questions

- Whether `Stroops` should reject negative values at construction, or only at the boundary.
- Whether the change should wait for the module split in #966, to avoid two large diffs at once.

## Not done

No types defined, no call sites changed, no migration written.
