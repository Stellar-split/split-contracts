# Issue #1114: Type safety improvements (status)

**Status:** the repository has no `unsafe` code to remove. This note records that audit, the runtime
checks that guard the types, and the invariants each type is expected to keep. The newtype work in
[`ISSUE_968_TYPE_SAFETY.md`](./ISSUE_968_TYPE_SAFETY.md) is still a plan.

## Unsafe code audit

A search of every `.rs` file in the repository (contracts, `tests/`, `fuzz/`) finds **no** `unsafe`
block, function or impl. The contracts are `#![no_std]` and use only safe Rust, so "remove unsafe blocks"
has nothing to remove.

The nearest risks are not `unsafe`, but code that can panic or silently narrow a value. Counts from a text
search, not a review of each site:

| File | `.unwrap()` | `.expect(...)` | numeric `as` casts |
|---|---|---|---|
| `contracts/split/src/lib.rs` | 106 | 170 | 184 |
| `contracts/split/src/calc.rs` | 15 | 1 | 8 |
| `contracts/split/src/validation.rs` | 7 | 0 | 7 |
| `contracts/split/src/types.rs` | 0 | 4 | 3 |

`CONTRIBUTING.md` asks for no `unwrap()` in production paths. Some of the `.unwrap()` calls in `lib.rs`
follow a check that makes them safe, for example `get(i).unwrap()` inside a loop bounded by `len()`.
Telling the safe ones from the rest needs a site-by-site review, which was not done here.

## Runtime checks that exist

- `overflow-checks = true` in the workspace release profile, so an unchecked `+` or `*` that overflows
  aborts instead of wrapping.
- `validation::checked_sum_amounts`, `checked_add_amount` and `validate_invoice_total` return
  `ContractError::ArithmeticOverflow` instead of overflowing, and cap an invoice total at
  `MAX_INVOICE_TOTAL`.
- `validation::validate_recipients_and_amounts`, `validate_deadline`, `assert_unique_recipients`,
  `assert_valid_bps`, `assert_bps_total`, `validate_fee_rate` and `validate_split_rules` check inputs at the
  entry points before anything is written.
- `#[contracterror]` enums give typed failures, see [`ERROR_CODES_SPLIT.md`](./ERROR_CODES_SPLIT.md).

## Type invariants

These are the rules the code enforces today. Where a value is a plain integer, the rule is held by the
checks above, not by the type.

| Value | Type | Invariant | Held by |
|---|---|---|---|
| Invoice id | `u64` | Starts at 1 and increases by one per invoice. | The invoice counter. |
| Amount | `i128` | Strictly positive per recipient. The invoice total is at most `MAX_INVOICE_TOTAL` (`i128::MAX / 10^10`). | `validate_recipients_and_amounts` |
| Recipients and amounts | `Vec<Address>`, `Vec<i128>` | Same length, at least one, at most `MAX_INVOICE_RECIPIENTS` (200), no duplicate address. | `validate_recipients_and_amounts`, `assert_unique_recipients` |
| Deadline | `u64` | After the current ledger time, and at most `MAX_DEADLINE_HORIZON_SECS` (315,360,000, ten years) ahead. | `validate_deadline` |
| Basis points | `u32` | A single rate is at most `10_000`. A set of ratios sums to exactly `10_000`. | `assert_valid_bps`, `assert_bps_total` |
| Platform fee rate | `u32` | At most `MAX_PLATFORM_FEE_BPS` (1,000, ten percent). | `validate_fee_rate` |
| Split rules | `Vec<SplitRule>` | One rule per recipient, between `MIN_SPLIT_RULES` and `MAX_SPLIT_RULES` (32), percentage rules at most 10,000 bps together. | `validate_split_rules` |
| Error code | `ContractError` (`u32`) | The numbers 1 to 82, each used once, never reordered. | The enum and its doc comment |
| Invoice records | `#[contracttype]` structs | At most 40 fields each. A field added to a stored struct needs a schema migration. | `CONTRIBUTING.md`, `migrations.rs` |

## What is not done

- No newtypes (`InvoiceId`, `Stroops`, `Bps`). They change the encoding of stored types and the generated
  client, so [`ISSUE_968_TYPE_SAFETY.md`](./ISSUE_968_TYPE_SAFETY.md) proposes applying them to new fields first.
- No review of the individual `unwrap` and `as` sites.
- No change to code, and no tests were added. The counts and rules were read from the source, not run.
