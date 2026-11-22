# ADR 0002: One `split` contract, entry points in `lib.rs`, logic in modules

- **Status:** accepted (describes the current structure)
- **Context:** The protocol has grown to hundreds of entry points: invoices, payments, disputes,
  fees, governance and observability. They share invoice state, the admin, pause and circuit-breaker
  flags, and the token configuration.

## Decision

- Keep the core protocol in a single `split` contract instead of one contract per feature.
- Declare every public entry point in `lib.rs`, grouped into `#[contractimpl]` blocks by feature and
  issue number. Each entry point does its authorisation check there.
- Put logic that can stand alone (validation, rules, velocity, analytics, monitoring, …) in a module
  that takes `&Env` and owns its storage keys. The module does not check auth; the entry point does.
- Put independent products (`charity-split`, `tax-strategy`, `mentorship`, …) in separate crates.

## Consequences

- Features share state without cross-contract calls, which keeps instruction and fee costs down.
- Auth is checked in one visible place per entry point.
- Modules are easy to unit test with `env.as_contract`.
- `lib.rs` is very large (about 19,000 lines). [`ISSUE_966_LIB_MODULE_PLAN.md`](../ISSUE_966_LIB_MODULE_PLAN.md)
  tracks moving more code into modules.
- The WASM size is bounded by one contract, so new features must watch binary size.

## Alternatives considered

- **One contract per feature.** Smaller binaries, but every shared read becomes a cross-contract
  call, and invoice state would need a separate owner contract.
- **Entry points inside modules.** Soroban allows several `#[contractimpl]` blocks, but scattering
  them makes the public surface and its auth checks harder to review.
