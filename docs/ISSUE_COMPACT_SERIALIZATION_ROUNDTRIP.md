# Issue: Compact Serialization Round-Trip Tests for `to_compact` / `from_compact`

## Overview

The `Invoice::to_compact` and `Invoice::from_compact` methods pack three
critical invoice fields — `status`, `funded`, and `deadline` — into a raw
`Bytes` blob for compact on-chain storage. No tests currently verify that
these values survive the encode/decode cycle intact. A silent mismatch in
byte offsets, endianness, or discriminant mappings would corrupt live invoice
state without any observable failure at the call site.

## Background

`to_compact` encodes the three fields sequentially:

| Offset | Length | Field      | Type   |
|--------|--------|------------|--------|
| 0      | 1 byte | `status`   | `u8`   |
| 1      | 16 bytes | `funded` | `i128` big-endian |
| 17     | 8 bytes  | `deadline` | `u64` big-endian |

Total: 25 bytes minimum.

`from_compact` reads these offsets back in the same order, then calls
`Invoice::assemble` and overwrites the three fields with the decoded values.
Any discrepancy between the encoding and decoding offsets would silently
restore wrong values — for example, treating part of the `funded` bytes as
the `deadline`, or mapping the wrong discriminant to an `InvoiceStatus`
variant.

The existing `InvoiceStatus::to_u8` / `from_u8` helpers are already
covered by `invoice_status_round_trip_all_variants` and
`invoice_status_discriminants_are_unique` in `types.rs`. What is missing
are integration-level tests that exercise `to_compact` → `from_compact` as a
unit and confirm all three fields come back unchanged.

## Acceptance Criteria

- A test constructs an `Invoice` with **known** `status`, `funded`, and
  `deadline` values.
- The test calls `to_compact` and then `from_compact` and asserts that each
  of the three fields is **bit-for-bit identical** to the original.
- The test suite covers **at least three distinct `InvoiceStatus` variants**
  to exercise different discriminant bytes.
- `cargo test` passes without any new compilation errors or test failures.

## Proposed Tests

The tests belong in `contracts/split/src/types.rs` inside the existing
`#[cfg(test)] mod tests` block, alongside the current
`invoice_status_round_trip_all_variants` test.

Because `to_compact` / `from_compact` accept a Soroban `&Env`, the tests
must use `soroban_sdk::Env::default()`.  The `Invoice::assemble` helper
(which `from_compact` calls internally) needs stub `InvoiceCore`,
`InvoiceExt`, and `InvoiceExt2` values; the helper methods
`InvoiceExt::default(env)` and `InvoiceExt2::default(env)` already exist for
exactly this purpose.

### Test 1 — `compact_round_trip_pending_status`

```rust
#[test]
fn compact_round_trip_pending_status() {
    let env = Env::default();

    // Build a minimal InvoiceCore with known status/funded/deadline.
    let core = make_stub_core(&env, InvoiceStatus::Pending, 0_i128, 1_000_u64);
    let ext  = InvoiceExt::default(&env);
    let ext2 = InvoiceExt2::default(&env);

    let invoice  = Invoice::assemble(core, ext, ext2);
    let compact  = invoice.to_compact(&env);

    // Provide fresh stubs so from_compact takes status/funded/deadline from
    // the compact blob, not from the stubs.
    let core2 = make_stub_core(&env, InvoiceStatus::Released, 999_i128, 999_u64);
    let ext2b = InvoiceExt::default(&env);
    let ext2c = InvoiceExt2::default(&env);

    let restored = Invoice::from_compact(&compact, core2, ext2b, ext2c);

    assert_eq!(restored.status,   InvoiceStatus::Pending);
    assert_eq!(restored.funded,   0_i128);
    assert_eq!(restored.deadline, 1_000_u64);
}
```

### Test 2 — `compact_round_trip_released_status_nonzero_funded`

```rust
#[test]
fn compact_round_trip_released_status_nonzero_funded() {
    let env = Env::default();

    let funded   = 5_000_000_i128;
    let deadline = 9_999_999_999_u64;

    let core = make_stub_core(&env, InvoiceStatus::Released, funded, deadline);
    let invoice  = Invoice::assemble(core, InvoiceExt::default(&env), InvoiceExt2::default(&env));
    let compact  = invoice.to_compact(&env);

    let restored = Invoice::from_compact(
        &compact,
        make_stub_core(&env, InvoiceStatus::Pending, 0, 0),
        InvoiceExt::default(&env),
        InvoiceExt2::default(&env),
    );

    assert_eq!(restored.status,   InvoiceStatus::Released);
    assert_eq!(restored.funded,   funded);
    assert_eq!(restored.deadline, deadline);
}
```

