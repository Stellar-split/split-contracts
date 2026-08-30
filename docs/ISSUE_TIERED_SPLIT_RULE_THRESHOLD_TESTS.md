# Issue: Missing Tests for `SplitRule::Tiered` Threshold Boundary

## Overview

`SplitRule::Tiered(threshold, bps)` pays `funded * bps / 10_000` to a
recipient only when `funded` **strictly exceeds** `threshold`; if
`funded <= threshold` the recipient receives `0`. No tests currently
verify this gate, so a regression in the conditional branch could ship
silently.

## Problem

In `_release_full` (inside `lib.rs`), the `Tiered` arm reads:

```rust
SplitRule::Tiered(threshold, bps) => {
    if funded > threshold {
        checked_bps_of(funded, bps, 10_000u128)
            .expect("ArithmeticOverflow")
    } else {
        0
    }
}
```

The `else` branch — the zero-payout path — is exercised by no existing
test, leaving the following bugs undetected:

* Off-by-one: using `>=` instead of `>` would pay recipients at exactly
  the threshold when they should receive nothing.
* Inverted condition: swapping the arms would pay recipients when they
  should receive nothing and vice-versa.
* Returning a non-zero constant instead of `0` in the else branch.

## Acceptance Criteria

| # | Rule | `funded` | Expected payout |
|---|------|----------|----------------|
| 1 | `Tiered(1000, 5000)` | `999` | `0` |
| 2 | `Tiered(1000, 5000)` | `1001` | `1001 * 5000 / 10_000 = 500` |
| 3 | Multiple `Tiered` rules on different recipients | mixed `funded` | each rule evaluated independently |

All three cases must pass `cargo test`.

---

## Type and Storage

`SplitRule` is a `#[contracttype]` enum defined in
`contracts/split/src/types.rs`:

```rust
pub enum SplitRule {
    Fixed(i128),
    Percentage(u32),
    /// Pay `funded * bps / 10_000` only once `funded` strictly exceeds
    /// `threshold`; otherwise pay `0`.  Encoded as `(threshold, bps)`.
    Tiered(i128, u32),
}
```

Split rules are stored on `InvoiceExt.split_rules: Vec<SplitRule>` and
evaluated at release time inside `_release_full` in
`contracts/split/src/lib.rs`.

---

## Implementation Notes

### Why `funded > threshold` and not `funded >= threshold`

The contract intentionally uses a **strict** comparison so that funding a
Tiered invoice to exactly the threshold amount does not trigger a payout.
A payer must fund past the threshold before any share is owed. Tests must
assert `funded == threshold` yields `0` and `funded == threshold + 1`
yields a non-zero result.

### Arithmetic

The payout when the gate is open is:

```
payout = funded * bps / 10_000
```

computed via `checked_bps_of(funded, bps, 10_000u128)` which uses
`u128` intermediates to prevent overflow and returns
`Err(ContractError::ArithmeticOverflow)` on overflow or
divide-by-zero.

### Interaction with `split_rules` validation at creation

`create_invoice` validates that split rules sum to exactly `10_000`
basis points. For a `Tiered(threshold, bps)` rule, `bps` is counted in
this sum regardless of whether the threshold will be met at release time,
so tests must ensure the full set of rules still sums to `10_000`.

---

## Test Plan

All tests should be placed in `contracts/split/src/test.rs` and follow
the conventions already established there (use `setup_initialized`,
`default_options`, `make_invoice`, etc.).

### Test 1 — Below threshold → zero payout

**Scenario:** A two-recipient invoice uses
`[Tiered(1000, 5000), Tiered(1000, 5000)]` split rules.
The invoice is funded to `999` (one stroop below the threshold of
`1000`). Releasing the invoice should pay both recipients `0`.

```
rule  : Tiered(1000, 5000)   (50 % once past 1 000)
funded: 999
expect: payout = 0
```

**Setup sketch:**

```rust
#[test]
fn test_tiered_rule_below_threshold_pays_zero() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator    = Address::generate(&env);
    let recipient1 = Address::generate(&env);
    let recipient2 = Address::generate(&env);

    // Mint tokens to a payer
    StellarAssetClient::new(&env, &token_id)
        .mint(&creator, &10_000);

    env.ledger().set_timestamp(1_000);

    // Build split rules: two Tiered(1000, 5000) rules (50 % each, sums to 10 000 bps)
    let mut split_rules: Vec<SplitRule> = Vec::new(&env);
    split_rules.push_back(SplitRule::Tiered(1000, 5000));
    split_rules.push_back(SplitRule::Tiered(1000, 5000));

    let mut options = default_options(&env);
    options.split_rules = split_rules;

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient1.clone());
    recipients.push_back(recipient2.clone());
    // Amounts are used for rule-sum validation at creation; choose equal values
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);
    amounts.push_back(500_i128);

    let id = c.create_invoice(
        &creator, &recipients, &amounts, &token_id, &9_999_u64, &options,
    );

    // Fund to 999 (one below threshold)
    let payer = Address::generate(&env);
    StellarAssetClient::new(&env, &token_id).mint(&payer, &999);
    c.pay(&payer, &id, &999_i128, &0_u64, &false, &false, &None);

    let balance_r1_before = tk.balance(&recipient1);
    let balance_r2_before = tk.balance(&recipient2);

    c.release_invoice(&creator, &id, &None);

    // Both recipients should have received nothing
    assert_eq!(tk.balance(&recipient1), balance_r1_before);
    assert_eq!(tk.balance(&recipient2), balance_r2_before);
}
```

---

### Test 2 — Above threshold → correct proportional payout

