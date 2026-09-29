# Issue: Missing Tests for `auto_resolve` Refund Path

## Summary

`auto_resolve` supports `ResolveAction::Refund`, which returns all contributions
to payers when `funded / total >= min_funded_bps / 10_000`. No test currently
exercises this path, leaving the refund branch of `auto_resolve` completely
untested.

## Problem Statement

The `auto_resolve` entry point evaluates a list of `ResolveRule` structs in
order and executes the action for the first matching rule. `ResolveAction` has
two variants:

| Variant   | Effect                                   |
|-----------|------------------------------------------|
| `Release` | Distributes funds to recipients normally |
| `Refund`  | Returns all contributions to payers      |

The `Release` path is exercised indirectly through other auto-release tests.
The `Refund` path has **zero test coverage**, meaning:

- A regression that breaks payer refunds in `auto_resolve` would not be caught.
- The threshold comparison (`funded_bps >= rule.min_funded_bps`) is not
  validated for the refund action.
- The "not auto-refunded below threshold" guard is unverified.

## Relevant Code

**Contract entry point** — `contracts/split/src/lib.rs` (function `auto_resolve`):

```rust
pub fn auto_resolve(env: Env, invoice_id: u64) {
    // ...
    let funded_bps = (invoice.funded as u128 * 10_000u128 / total as u128) as u32;

    for rule in invoice.auto_resolve_rules.clone().iter() {
        if funded_bps >= rule.min_funded_bps {
            match rule.action {
                ResolveAction::Release => { /* ... */ }
                ResolveAction::Refund => {
                    // aggregates payments per payer and transfers back
                    // sets invoice.status = InvoiceStatus::Refunded
                    // emits invoice_refunded + invoice_state_changed events
                }
            }
            return;
        }
    }

    panic!("no matching resolution rule");
}
```

**Types** — `contracts/split/src/types.rs`:

```rust
pub enum ResolveAction { Release, Refund }

pub struct ResolveRule {
    pub min_funded_bps: u32,   // e.g. 9000 = 90%
    pub action: ResolveAction,
}
```

**Invoice options** — `auto_resolve_rules` is a field on `InvoiceOptions`:

```rust
pub auto_resolve_rules: Vec<ResolveRule>,
```

## Acceptance Criteria

Four tests must be added to `contracts/split/src/test.rs`:

### 1. `test_auto_resolve_refund_above_threshold`

- Create an invoice for a total of 1 000 tokens.
- Set `auto_resolve_rules` to
  `[ResolveRule { min_funded_bps: 9000, action: ResolveAction::Refund }]`.
- Fund the invoice to **910 tokens** (91% — above the 9 000 bps threshold).
- Call `auto_resolve(invoice_id)`.
- Assert that `invoice.status == InvoiceStatus::Refunded`.
- Assert that the payer's token balance is restored to its pre-payment value
  (i.e. the contract transferred 910 tokens back to the payer).

### 2. `test_auto_resolve_refund_state_changed_event`

- Same setup and funding as test 1.
- After calling `auto_resolve`, verify that an `invoice_state_changed` event
  was emitted (`topic[1] == "st_chg"`).

### 3. `test_auto_resolve_refund_not_triggered_below_threshold`

- Create an invoice for a total of 1 000 tokens.
- Same `auto_resolve_rules` as above (threshold 9 000 bps / 90%).
- Fund the invoice to **890 tokens** (89% — below the 90% threshold).
- Calling `auto_resolve` must **panic** with `"no matching resolution rule"`.
- Assert that the invoice remains `Pending` and the payer's balance is still
  890 tokens lower than the initial minted amount (i.e. no refund happened).

### 4. `test_auto_resolve_refund_restores_multiple_payer_balances`

- Create an invoice for 1 000 tokens.
- Same `auto_resolve_rules` (threshold 9 000 bps).
- Two different payers each contribute 455 tokens (910 tokens total, 91%).
- Call `auto_resolve`.
- Assert `invoice.status == InvoiceStatus::Refunded`.
- Assert each payer's balance is fully restored (455 tokens each).

## Test Skeleton

Below is a ready-to-paste skeleton for the four tests. It follows the
project's existing helper conventions (`setup_initialized`, `make_invoice`,
`default_options`, `client`, `token_client`, the `topic1_is` helper already
present in `test.rs`).

