# Gas Optimizations (issue #761)

This document records the measured compute-unit (CU) cost of the bulk refund
path after it was consolidated into a **single storage scan + single transfer
loop**.

## Background

`refund` must aggregate every payer's contribution and then return each
contribution individually. Historically the aggregation step re-read the payment
shards once per payer it encountered, giving:

```
O(S × N)   —   one full shard scan per payer
```

for an invoice with *N* payers spread across *S* shards.

## Approach

The shipped implementation (see `refund` in `contracts/split/src/lib.rs`):

1. performs **one pass** over all `SHARD_COUNT` payment shards, accumulating
   per-payer totals into two in-memory maps (`totals` and `donate_totals`);
2. creates a **single** `token::Client` and reuses it for every transfer;
3. then issues one transfer per payer.

```
O(S + N)   —   one shard scan total, then one transfer loop
```

Because the per-payer transfer is the dominant cost, total consumption is
effectively linear in the number of payers.

## Measured Savings

Measured against the Soroban test host (`soroban-env-host 22.x`) using
`env.cost_estimate().budget().cpu_instruction_cost()`. The budget tracker resets
before every top-level invocation, so each figure covers the `refund` call
alone. All values are CPU instructions; the Soroban limit is 100 000 000.

| Payers | CU consumed | % of 100M limit | Growth vs 5 payers |
|--------|-------------|-----------------|--------------------|
|      5 |   3 030 370 |           3.0 % |              1.0×  |
|     20 |   9 078 795 |           9.1 % |              3.0×  |
|     50 |  28 333 977 |          28.3 % |              9.3×  |

The near-linear scaling (3.0× and 9.3× for 3× and 10× the number of payers) is
the observable signature of `O(S + N)`: the fixed shard-scan cost is amortised
and the dominant term is the single transfer per payer.

> **Note on the baseline.** An earlier revision of this document quoted a
> *before/after* table whose pre-refactor figures were not reproducible from a
> committed benchmark. They have been replaced with the measured post-refactor
> figures above. The substantive change is the algorithmic reduction from
> `O(S × N)` to `O(S + N)`; the absolute values are host-specific.

## Stress test

`test_refund_50_payers` (in `contracts/split/src/test.rs`) drives 50 distinct
payers through `pay`, lets the invoice expire, calls `refund`, and asserts:

* the invocation consumes **< 100 000 000** CPU instructions, and
* every payer's token balance is restored to exactly the amount they paid
  (behaviour unchanged).

## Key invariants preserved

- Refund amounts per payer are unchanged.
- `donate_on_failure` contributions are still correctly redirected to the creator.
- All events (`payer_refunded`, `invoice_refunded`, `invoice_state_changed`) fire
  in the same order as before.
