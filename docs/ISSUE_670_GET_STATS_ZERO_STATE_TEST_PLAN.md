# Issue #670: Test Plan — `get_stats` Returns Zero on a Fresh Contract

Test plan only — no test code. Written per explicit request; the acceptance
criteria below should be turned into `#[test]` functions in
`contracts/split/src/test.rs` by whoever picks this up.

## Important: the issue's "target file" is stale/dead code

The issue names `contracts/split/src/stats.rs` as the target and describes
`get_stats()` reading "protocol-wide counters from instance storage." That
file exists but **is not part of the compiled crate** —
`contracts/split/src/lib.rs` never declares `mod stats;`, so nothing in it is
reachable from the contract. The real, publicly-callable `get_stats` is:

- `SplitContract::get_stats` — `contracts/split/src/lib.rs:12687-12709`

It returns a **4-tuple** `(total_invoices: u64, total_volume: i128,
total_released: i128, total_refunded: i128)`, backed by **persistent**
storage keys `tot_inv` / `tot_vol` / `tot_rel` / `tot_ref`
(lib.rs:561-589) — not the 3-counter, instance-storage shape
`stats.rs` implements. Write tests against `lib.rs`'s `get_stats`; the
`stats.rs` module has no effect on contract behavior as it stands today.

(Separately: while reviewing `stats.rs` for this issue, it turned out to be
genuinely broken — two competing implementations pasted together from an
unresolved merge, with duplicate `pub type Stats` and duplicate `pub fn
get_stats` definitions, which would fail to compile if the module were ever
declared. That's fixed as part of this pass — see below — but it doesn't
change what to test, since the module still isn't wired into the crate.)

## Test scenarios

Map the acceptance criteria onto the real 4-tuple API:

1. **Fresh contract, no invoices created → all counters zero.**
   Deploy the contract (whatever the test harness's standard `initialize`
   setup is), call `get_stats()` with no prior invoice/payment activity, and
   assert `(total_invoices, total_volume, total_released, total_refunded) ==
   (0, 0, 0, 0)`. This exercises the `unwrap_or(0u64)` /
   `unwrap_or(0i128)` fallbacks at lib.rs:12688-12707 for all four keys.

2. **Create one invoice → `total_invoices` increments to 1, other counters
   stay zero.**
   Call `create_invoice` (or the test harness's helper) once, then
   `get_stats()`, and assert `total_invoices == 1` with `total_volume ==
   total_released == total_refunded == 0` — creating an invoice alone
   shouldn't touch the volume/released/refunded counters, only the
   `checked_add(1)` on `total_invoices` at lib.rs:5790-5799.

3. *(Not in the original AC, but cheap given scenario 2's setup and matches
   the "off-by-one on the boundary" spirit of these four issues)*: create a
   second invoice and assert `total_invoices == 2`, to confirm the counter
   accumulates rather than resets or saturates at 1.

## Findings from this review

- **Fixed:** `contracts/split/src/stats.rs` contained two duplicate,
  independently-written implementations of the same module (both created
  under separate PRs for issue #313, then mechanically concatenated by a
  merge commit — `643cf68`, merging branches that each added `d06e833` and
  `7b9a33f`). It had duplicate `pub type Stats` and duplicate `pub fn
  get_stats` definitions, which is invalid Rust (E0428, duplicate
  definition) and would only surface at compile time if the module were ever
  declared with `mod stats;`. Deduplicated to a single, consistent
  implementation; behavior is unchanged since the module remains unused.
  No `mod stats;` declaration was added — wiring it in and reconciling it
  with `lib.rs`'s existing (and differently-shaped) `get_stats` is a larger
  change than this test-planning pass and was left alone.
- No bug found in `lib.rs`'s real `get_stats` / counter-increment logic —
  zero-defaulting and the `total_invoices` increment on creation both look
  correct.
