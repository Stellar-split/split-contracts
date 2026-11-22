# Issue #967: Consolidating storage key construction (plan)

**Status:** plan only. Key construction code is unchanged.

## Current state

`contracts/split/src/storage_keys.rs` holds the key families, and
[`STORAGE_KEY_REGISTRY.md`](../STORAGE_KEY_REGISTRY.md) documents the byte layout and tier choice.
Calls that build keys still appear in more than one module, so the same prefix and discriminant
pattern is repeated in places outside `storage_keys.rs`.

## Patterns to consolidate

1. **Prefix plus discriminant plus fields.** The layout is `prefix (8 bytes) || discriminant (u32 BE)
   || fields`. Each family builds it the same way.
2. **Fixed-size buffers.** Composite keys are written into a stack `[u8; N]` buffer, converted once.
3. **Tier selection.** Choosing instance, persistent or temporary for a key is repeated at each call
   site instead of being decided by the key type.

## Proposed utilities

- One `fn build_key<const N: usize>(prefix: Symbol, disc: u32, fields: &[u8]) -> Bytes` in
  `storage_keys.rs`, used by every family.
- One accessor per family that returns the key and its tier, so a call site cannot pick the wrong tier.

## Steps

1. Find every key construction outside `storage_keys.rs` (a search for the prefix constants).
2. Replace them with the accessor, one family at a time.
3. Confirm the bytes are identical. A snapshot of a few keys before and after is enough evidence.

## Not done

No search performed on the current tree yet, no utilities written, no snapshot check.
