# Fee calculation flow

This page describes how the platform fee is calculated and reported in `contracts/split`. The code is
in `src/calc.rs` and `src/math.rs`.

## Inputs

- `funded`: the amount paid into the invoice, in the token's smallest unit.
- `fee_bps`: the platform fee rate in basis points. 100 bps is 1%, and the valid range is 0 to 10 000.

## Calculation

```text
platform_fee = funded * fee_bps / 10_000
```

- The product is computed with `mul_div`, which is overflow-safe. If `funded * fee_bps` overflows
  `i128`, the call returns `ArithmeticOverflow`.
- Division rounds toward zero, so the fee never exceeds the exact value.
- A fee rate of 0, or a funded amount of 0, gives a fee of 0 with no error.

## Worked example

| Funded | Rate | Fee |
|---|---|---|
| 1 000 000 | 150 bps (1.5%) | 15 000 |
| 1 000 000 | 0 bps | 0 |
| 999 | 150 bps | 14 (exact value 14.985, rounded down) |

## Funding share

Separately from the fee, `funding_bps(funded, total)` reports progress as a basis-point share of the
total, clamped to 10 000. Overfunding reports 10 000, and a zero total reports 0.

## Events

- `fee_calculated` with topics `("fee_calculated", fee_type)` and the computed amount as data.
- `fee_distributed` with topics `("fee_distributed", fee_type)` and the recipient and amount as data.

The fee type is part of the topic, so an indexer can filter by it without decoding the data.

## Not covered here

Which fee types exist beyond the platform fee, and where each one is routed, are defined in
`FeeType` in `src/calc.rs`. This page does not list them.
