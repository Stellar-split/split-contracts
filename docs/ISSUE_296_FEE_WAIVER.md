# Issue #296: Fee Waiver Mechanism for Whitelisted Creators

## Overview

Adds an admin-managed fee waiver list so specific creators (e.g., non-profits, launch partners) can be exempted from platform fees.

## Implementation Details

### Admin Functions

#### `add_fee_waiver(admin, creator)`
- Admin-only (SuperAdmin role required)
- Adds creator address to the fee waiver list
- Enforces max 100 entries
- Emits `fee_waiver_granted` event

#### `remove_fee_waiver(admin, creator)`
- Admin-only (SuperAdmin role required)
- Removes creator from fee waiver list
- Emits `fee_waiver_revoked` event

### Read Function

#### `has_fee_waiver(creator) -> bool`
- Public read function
- Returns true if creator is on the waiver list
- Used at release time to determine platform fee

## Fee Deduction Logic

At release time in `_release()`:
```
if creator_has_fee_waiver:
    platform_fee_bps = 0
else:
    platform_fee_bps = configured_value
```

## Storage

- List stored in persistent storage under `creator_fee_waiver_key()`
- Max entries: 100 (enforced in `add_fee_waiver`)
- Type: `Vec<Address>`

## Events

- `fee_waiver_granted { creator }` - When waiver is added
- `fee_waiver_revoked { creator }` - When waiver is removed

## Testing

All tests in `contracts/split/src/test.rs`:
- `test_add_fee_waiver` - Admin can add creator to waiver list
- `test_remove_fee_waiver` - Admin can remove creator from waiver list
- `test_fee_waiver_exempts_from_fees` - Creator on list pays 0 platform fee
- `test_fee_waiver_max_entries_enforced` - Adding 101st entry fails
- `test_fee_waiver_persists_across_operations` - Waiver remains active through operations
