# Issue #673: Boundary Test Plan — FeeBracket Rate Selection

Test plan only — no test code. Written per explicit request; the acceptance
criteria below should be turned into `#[test]` functions in
`contracts/split/src/test.rs` by whoever picks this up.

## Code under test

- `FeeBracket { max_amount: i128, rate_bps: u32 }` — `contracts/split/src/types.rs:1601-1606`
- `SplitContract::set_fee_brackets` — `contracts/split/src/lib.rs:4470-4491`
- `SplitContract::compute_fee` — `contracts/split/src/lib.rs:4493-4536`

## How bracket selection actually works

`compute_fee` is **not** a "pick one bracket for the whole amount" lookup —
it's a marginal/progressive scheme, same shape as tax brackets. Brackets are
walked in order; each bracket taxes only the slice of `amount` that falls
between the previous bracket's `max_amount` and its own:

```
prev_max = 0
for each bracket b (in ascending max_amount order):
    slice_limit = b.max_amount - prev_max        // width of this bracket
    slice       = min(remaining, slice_limit)
    fee        += slice * b.rate_bps / 10_000
    remaining  -= slice
    prev_max    = b.max_amount
    stop when remaining <= 0
```

The boundary comparison is `if remaining > slice_limit` (line 4521) — strictly
greater, not `>=`. This is the exact comparison the issue is worried about, so
it's the one to pin down with tests.

`set_fee_brackets` (lib.rs:4470) enforces two invariants at write time that
matter for test setup:
- `max_amount` must be strictly ascending across brackets (no duplicate or
  out-of-order boundaries).
- The **last** bracket's `max_amount` must be exactly `i128::MAX` — brackets
  are required to be exhaustive, so there's no separate "amount exceeds all
  brackets" code path to test; it's structurally impossible to configure.

When no brackets have ever been set, `compute_fee` falls back to a single
synthetic bracket `{ max_amount: i128::MAX, rate_bps: platform_fee_bps }`
(lib.rs:4501-4513) — this is the "defined default" for criterion 3.

## Test scenarios

Suggested setup: two brackets, `[{max_amount: 1000, rate_bps: 200}, {max_amount: i128::MAX, rate_bps: 500}]`
(2% up to 1000, 5% above), installed via `set_fee_brackets`.

1. **Amount exactly equal to `max_amount` selects that bracket, not the next.**
   `compute_fee(1000)` — `remaining(1000) > slice_limit(1000)` is false, so the
   *entire* amount is charged at bracket 0's rate: `1000 * 200 / 10_000 = 20`.
   Assert result is `20`, not `1000*200/10_000 + 0*500/10_000` reaching into
   bracket 1 at all (i.e. assert no bracket-1 contribution, which in this
   single-tier case is the same number — the meaningful assertion is that
   `compute_fee(1000) != compute_fee(1001)`'s marginal-rate jump, see below).

2. **Amount one unit above `max_amount` spills the marginal unit into the next
   bracket.** `compute_fee(1001)` — bracket 0 takes its full width (1000 at
   2% = 20), the remaining 1 unit falls to bracket 1 at 5%: `1 * 500 / 10_000`
   truncates to `0` (integer division), so total is `20`. To make the spill
   visible in an assertion, pick amounts where the marginal unit's fee is
   non-zero, e.g. use `rate_bps: 5000` on bracket 1 or test at a larger scale
   (e.g. `max_amount: 1_000_000`, spill amount `1_000_020`) so
   `compute_fee(max_amount + 1) > compute_fee(max_amount)` is a strict,
   non-rounded inequality. Assert the fee attributable to the spilled unit
   equals `1 * bracket_1.rate_bps / 10_000` exactly.

3. **Amount exceeding all brackets falls back to a defined default.** Because
   `set_fee_brackets` forces the last bracket to `i128::MAX`, "exceeding all
   brackets" isn't reachable once brackets are configured — cover the
   *actual* default path instead: call `compute_fee` **before** ever calling
   `set_fee_brackets`, and assert it uses the flat `platform_fee_bps` single
   bracket (e.g. set `platform_fee_bps` to a known value via whatever the
   existing admin setter is, then assert `compute_fee(amount) == amount *
   platform_fee_bps / 10_000`).

4. **Invariant guard (bonus, not in original AC but cheap to add given it's
   the mechanism that makes scenario 3 well-defined):** assert
   `set_fee_brackets` panics/rejects a brackets vec whose last `max_amount !=
   i128::MAX`, and one with non-strictly-ascending `max_amount` values.

## Findings from this review

No off-by-one bug was found in the current `compute_fee`/`set_fee_brackets`
boundary logic — the `remaining > slice_limit` (strict) comparison correctly
keeps an amount equal to a bracket's `max_amount` entirely within that
bracket, and pushes only the excess into the next one. No code changes were
made for this issue.
