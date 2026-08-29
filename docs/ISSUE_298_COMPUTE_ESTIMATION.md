# Issue #298: Per-Invoice Compute Cost Estimation Before Submission

## Overview

Adds a `simulate_release()` read function that estimates the compute cost of releasing an invoice before the transaction is submitted, helping prevent compute budget exhaustion failures.

## Implementation Details

### Read Function

#### `simulate_release(invoice_id) -> SimulateReleaseResult`
- Public read function (not affected by circuit breaker)
- Returns `{ estimated_instructions: u64, estimated_fee_stroops: u64, would_succeed: bool }`

**Return Fields:**
- `estimated_instructions`: Estimated Soroban instructions required for release
- `estimated_fee_stroops`: Estimated Stroops fee (conversion using Soroban fee schedule)
- `would_succeed`: Boolean indicating if estimated cost is within budget

## Estimation Calculation

The estimation accounts for:

1. **Base Overhead**: `INSTRUCTIONS_BASE = 1_000_000`
   - Fixed cost for release setup, validation, etc.

2. **Per Recipient Cost**: `recipient_count * INSTRUCTIONS_PER_RECIPIENT = recipient_count * 500_000`
   - Cost of transferring tokens to each recipient
   - Includes fee calculation, event emission

3. **Per Shard Cost**: `SHARD_COUNT * INSTRUCTIONS_PER_SHARD = 8 * 100_000`
   - Cost of aggregating payment shards
   - Fixed at 8 shards per contract architecture

4. **Total Formula**:
```
estimated_instructions = 1_000_000 + recipient_count * 500_000 + 8 * 100_000
```

5. **Fee Conversion**:
```
estimated_fee_stroops = (estimated_instructions / 10_000) * STROOPS_PER_10K_INSTRUCTIONS
estimated_fee_stroops = estimated_instructions / 10_000  (1 Stroop per 10k instructions)
```

6. **Budget Check**:
```
would_succeed = estimated_instructions <= INSTRUCTION_BUDGET_LIMIT (100_000_000)
```

## Constants

- `INSTRUCTION_BUDGET_LIMIT: u64 = 100_000_000` - Soroban per-transaction limit
- `INSTRUCTIONS_BASE: u64 = 1_000_000`
- `INSTRUCTIONS_PER_RECIPIENT: u64 = 500_000`
- `INSTRUCTIONS_PER_SHARD: u64 = 100_000`
- `SHARD_COUNT: u64 = 8`
- `STROOPS_PER_10K_INSTRUCTIONS: u64 = 1`

## Use Case

SDK integration pattern:
```rust
let result = contract.simulate_release(invoice_id);
if !result.would_succeed {
    warn!("Invoice may fail due to budget: {} instructions", result.estimated_instructions);
    // Don't submit the transaction
}
```

## Testing

All tests in `contracts/split/src/test.rs`:
- `test_simulate_release_single_recipient` - Basic estimation for 1 recipient
- `test_simulate_release_multiple_recipients` - Scaling with 10 recipients
- `test_simulate_release_instruction_budget_calculation` - Verify calculation formula
- `test_simulate_release_would_succeed_at_budget_limit` - Confirm budget check
- `test_simulate_release_estimate_for_large_invoice` - 50-recipient invoice estimation
