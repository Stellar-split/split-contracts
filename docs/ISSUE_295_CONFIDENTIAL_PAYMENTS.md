# Issue #295: Confidential Payment Amounts Using Blinded Commitments

## Overview

Implements confidential payment mode where amounts are hidden using Pedersen commitments, revealed only to the invoice creator.

## Implementation Details

### Entry Points

#### `pay_confidential(invoice_id, commitment, range_proof, encrypted_amount)`
- Submits a confidential payment for an invoice
- `commitment`: Pedersen commitment C = r·G + amount·H
- `range_proof`: Bulletproof-style proof (placeholder for full ZK integration)
- `encrypted_amount`: Amount encrypted under creator's public key
- Stores commitment and encrypted amount per (invoice_id, payer)
- Increments confidential payment counter

#### `reveal_confidential_total(invoice_id, decrypted_sum, proof)`
- Creator reveals the decrypted sum of all confidential payments
- Provides ZK proof that decrypted_sum matches encrypted amounts
- Credits the sum to the invoice
- Triggers release if fully funded

#### `get_confidential_payment_count(invoice_id) -> u32`
- Returns number of confidential payments for an invoice
- Tracks total payments without revealing amounts

## Cryptographic Scheme

The implementation uses a placeholder verification scheme for future ZK integration:

1. **Range Proof Validation**: Hash concatenation of commitment + proof, verify non-zero
2. **Reveal Proof Validation**: Hash of proof bytes, verify non-zero
3. **Full ZK Integration**: Future versions will use Bulletproofs or similar for on-chain verification

## Storage

- Per-payment: `(invoice_id, payer) -> ConfidentialPayment { commitment, encrypted_amount }`
- Counter: `invoice_id -> u32` (count of confidential payments)

## Testing

All tests in `contracts/split/src/test.rs`:
- `test_pay_confidential_stores_commitment` - Verify storage
- `test_confidential_payment_overwrite` - Same payer replaces previous commitment
- `test_get_confidential_payment_count` - Counter increments correctly
- `test_pay_confidential_rejects_zero_range_proof` - Validation enforced
- `test_reveal_confidential_total_triggers_release` - Full funding triggers release
- `test_reveal_confidential_total_partial_funding` - Partial funding keeps pending
- `test_reveal_confidential_total_rejects_zero_sum` - Must be positive
- `test_pay_confidential_blocked_by_circuit_breaker` - Circuit breaker integration
- `test_reveal_confidential_blocked_by_circuit_breaker` - Circuit breaker blocks reveal