### Test 3 — `compact_round_trip_expired_status_max_values`

```rust
#[test]
fn compact_round_trip_expired_status_max_values() {
    let env = Env::default();

    let funded   = i128::MAX;
    let deadline = u64::MAX;

    let core = make_stub_core(&env, InvoiceStatus::Expired, funded, deadline);
    let invoice  = Invoice::assemble(core, InvoiceExt::default(&env), InvoiceExt2::default(&env));
    let compact  = invoice.to_compact(&env);

    let restored = Invoice::from_compact(
        &compact,
        make_stub_core(&env, InvoiceStatus::Pending, 0, 0),
        InvoiceExt::default(&env),
        InvoiceExt2::default(&env),
    );

    assert_eq!(restored.status,   InvoiceStatus::Expired);
    assert_eq!(restored.funded,   i128::MAX);
    assert_eq!(restored.deadline, u64::MAX);
}
```

### Test 4 — `compact_round_trip_negative_funded`

```rust
#[test]
fn compact_round_trip_negative_funded() {
    let env = Env::default();

    // Negative funded is unusual but i128 permits it; the codec must not
    // corrupt the sign bit.
    let funded   = -1_i128;
    let deadline = 42_u64;

    let core = make_stub_core(&env, InvoiceStatus::Refunded, funded, deadline);
    let invoice  = Invoice::assemble(core, InvoiceExt::default(&env), InvoiceExt2::default(&env));
    let compact  = invoice.to_compact(&env);

    let restored = Invoice::from_compact(
        &compact,
        make_stub_core(&env, InvoiceStatus::Pending, 0, 0),
        InvoiceExt::default(&env),
        InvoiceExt2::default(&env),
    );

    assert_eq!(restored.status,   InvoiceStatus::Refunded);
    assert_eq!(restored.funded,   -1_i128);
    assert_eq!(restored.deadline, 42_u64);
}
```

### Helper — `make_stub_core`

A private helper that constructs a minimal `InvoiceCore` with the given
lifecycle fields and dummy values for everything else.  Add it inside the
`mod tests` block:

```rust
#[cfg(test)]
fn make_stub_core(
    env: &Env,
    status: InvoiceStatus,
    funded: i128,
    deadline: u64,
) -> InvoiceCore {
    use soroban_sdk::Address;
    let dummy_addr = Address::generate(env);
    InvoiceCore {
        version:             1,
        creator:             dummy_addr.clone(),
        co_creators:         soroban_sdk::Vec::new(env),
        recipients:          soroban_sdk::Vec::new(env),
        amounts:             soroban_sdk::Vec::new(env),
        tokens:              soroban_sdk::Vec::new(env),
        funding_token:       dummy_addr,
        deadline,
        funded,
        status,
        payments:            soroban_sdk::Vec::new(env),
        drip_duration:       None,
        release_timestamp:   None,
        claimed:             soroban_sdk::Vec::new(env),
        frozen:              false,
        completion_time:     None,
        allow_early_withdrawal: false,
        bonus_pool:          0,
        bonus_max_payers:    0,
        prerequisite_id:     None,
        tranches:            soroban_sdk::Vec::new(env),
        released_bps:        0,
        clone_depth:         0,
        predecessor_id:      None,
        metadata_hash:       None,
    }
}
```

## Risk

| Severity | Area |
|----------|------|
| High | Silent data corruption if byte offsets diverge between `to_compact` and `from_compact` |
| Medium | `InvoiceStatus::PayoutInProgress` (discriminant 9) is handled by `to_u8`/`from_u8` but **not** by the `match` arms inside `to_compact` / `from_compact` — those arms omit the variant and fall back to `Pending` on decode. The round-trip tests will immediately surface this gap. |

## Implementation Notes

- `to_compact` uses a `match` that currently omits `InvoiceStatus::PayoutInProgress`.
  Adding a test for that variant will reveal the silent fallback and prompt
  the implementer to add the missing arm.
- The byte layout (1 + 16 + 8 = 25 bytes) is validated by the `bytes.len() < 25`
  guard in `from_compact`; tests with intentionally short blobs would also be
  a useful addition but are out of scope for this issue.
- All tests are pure unit tests — no contract deployment, no XDR serialisation
  round-trip, no network access required.

## Files to Modify

| File | Change |
|------|--------|
| `contracts/split/src/types.rs` | Add `make_stub_core` helper and four `compact_round_trip_*` tests inside the existing `#[cfg(test)] mod tests` block |
