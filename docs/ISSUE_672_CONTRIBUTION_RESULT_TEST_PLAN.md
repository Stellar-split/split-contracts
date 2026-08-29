# Issue #672: Test Plan — ContributionResult Fields After Capped Payment

Test plan only — no test code. Written per explicit request; the acceptance
criteria below should be turned into `#[test]` functions in
`contracts/split/src/test.rs` by whoever picks this up.

## Code under test

- `ContributionResult { invoice_id, amount_applied, refund_amount }` —
  `contracts/split/src/types.rs:1645-1652`
- `SplitContract::contribute` — `contracts/split/src/lib.rs:2917-3004`

## How capping actually works

```rust
let total: i128 = invoice.amounts.iter().sum();
let remaining = total.saturating_sub(invoice.funded);

let (amount_applied, refund_amount) = if amount > remaining {
    (remaining, amount - remaining)
} else {
    (amount, 0i128)
};
```

`remaining` is clamped at 0 via `saturating_sub`, so it can never go negative
even if `funded` somehow exceeds `total`. `amount_applied` is only ever
credited to `invoice.funded` and pushed as a `Payment` when `amount_applied >
0` (lib.rs:2979-2997); `refund_amount` itself is not transferred back to the
payer inside `contribute` — it only emits `refund_issued` (lib.rs:2975-2977)
and is returned to the caller for the actual transfer to happen elsewhere in
the call chain. Confirm that behavior (or whatever the current transfer
convention is) before asserting on-chain token balances in the test, not just
the returned struct.

## Test scenarios

1. **Partial cap: total 1000, funded 900, contribute 200.**
   `remaining = 100`. `amount(200) > remaining(100)` →
   `amount_applied = 100`, `refund_amount = 100`. Assert both fields, and
   assert `invoice.funded == 1000` after the call (via `get_invoice` or
   equivalent), and that the invoice's status flips to `Released` since
   `funded >= total` is reached (lib.rs:2991-2994) — worth asserting
   explicitly since it's an easy thing to regress.

2. **Contribution within limit: total 1000, funded 500, contribute 200.**
   `remaining = 500`. `amount(200) > remaining(500)` is false →
   `amount_applied = 200`, `refund_amount = 0`. Assert `refund_amount == 0`
   and `invoice.funded == 700`, status still `Pending`.

3. **Invoice already fully funded: contribute 200 → `amount_applied == 0`,
   `refund_amount == 200`.**
   Caution on setup: `contribute` itself flips status to `Released` the
   moment `funded` reaches `total` (lib.rs:2991-2994), and `contribute`
   asserts `invoice.status == Pending` up front (lib.rs:2962) — so driving an
   invoice to `funded == total` via a **prior `contribute` call** and then
   calling `contribute` again will panic with `InvoiceNotPending` before
   reaching the capping logic at all, not return a zeroed
   `ContributionResult`. That panic path itself may be worth a separate
   test, but it is a different scenario than this AC describes.
   To reach "funded >= total, status still Pending, then contribute()
   returns a capped-to-zero result," the invoice needs to become overfunded
   through a path that does *not* flip status — e.g. a payment made via
   `pay`/`_pay` under `OverfundingPolicy::AcceptAll` (see
   `types.rs` `OverfundingPolicy`), which allows `funded` to exceed `total`
   while leaving status as-is. Confirm `_pay`'s `AcceptAll` handling doesn't
   also transition status before relying on it for this test's setup. Then:
   `remaining = total.saturating_sub(funded) = 0` (saturating, so funded >
   total doesn't go negative), `amount(200) > remaining(0)` → `amount_applied
   = 0`, `refund_amount = 200`. Assert `invoice.funded` is unchanged by this
   call (no `Payment` record pushed, since the `amount_applied > 0` guard at
   lib.rs:2979 is false).

## Findings from this review

The capping arithmetic itself is correct for all three scenarios. The one
non-obvious risk is scenario 3's test setup, flagged above: naively "fully
fund via contribute() then call contribute() again" does not exercise the
capped-return path, it hits an unrelated `InvoiceNotPending` panic. No code
changes were made for this issue — this is a test-design note, not a bug in
`contribute` itself.
