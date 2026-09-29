# Issue #671: Integration Test Plan — InstalmentPlan `paid_index` Tracking

Test plan only — no test code. Written per explicit request; the acceptance
criteria below should be turned into `#[test]` functions in
`contracts/split/src/test.rs` by whoever picks this up.

## Code under test

- `InstalmentTranche { amount: i128, ledger: u32 }` — `contracts/split/src/types.rs:1587-1592`
- `InstalmentPlan { tranches: Vec<InstalmentTranche>, paid_index: u32 }` —
  `contracts/split/src/types.rs:1594-1599`
- Advancement logic lives inside `SplitContract::_pay` —
  `contracts/split/src/lib.rs:6861-6879`

## How paid_index advancement actually works

At the top of `_pay`, if an `InstalmentPlan` exists for `(invoice_id,
payer)`:

```rust
let paid_index = plan.paid_index;
assert!((paid_index as usize) < plan.tranches.len().try_into().unwrap(), "ScheduleViolation");
let tranche = plan.tranches.get(paid_index).unwrap();
if amount != tranche.amount || env.ledger().sequence() < tranche.ledger {
    panic!("ScheduleViolation");
}
plan.paid_index += 1;
env.storage().persistent().set(&plan_storage_key, &plan);
events::instalment_tranche_paid(env, invoice_id, payer, paid_index, amount);
```

Two independent gates must pass for a tranche to be accepted:
1. `amount == tranche.amount` exactly (no partial/over payment against a
   tranche).
2. `env.ledger().sequence() >= tranche.ledger` — note this compares the
   **ledger sequence**, not `env.ledger().timestamp()`. Advancing "ledger
   time" in the test must mean bumping the sequence number
   (`env.ledger().set(LedgerInfo { sequence_number: ..., .. })` or the
   test harness's `env.ledger().with_mut(...)`), not the timestamp — using
   the wrong one will make a valid test look like it's testing the reject
   path when it isn't.

No explicit bounds-check panic message distinguishes "already fully paid"
(`paid_index == tranches.len()`) from any other assert — both currently
surface as `"ScheduleViolation"`. That's fine for these tests but worth
noting if a future test wants to assert on a specific panic reason.

## Test scenarios

Setup: create an instalment plan via `SplitContract::register_instalment_plan`
(`contracts/split/src/lib.rs:4422`) for a payer on an invoice, with three
tranches at increasing ledger sequences, e.g.:
```
tranches = [
  { amount: 100, ledger: 10 },
  { amount: 150, ledger: 20 },
  { amount: 200, ledger: 30 },
]
```

1. **Plan creation.** After installing the plan, assert `paid_index == 0`
   and `tranches.len() == 3`.

2. **Advance ledger time and pay each tranche in order, asserting
   `paid_index` increments.**
   - Set ledger sequence to `10`, pay `100` → assert `paid_index == 1`.
   - Set ledger sequence to `20`, pay `150` → assert `paid_index == 2`.
   - Set ledger sequence to `30`, pay `200` → assert `paid_index == 3`.
   - Also assert `instalment_tranche_paid` fires with the pre-increment
     index each time (the event is emitted with the *old* `paid_index`
     value, captured before the `+= 1`, per lib.rs:6876-6878).
   - After the third payment, `paid_index (3) == tranches.len() (3)`; assert
     a fourth payment attempt at any amount panics with `ScheduleViolation`
     via the bounds check, not the amount/ledger check.

3. **Paying an instalment before its ledger is rejected.**
   With the plan freshly created (`paid_index == 0`, sequence < 10), attempt
   to pay tranche 0's exact amount (`100`) while the current ledger sequence
   is still below `10`. Assert it panics with `ScheduleViolation` and that
   `paid_index` is unchanged (still `0`) — the panic happens before the
   `plan.paid_index += 1` / storage write, so this should hold structurally,
   but it's worth asserting explicitly since it's the exact invariant the
   issue cares about.
   Also worth covering as a variant: paying the *wrong amount* at the
   correct ledger (e.g. `99` instead of `100` at sequence `10`) hits the
   same `ScheduleViolation` panic via the `amount != tranche.amount` half of
   the condition — useful to confirm both halves of the `||` are load-bearing
   independently.

## Findings from this review

No bug found in the advancement/gating logic — both the amount-equality and
ledger-sequence-order checks are correctly enforced before `paid_index` is
mutated or persisted. No code changes were made for this issue. The only
practical gotcha for whoever writes the tests is the ledger-sequence vs.
timestamp distinction called out above.
