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

## Questions?

Open a [Discussion](../../discussions) or ask in the issue thread.
