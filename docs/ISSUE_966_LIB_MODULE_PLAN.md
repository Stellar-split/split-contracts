# Issue #966: Splitting `lib.rs` into modules (plan)

**Status:** plan only. No code has been moved.

## Current state

`contracts/split/src/lib.rs` is about 19,000 lines, and most of the contract's functions are still
inside it. Several areas already live in their own modules: `calc`, `events`, `validation`,
`storage`, `storage_keys`, `insurance_pool`, `sustainability`, `cooperative`, `velocity`,
`split_rules`, `payment_tracking`, `security_audit`, `webhooks`, `priority`. The `lib.rs` entry points
are the parts still to be grouped.

## Target layout

| Module | Contents |
|---|---|
| `admin` | initialise, admin transfer, pause and config setters |
| `invoice` | create, update metadata, cancel, clone |
| `payment` | deposit, batch, release, refund, reversal |
| `fees` | waivers, creator tiers, fee queries (logic already in `calc`) |
| `queries` | read-only getters and list functions |

Each module exposes functions that take `Env` and return `Result<_, ContractError>`, as today.
`lib.rs` keeps the `#[contractimpl]` block and delegates each method to the module, so the
contract's public interface does not change.

## Steps

1. Move one group at a time, starting with `queries`, since it has no writes.
2. After each move, the public method signatures must be byte-identical.
3. Keep the tests in `test.rs` unchanged. A move is complete when they pass without edits.

## Risks

- `#[contractimpl]` requires every public function in one impl block, so delegation adds a line per
  method. Check that the generated client keeps its method names.
- Moving code can change the order of storage reads, which changes the instruction budget. Compare
  the budget before and after each step.

## Not done

No module moved, no test run, no budget comparison.
