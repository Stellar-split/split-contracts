# Issue #297: Contract-Wide Circuit Breaker for Emergency Pause

## Overview

Adds a circuit breaker that an admin can activate to halt all state-mutating operations immediately in case of discovered vulnerability or emergency.

## Implementation Details

### Admin Functions

#### `activate_circuit_breaker(admin, reason)`
- Admin-only (SuperAdmin role required)
- Sets circuit breaker flag to true
- Stores reason string
- Emits `circuit_breaker_activated` event
- All mutating operations immediately fail

#### `deactivate_circuit_breaker(admin)`
- Admin-only (SuperAdmin role required)
- Sets circuit breaker flag to false
- Clears reason string
- Emits `circuit_breaker_deactivated` event
- Operations resume normally

### Read Function

#### `get_circuit_breaker_status() -> CircuitBreakerStatus`
- Public read function (not affected by breaker)
- Returns `{ active: bool, reason: Option<String> }`
- Allows monitoring of circuit breaker state

## Enforcement

### Blocked Mutating Operations

When `circuit_breaker` is active, the following fail:
- `create_invoice`
- `pay`
- `pay_confidential`
- `reveal_confidential_total`
- `refund`
- All other state-mutating operations

### Allowed Read Operations

Always work, even when active:
- `get_invoice`
- `get_invoice_ext`
- `get_circuit_breaker_status`
- `get_confidential_payment_count`
- `has_fee_waiver`
- `simulate_release`

## Implementation

- Check added in `require_not_paused()` function
- Reads `circuit_breaker_key()` from persistent storage
- Defaults to `false` (inactive)
- Error returned: `"ContractPaused"`

## Storage

- Flag: `circuit_breaker_key() -> bool`
- Reason: `circuit_breaker_reason_key() -> Option<String>`
- Type: Persistent storage

## Events

- `circuit_breaker_activated { reason }` - When activated
- `circuit_breaker_deactivated {}` - When deactivated

## Testing

All tests in `contracts/split/src/test.rs`:
- `test_circuit_breaker_activate_deactivate` - Basic activation/deactivation
- `test_circuit_breaker_blocks_create_invoice` - Prevents invoice creation
- `test_circuit_breaker_blocks_pay` - Prevents payments
- `test_circuit_breaker_allows_read_operations` - Read-only operations still work
- `test_circuit_breaker_prevents_refund` - Blocks refund operations
- `test_pay_confidential_blocked_by_circuit_breaker` - Integration with confidential payments
- `test_reveal_confidential_blocked_by_circuit_breaker` - Integration with confidential reveals
