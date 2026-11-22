# Issue #960: Debug mode logging (design scaffold)

**Status:** design only. No logging is added.

## Goal

Optional verbose output for troubleshooting, with log levels and filters, and no cost when it is
off.

## Constraints

- Soroban contracts have no runtime log levels. Diagnostic output in tests uses the SDK's testing
  facilities; production output is events and return values.
- Any logging path in production adds to the instruction budget, even when unused, so the default
  must compile it out.

## Proposed approach

1. A Cargo feature, `debug-log`, off by default. Each log site is behind `#[cfg(feature = "debug-log")]`.
2. Levels as a small enum (`Error`, `Warn`, `Info`, `Debug`), compared against a level constant set
   at build time. A runtime level would cost storage reads on every call, so it is avoided.
3. Filters by module name, also build-time.
4. Log lines are formatted only inside the feature gate, so release builds carry no formatting code.

## Open questions

- Whether `debug-log` should be enabled in the testnet build only.
- How output reaches a developer: test output, or events with a debug topic.

## Not done

No feature, no macros, no tests. The performance claim ("no perf impact") can only be checked
after an implementation exists, by comparing budgets with the feature off.
