# Contributing to split-contracts

Thank you for your interest in contributing to StellarSplit! This repo is part of the [Drips Wave Program](https://drips.network/wave) — a monthly open-source bounty program run by the Stellar Development Foundation.

## Before You Start

**Do not begin coding until you have been assigned to an issue by a maintainer.**

1. Browse [open issues](../../issues) and find one labelled `good first issue` or matching your skill level.
2. Comment on the issue: "I'd like to work on this."
3. Wait for a maintainer to assign you. Only then should you fork and start coding.

## Workflow

### 1. Fork & Clone

```bash
git clone https://github.com/<your-username>/split-contracts.git
cd split-contracts
```

### 2. Create a Branch

Branch names must follow this pattern:

```
fix/issue-NUMBER-short-description
feat/issue-NUMBER-short-description
```

Examples:
- `fix/issue-3-refund-edge-case`
- `feat/issue-7-add-partial-release`

```bash
git checkout -b fix/issue-42-short-description
```

### 3. Make Your Changes

- Write clean, well-commented Rust code.
- Add or update tests in `contracts/split/src/test.rs`.
- Run `cargo test --workspace` and ensure all tests pass.
- Run `cargo clippy` and fix any warnings.
- Run `cargo fmt` to format your code.

### 4. Commit

Use conventional commits:

```
fix: correct refund logic when deadline is exact ledger timestamp (#42)
feat: add partial release function (#7)
```

### 5. Open a Pull Request

- Title: concise, under 70 characters.
- Description: what changed, why, and how you tested it.
- Reference the issue: `Closes #42`
- Do not open a PR without a linked issue.

## Code Standards

- All public functions must have Rust doc comments (`///`).
- No `unwrap()` in production code paths — use `expect("descriptive message")` or proper error handling.
- Keep functions small and focused.

## Adding a new `ContractError` variant

`ContractError` (`contracts/split/src/error.rs`) is a `#[repr(u32)]` enum with an explicit
discriminant on every variant. Its doc comment states the rule: **discriminants are stable —
never reorder, only append.** Soroban clients and indexers match on the numeric error code, so
changing an existing variant's number (or reusing a retired one) is a breaking change even
though the Rust source still compiles.

When you need a new error case:

1. **Never reorder or renumber existing variants.** Do not "tidy up" the list, fill gaps, or
   resequence numbers to keep them contiguous — gaps (e.g. `50`, `52` with `51` used elsewhere)
   are expected and are not bugs to fix.
2. **Append your variant at the end of the enum**, with the next unused discriminant. Find the
   current highest number in the file and add one to it — do not reuse a number that is skipped
   earlier in the list.
3. **Document it.** Add a `///` doc comment above the variant explaining when it is returned,
   and reference the issue number that introduced it (the existing variants follow an
   `/// Issue #NNN: ...` convention).
4. **Update any call sites** that need to return the new error, and add/extend tests in
   `contracts/split/src/test.rs` covering the new failure path.

### Before

```rust
    /// Issue #522: Parent chain depth exceeds the allowed maximum.
    ParentChainTooDeep = 63,
}
```

### After

```rust
    /// Issue #522: Parent chain depth exceeds the allowed maximum.
    ParentChainTooDeep = 63,
    /// Issue #611: Payout schedule references a milestone that does not exist.
    MilestoneNotFound = 64,
}
```

Note that the new variant is appended after the last existing one with the next free
discriminant (`64`); none of the earlier numbers are touched.

## Adding a new invoice option field

`create_invoice` takes its optional parameters through two parameter groups,
[`InvoiceOptions`](contracts/split/src/types.rs) and
[`InvoiceOptions2`](contracts/split/src/types.rs), instead of a flat argument
list, so the function stays within Soroban's 10-parameter limit. Those two
structs are also the public surface that callers fill in, but the values that
must survive on chain are fanned out into the persistent invoice structs
([`InvoiceCore`](contracts/split/src/types.rs),
[`InvoiceExt`](contracts/split/src/types.rs),
[`InvoiceExt2`](contracts/split/src/types.rs), and
[`InvoiceExt3`](contracts/split/src/types.rs)).

### The 40-field `#[contracttype]` constraint

Soroban `#[contracttype]` structs are capped at **40 fields**. `InvoiceOptions`
is intentionally kept at (or very near) that ceiling, and the overflow bucket
`InvoiceOptions2` exists so newly-added options do **not** break the limit.

- Put a new option in **`InvoiceOptions`** only if it currently has fewer than
  40 fields.
- If `InvoiceOptions` is already at 40 fields, add the field to
  **`InvoiceOptions2`** instead (and point the doc comment at the issue, e.g.
  `/// Issue #NNN: ...`). Never reorder or delete existing fields to "make
  room" — field order and offsets are part of the on-chain XDR layout.

### The `InvoiceCore` / `InvoiceExt` / `InvoiceExt2` storage split

Persistent invoice state is sharded on purpose so that hot-path reads stay
small and so that new fields can be added without disturbing the core layout:

| Struct | Holds | When to add your field here |
|--------|-------|------------------------------|
| `InvoiceCore` | Always-present, frequently-read invoice facts (creator, recipients, amounts, status, funding). | Only for data every invoice carries and that the hot path reads. Rarely the right place for an *optional* new option. |
| `InvoiceExt` | The bulk of optional/extension fields (co-signers, penalties, tax, routing, velocity, etc.). | The default home for a new optional behavior flag or value. |
| `InvoiceExt2` | Overflow extension state (notifications, disputes, auctions, oracle pricing, KYC, escrow). | When `InvoiceExt` is near its ceiling, or the field is logically grouped with dispute/auction/oracle state. |
| `InvoiceExt3` | Newer extension bucket for recently added fields. | When both `InvoiceExt` and `InvoiceExt2` are full. |

Rule of thumb: an option that is *optional* and only used by some invoices
belongs in `InvoiceExt`/`InvoiceExt2`/`InvoiceExt3`, **not** `InvoiceCore`.

### End-to-end checklist for adding a new field

1. **Add the field to the input struct.** Decide `InvoiceOptions` vs
   `InvoiceOptions2` using the 40-field rule above. Add a `/// Issue #NNN:`
   doc comment describing the field.
2. **Add the matching persisted field** to the correct storage struct
   (`InvoiceExt`, `InvoiceExt2`, or `InvoiceExt3`) so the value is actually
   stored on chain. Keep the field name consistent with the input struct.
3. **Wire the copy in `create_invoice`.** Find where the other `InvoiceOptions`
   fields are mapped into `InvoiceExt`/`InvoiceExt2` and add the assignment
   (e.g. `ext.my_field = options.my_field;`). Also update `InvoiceExt::default`
   (and any other default constructors) so the new field is initialised to its
   zero/empty/`None` default and is never accidentally omitted.
4. **Thread it through reads/updates.** If the field can change after creation
   (e.g. via an `update_*` or `set_*` entry point), update the corresponding
   getter/setter and any merge logic so the new value is round-tripped.
5. **Update `STORAGE_KEY_REGISTRY.md`** if your change introduces a new storage
   key (most option fields reuse the existing per-invoice key, so this is only
   needed for genuinely new keys).
6. **Storage schema change → migration entry.** Because you changed the shape
   of an on-chain `#[contracttype]` struct, this is a **storage schema change**.
   Bump `CURRENT_SCHEMA_VERSION` in
   [`migrations.rs`](contracts/split/src/migrations.rs), add a `migration_vN`
   function that backfills a sensible default for every invoice already stored
   on chain, and wire it into `run_pending_migrations` (see the existing
   `v1 -> v2` / `v2 -> v3` examples in that file). Add a migration note to your
   PR description. Skipping this step leaves already-deployed contracts on a
   stale schema, and every entry point will panic with `MigrationRequired`
   until `migrate` is called.
7. **Tests.** Add/extend tests in `contracts/split/src/test.rs` covering: the
   field is accepted at creation, persisted, round-trips through any update
   path, and that the schema migration backfills a correct default for
   pre-existing invoices.
8. **Docs.** If the behavior is user-visible, mention it in `README.md` and/or
   the relevant `docs/` page.

### Before

```rust
// InvoiceOptions2 is at the 40-field ceiling, so a new flag goes here:
pub struct InvoiceOptions2 {
    // ...existing fields...
    /// Issue #416: SHA-256 hash of the required off-chain release preimage.
    pub release_condition_hash: Option<BytesN<32>>,
}
```

### After

```rust
pub struct InvoiceOptions2 {
    // ...existing fields...
    /// Issue #416: SHA-256 hash of the required off-chain release preimage.
    pub release_condition_hash: Option<BytesN<32>>,
    /// Issue #703: opt-in flag enabling per-payer receipt minting on release.
    pub mint_receipts: Option<bool>,
}
```

The same field is then added to `InvoiceExt2` (or `InvoiceExt3`), copied in
`create_invoice`, defaulted in `InvoiceExt2::default`, and covered by a schema
migration + tests.

## Questions?

Open a [Discussion](../../discussions) or ask in the issue thread.