```rust
// ---------------------------------------------------------------------------
// Issue: auto_resolve Refund path — tests for ResolveAction::Refund
// ---------------------------------------------------------------------------

#[test]
fn test_auto_resolve_refund_above_threshold() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    // Build a rule: refund when funded >= 90%.
    let mut rules: Vec<types::ResolveRule> = Vec::new(&env);
    rules.push_back(types::ResolveRule {
        min_funded_bps: 9_000,
        action: types::ResolveAction::Refund,
    });

    let mut opts = default_options(&env);
    opts.auto_resolve_rules = rules;

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let id = c.create_invoice(
        &creator, &recipients, &amounts, &token_id, &9_999_u64, &opts,
    );

    // Fund to 91% (910 of 1000).
    c.pay(&payer, &id, &910_i128, &0_u64, &false, &false, &None);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);

    c.auto_resolve(&id);

    // Invoice must be Refunded.
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Refunded);

    // Payer must have all 910 tokens returned (started with 1000, paid 910).
    assert_eq!(tk.balance(&payer), 1_000);
}

#[test]
fn test_auto_resolve_refund_state_changed_event() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    let mut rules: Vec<types::ResolveRule> = Vec::new(&env);
    rules.push_back(types::ResolveRule {
        min_funded_bps: 9_000,
        action: types::ResolveAction::Refund,
    });

    let mut opts = default_options(&env);
    opts.auto_resolve_rules = rules;

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let id = c.create_invoice(
        &creator, &recipients, &amounts, &token_id, &9_999_u64, &opts,
    );

    c.pay(&payer, &id, &910_i128, &0_u64, &false, &false, &None);
    c.auto_resolve(&id);

    // At least one invoice_state_changed event (Pending -> Refunded) must have fired.
    assert!(
        has_state_changed_event(&env),
        "invoice_state_changed event must be emitted by auto_resolve on Refund"
    );
}

#[test]
#[should_panic(expected = "no matching resolution rule")]
fn test_auto_resolve_refund_not_triggered_below_threshold() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    let mut rules: Vec<types::ResolveRule> = Vec::new(&env);
    rules.push_back(types::ResolveRule {
        min_funded_bps: 9_000,
        action: types::ResolveAction::Refund,
    });

    let mut opts = default_options(&env);
    opts.auto_resolve_rules = rules;

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let id = c.create_invoice(
        &creator, &recipients, &amounts, &token_id, &9_999_u64, &opts,
    );

    // Fund to only 89% (890 of 1000) — below the 9 000 bps threshold.
    c.pay(&payer, &id, &890_i128, &0_u64, &false, &false, &None);

    // Must panic — threshold not met.
    c.auto_resolve(&id);
}

#[test]
fn test_auto_resolve_refund_restores_multiple_payer_balances() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer1, &500);
    sa.mint(&payer2, &500);
    env.ledger().set_timestamp(1_000);

    let mut rules: Vec<types::ResolveRule> = Vec::new(&env);
    rules.push_back(types::ResolveRule {
        min_funded_bps: 9_000,
        action: types::ResolveAction::Refund,
    });

    let mut opts = default_options(&env);
    opts.auto_resolve_rules = rules;

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let id = c.create_invoice(
        &creator, &recipients, &amounts, &token_id, &9_999_u64, &opts,
    );

    // Two payers together hit 91% (455 + 455 = 910).
    c.pay(&payer1, &id, &455_i128, &0_u64, &false, &false, &None);
    c.pay(&payer2, &id, &455_i128, &0_u64, &false, &false, &None);

    c.auto_resolve(&id);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Refunded);
    assert_eq!(tk.balance(&payer1), 500, "payer1 must be fully refunded");
    assert_eq!(tk.balance(&payer2), 500, "payer2 must be fully refunded");
}
```

## Notes for Implementers

### `invoice_refunded` event signature

The `auto_resolve` path calls `events::invoice_refunded` with **two arguments**:

```rust
events::invoice_refunded(&env, invoice_id, total_refunded_amount);
```

This is the two-argument overload defined in `events.rs`. The zero-argument
overload (used elsewhere) has the same topic layout but no amount in the
data payload. Both are currently present in the codebase. Tests that check
for a `"refunded"` topic do not need to inspect the data payload to pass.

### Token availability

The contract holds the tokens transferred during `pay()`. On `auto_resolve` /
`Refund`, those same tokens are returned. In the test environment the mock
token contract automatically balances, so no additional `mint` calls beyond
the payer's initial mint are required.

### `min_funded_bps` check boundary

The comparison in `auto_resolve` is `funded_bps >= rule.min_funded_bps` (not
strictly greater), so:

| Funded | Total | `funded_bps` | Threshold | Triggers? |
|--------|-------|-------------|-----------|-----------|
| 910    | 1000  | 9100        | 9000      | **yes**   |
| 900    | 1000  | 9000        | 9000      | **yes**   |
| 890    | 1000  | 8900        | 9000      | **no**    |

The 89% test case (890/1 000) is deliberately chosen to sit just below the
threshold to guard against off-by-one errors.

### Panic message for the below-threshold test

```
"no matching resolution rule"
```

This is the exact string panicked by `auto_resolve` when no rule matches. The
`#[should_panic(expected = "...")]` annotation must match it verbatim.

## Verification

After adding these tests, run:

```
cargo test -p split
```

All four new tests should pass alongside the existing suite. No changes to
production code are required to make them pass — the `auto_resolve` / `Refund`
branch is already implemented; only tests are missing.
