# Issue #995: Escrow with conditional release triggers (status)

**Status:** the conditional release triggers exist in `contracts/invoice-escrow/src/lib.rs`:

- `set_oracle_condition` and `clear_oracle_condition`, with `check_oracle_condition` for a reading.
- `schedule_release` and `cancel_release_schedule`, which release at a time, and after a minimum
  number of ledgers.

Their error codes are documented in [`ERROR_CODES.md`](./ERROR_CODES.md) (codes 28 to 37).

## What this change does

Records the status above. It changes no code and adds no tests.

## What still needs checking

- Whether a release needs every configured trigger to hold, or any one of them.
- That a release made by a trigger still runs the co-signer approval when co-signers are configured.
- Whether the scheduled release and the oracle condition can be set together, and which one wins.

## Not done

No review of the trigger logic, no new tests.
