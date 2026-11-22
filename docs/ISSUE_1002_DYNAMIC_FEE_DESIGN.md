# Issue #1002: Dynamic fee calculation (design scaffold)

**Status:** design only. The fee is still the fixed-rate formula in [`FEE_FLOW.md`](./FEE_FLOW.md).

## Goal

Let the fee rate depend on the invoice, for example on its size, the creator's tier, or how long the
invoice runs, instead of one rate for every invoice.

## Relevant existing code

- The fixed rate is applied by `calc_platform_fee(funded, fee_bps)` in `contracts/split/src/calc.rs`.
- Creator fee tiers are in `creator_fee_tiers.rs`, which already varies a rate by creator. The dynamic
  design should reuse that rather than add a second source of rates.

## Proposed approach

1. A rate function `fn effective_fee_bps(invoice, config) -> u32` that returns the rate for one
   invoice, computed from its fields and the config.
2. The result is clamped to the range 0 to 10 000, so a bad config cannot make the fee exceed the amount.
3. Each computed rate is recorded in the `fee_calculated` event, so a dashboard can show which rate
   applied.

## Constraints

- The rate must be fixed when the invoice is created, not recomputed at release. Otherwise a config
  change moves the fee on invoices already in progress.
- A zero rate is valid and must produce a zero fee, as `FEE_FLOW.md` already says.

## Open questions

- Which inputs drive the rate. Size and creator tier are the obvious two, but neither is chosen here.
- Who may change the config, and whether a change is logged.

## Not done

No code, no tests, no rate function.