**Scenario:** Same two-recipient invoice, funded to `1001` (one stroop
above the threshold of `1000`). Each recipient holds a
`Tiered(1000, 5000)` rule (50 %).

```
rule  : Tiered(1000, 5000)
funded: 1001
expect: payout_per_recipient = 1001 * 5000 / 10_000 = 500
        (integer floor division; total paid out = 1000, remainder 1 stays in contract)
```

**Setup sketch:**

```rust
#[test]
fn test_tiered_rule_above_threshold_pays_correct_amount() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator    = Address::generate(&env);
    let recipient1 = Address::generate(&env);
    let recipient2 = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&creator, &10_000);
    env.ledger().set_timestamp(1_000);

    let mut split_rules: Vec<SplitRule> = Vec::new(&env);
    split_rules.push_back(SplitRule::Tiered(1000, 5000));
    split_rules.push_back(SplitRule::Tiered(1000, 5000));

    let mut options = default_options(&env);
    options.split_rules = split_rules;

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient1.clone());
    recipients.push_back(recipient2.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);
    amounts.push_back(500_i128);

    // Total = 1000; but we want to fund slightly over (1001).
    // Adjust amounts so the invoice target >= funded amount, or allow overfunding.
    // Simplest: set amounts to 1001 so the invoice accepts the payment fully.
    let mut amounts2: Vec<i128> = Vec::new(&env);
    amounts2.push_back(501_i128);
    amounts2.push_back(500_i128);
    let id = c.create_invoice(
        &creator, &recipients, &amounts2, &token_id, &9_999_u64, &options,
    );

    let payer = Address::generate(&env);
    StellarAssetClient::new(&env, &token_id).mint(&payer, &1001);
    // Pay 1001; invoice total = 1001 so this also triggers auto-release.
    // Prevent auto-release by using a prerequisite or check funded before release.
    // For simplicity, pay 1001 which auto-releases.

    c.pay(&payer, &id, &1001_i128, &0_u64, &false, &false, &None);

    // expected per-recipient: 1001 * 5000 / 10_000 = 500
    assert_eq!(tk.balance(&recipient1), 500);
    assert_eq!(tk.balance(&recipient2), 500);
}
```

---

### Test 3 — Multiple independent `Tiered` rules

**Scenario:** A three-recipient invoice where:

* Recipient A: `Tiered(500, 3000)` — 30 % once past 500
* Recipient B: `Tiered(2000, 3000)` — 30 % once past 2 000
* Recipient C: `Tiered(0, 4000)` — 40 % always (threshold = 0, so
  `funded > 0` is always true once any payment arrives)

The invoice is funded to exactly `1500`. Only recipients A and C are
above their respective thresholds; B is not.

```
funded: 1500

A: threshold = 500,  1500 >  500  → 1500 * 3000 / 10_000 = 450
B: threshold = 2000, 1500 <= 2000 → 0
C: threshold = 0,    1500 >  0    → 1500 * 4000 / 10_000 = 600
```

**Setup sketch:**

```rust
#[test]
fn test_tiered_rules_evaluated_independently() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator    = Address::generate(&env);
    let recipient_a = Address::generate(&env);
    let recipient_b = Address::generate(&env);
    let recipient_c = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&creator, &10_000);
    env.ledger().set_timestamp(1_000);

    // Rules sum: 3000 + 3000 + 4000 = 10_000 ✓
    let mut split_rules: Vec<SplitRule> = Vec::new(&env);
    split_rules.push_back(SplitRule::Tiered(500,  3000));
    split_rules.push_back(SplitRule::Tiered(2000, 3000));
    split_rules.push_back(SplitRule::Tiered(0,    4000));

    let mut options = default_options(&env);
    options.split_rules = split_rules;

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient_a.clone());
    recipients.push_back(recipient_b.clone());
    recipients.push_back(recipient_c.clone());

    // Amounts only need to be positive and pass the split_rules BPS sum check.
    // Total must be >= funded (1500) so the invoice accepts the payment.
    let mut amounts: Vec<i128> = Vec::new(&env);
    amounts.push_back(500_i128);   // 30 % target
    amounts.push_back(500_i128);   // 30 % target
    amounts.push_back(500_i128);   // 40 % target (total = 1500)

    let id = c.create_invoice(
        &creator, &recipients, &amounts, &token_id, &9_999_u64, &options,
    );

    // Fund exactly 1500 — auto-releases because total == funded
    let payer = Address::generate(&env);
    StellarAssetClient::new(&env, &token_id).mint(&payer, &1500);
    c.pay(&payer, &id, &1500_i128, &0_u64, &false, &false, &None);

    assert_eq!(tk.balance(&recipient_a), 450,  "A should receive 450 (threshold met)");
    assert_eq!(tk.balance(&recipient_b),   0,  "B should receive 0  (threshold not met)");
    assert_eq!(tk.balance(&recipient_c), 600,  "C should receive 600 (threshold = 0 always met)");
}
```

---

## Files to Modify

| File | Change |
|------|--------|
| `contracts/split/src/test.rs` | Add the three test functions described above |

No production code changes are required — the existing implementation in
`_release_full` already handles these cases correctly. These tests exist
solely to **guard the existing behaviour** against future regressions.

---

## Verification

```
cargo test -p split -- tiered 2>&1
```

All three new tests should pass with output similar to:

```
test test_tiered_rule_below_threshold_pays_zero        ... ok
test test_tiered_rule_above_threshold_pays_correct_amount ... ok
test test_tiered_rules_evaluated_independently         ... ok
```
