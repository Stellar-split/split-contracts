# Issue #1106: Custom error types (status)

**Status:** covered for the typed errors. Every contract crate defines an error enum, and the `split`
contract's 82 codes are consistent and unique. The remaining gap is code that still fails with a plain
panic message.

## Requirements against the code

| Requirement | Where it is |
|---|---|
| Error types | `ContractError` in `contracts/split/src/error.rs`, a `#[contracterror]` enum with `#[repr(u32)]` and an explicit discriminant on every variant. The other crates have their own enum in `errors.rs`: `invoice-escrow` (47 codes), `charity-split` (22), `mentorship` (21), `skill-verification` (15) and `tax-strategy` (13). |
| Type system | `#[contracterror]` derives the conversion to and from the Soroban `Error` value, so a failure reaches a client as `Error(Contract, #code)`. The enum is `Copy`, `Clone`, `Debug`, `Eq` and `PartialEq`. |
| Type checking | Entry points that return `Result<_, ContractError>` and `panic_with_error!` take a `ContractError`, so the compiler rejects a code that is not in the enum. |
| Serialization | Provided by `#[contracterror]`. The code is the number in the transaction result. |
| Error catalog | [`ERROR_CODES_SPLIT.md`](./ERROR_CODES_SPLIT.md) for `split`, and [`ERROR_CODES.md`](./ERROR_CODES.md) for `invoice-escrow`. The other three crates have no catalog yet. |
| No type conflicts | The `split` codes are the numbers 1 to 82, each used once. Code 51 is declared after code 56 in the source, so reading the enum top to bottom does not show it in order. The rule in the doc comment is that numbers are stable and new codes are only appended. |

## Gaps

- **Plain panics.** In `contracts/split/src/lib.rs` there are 20 uses of `panic_with_error!` against
  38 `panic!` calls, 486 `assert!` calls and 170 `.expect(...)` calls. Those fail with a message and no
  code, so a client cannot match them by number. Converting them is a wide change that alters what
  existing tests expect, so it is not done here.
- **Overlapping variants.** Some pairs describe nearly the same case, for example `InvalidAmount` (5)
  and `ZeroAmountNotAllowed` (41), or `NotAuthorized` (1) and `RoleNotHeld` (39). They are stable
  numbers now, so they stay.
- **Catalogs for `charity-split`, `mentorship`, `skill-verification` and `tax-strategy`** are not written.

## What this change does

Records the status above. It changes no code. The behaviour was read, not run, and no tests were added.
