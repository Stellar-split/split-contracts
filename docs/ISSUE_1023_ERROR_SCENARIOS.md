# Error scenario testing (#1023)

The suite lives in `contracts/split/src/error_scenario_tests.rs`.

## How a scenario runs

1. A fresh environment is created with an initialized contract and a pending 100-stroop invoice.
2. **Error injection:** one invalid operation is attempted through the `try_*` client, which must be rejected.
3. **State check:** the invoice's `funded` and `status` must equal their values from before the attempt.
4. **Recovery:** a valid payment of the full amount must still succeed and release the invoice.

## Scenario catalogue

| Scenario | Category | Expected result |
|---|---|---|
| pay zero | InvalidAmount | rejected |
| pay negative | InvalidAmount | rejected |
| pay more than remaining | InvalidAmount | rejected |
| pay after deadline | Timing | rejected |
| refund before deadline | Timing | rejected |
| create invoice with zero amount | InvalidInput | rejected |
| create invoice with mismatched recipients and amounts | InvalidInput | rejected |
| pay an unknown invoice | NotFound | rejected |

Two further tests cover state errors: cancelling an invoice that is already released, and refunding twice.

## Reporting and analytics

`ErrorReport` records which scenarios were rejected or wrongly accepted, how many recovered, and a count per `ErrorCategory`. The `error_analytics_group_scenarios_by_category` test checks the per-category totals.

## Adding a scenario

Add an entry to `scenarios()` with a name, a category and an `inject` function that returns `true` when the contract rejects the operation, then update the table above.
