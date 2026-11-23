# Issue #1156: Fee Tier Calculation Edge Case Tests

## Overview

Edge-case tests for the three fee mechanisms: volume-based fee tiers, amount-based fee brackets, and volume-based rebates. Covers boundary values, discount application, tier transitions and rounding.

## Functions Under Test

All in `contracts/split/src/lib.rs`.

#### `set_fee_tiers(admin, tiers: Vec<FeeTier>)` / `get_fee_tiers()`
- `FeeTier { volume_threshold: u64, fee_bps: u32 }`
- At most 5 tiers, sorted strictly ascending by `volume_threshold`
- Emits `fee_tiers_updated`

#### `get_applicable_fee(creator) -> u32`
- No tiers: returns `get_platform_fee_bps()`
- Otherwise: the `fee_bps` of the highest tier with `creator_volume >= volume_threshold`
- If the volume is below every threshold, falls back to the platform fee

#### `set_fee_brackets(admin, brackets)` / `compute_fee(amount) -> i128`
- `FeeBracket { max_amount: i128, rate_bps: u32 }`, applied marginally (each slice of the amount is charged at its bracket's rate)
- `amount <= 0` returns `0`
- No brackets: a single flat bracket at the platform fee
- Each slice is charged `slice * rate_bps / 10_000`, rounded down per slice

#### `set_rebate_tiers(admin, tiers)` / `get_rebate_tiers()`
- At most 5 tiers, strictly ascending `min_volume`
- Rebate = `checked_bps_of(total_fee, rebate_bps, 10_000)` and accrues to the creator's rebate balance

## Test Cases

Add to `contracts/split/src/test.rs`.

### Boundary values
- `test_fee_tier_volume_exactly_at_threshold`: `volume == threshold` qualifies (`>=`)
- `test_fee_tier_volume_one_below_threshold`: `threshold - 1` stays in the lower tier
- `test_fee_tier_volume_zero`: falls back to the platform fee when the first threshold is above 0
- `test_fee_tier_threshold_zero`: a first tier at 0 applies to every creator
- `test_fee_tier_volume_u64_max`: picks the top tier with no overflow
- `test_fee_tier_bps_zero_and_10000`: 0% and 100% tiers

### Discount application
- `test_fee_tier_lower_bps_at_higher_volume`: a creator moving up a tier pays less on the next release
- `test_fee_tier_with_fee_waiver`: a waiver (`has_fee_waiver`) overrides the tier and the fee is 0
- `test_rebate_applied_on_release`: rebate balance equals `fee * rebate_bps / 10_000`
- `test_rebate_and_tier_combined`: the tier sets the fee, then the rebate is applied to that fee

### Tier transitions
- `test_fee_tier_transition_mid_lifetime`: release, volume crosses the threshold, release again; each release uses the tier in force at the time
- `test_fee_tier_update_by_admin_takes_effect`: replacing tiers changes the next `get_applicable_fee`
- `test_fee_tier_clear_falls_back_to_platform`: an empty tier vector restores the platform fee
- `test_fee_bracket_amount_spans_brackets`: an amount crossing 2–3 brackets equals the sum of the slices

### Rounding
- `test_compute_fee_rounds_down_per_slice`: e.g. 1 bps on 9_999 gives 0
- `test_compute_fee_dust_amount`: `amount = 1` gives 0 fee
- `test_compute_fee_marginal_vs_flat`: the marginal total is at most the flat fee at the top rate
- `test_rebate_rounding_never_exceeds_fee`

### Edge amounts
- `test_compute_fee_zero_and_negative`: returns 0
- `test_compute_fee_i128_max`: no panic, uses `u128` intermediate math
- `test_compute_fee_bracket_max_amount_i128_max`: last bracket is unbounded

### Validation
- `test_set_fee_tiers_more_than_five_rejected`
- `test_set_fee_tiers_unsorted_rejected`
- `test_set_fee_tiers_duplicate_threshold_rejected`
- `test_set_rebate_tiers_more_than_five_panics`: `"Maximum 5 rebate tiers allowed"`
- `test_set_rebate_tiers_unsorted_panics`: `"rebate tiers must be sorted"`
- `test_set_fee_tiers_non_admin_rejected`

## Known Gap

`set_fee_tiers` checks its limits with `debug_assert!`, which is **removed in release builds**. The on-chain WASM therefore accepts more than 5 tiers and unsorted tiers. Because `get_applicable_fee` scans from the end and stops at the first match, unsorted tiers can return the wrong fee. The three `set_fee_tiers_*_rejected` tests pass under `cargo test` (debug) but do not reflect the deployed behaviour. Recommended fix: change these checks to `assert!`, matching `set_rebate_tiers`.

## Acceptance Criteria

- [ ] Boundaries handled
- [ ] Discounts applied
- [ ] Transitions smooth
- [ ] Rounding correct (always down, never exceeds the expected fee)
- [ ] All tests pass (`cargo test -p split`)
- [ ] No calculation errors (no overflow at extreme amounts)
