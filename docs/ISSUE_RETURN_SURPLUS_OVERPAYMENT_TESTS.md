# Issue: Integration Tests for `OverfundingPolicy::ReturnSurplus` Overpayment

## Background

`OverfundingPolicy::ReturnSurplus` was introduced in issue #420.  When this policy
is active, `_pay` computes the portion of the incoming payment that fits under
the invoice's `total` target and immediately transfers the remainder back to the
payer—without waiting for a release:

```rust
// contracts/split/src/lib.rs — inside _pay()
OverfundingPolicy::ReturnSurplus => {
    // `remaining` can be negative if an earlier `AcceptAll` phase
    // overshot the target, so clamp before comparing.
    amount.min(remaining.max(0))
}
...
// After token transfer:
if invoice.overfunding_policy == OverfundingPolicy::ReturnSurplus && excess > 0 {
    token_client.transfer(&env.current_contract_address(), payer, &excess);
}
```

There are currently **no integration tests** that verify:

1. the surplus (`excess`) is transferred back to the payer during the same call,
2. `invoice.funded` is capped at `total` and never exceeds it, and
3. only the expected events (`payment_received` + a surplus-refund transfer) are
   emitted—no extra state-change events that would indicate incorrect lifecycle
   behaviour.

## Acceptance Criteria

| # | Criterion |
|---|-----------|
| AC-1 | Payer sends `total + 100` stroops; after the call the payer's token balance has increased by exactly `100` compared to after the transfer. |
| AC-2 | `invoice.funded == total` after the overpayment (not `total + 100`). |
| AC-3 | Exactly one `payment_received` event is emitted for `amount = total`; no other state-change events (`released`, `refunded`, etc.) are emitted during the overpayment call itself. |
| AC-4 | `cargo test` passes with the new tests added. |

## Target File

- **Primary implementation**: `contracts/split/src/lib.rs` — `_pay()` function
  (search for `OverfundingPolicy::ReturnSurplus`)
- **Test file to extend**: `contracts/split/src/test.rs`

## Proposed Test Structure

### Helper setup (reusable across tests)

```rust
// In contracts/split/src/test.rs

/// Returns (env, contract_id, token_admin, creator, payer, token).
/// The contract is initialised; the token is minted to `payer` with
/// a large balance so overflow tests can over-send freely.
fn setup_return_surplus_invoice() -> (Env, Address, Address, Address, Address, Address, u64) {
    // 1. Create Env, register token + split contracts.
    // 2. Call initialize() with zero platform fee for simplicity.
    // 3. Create an invoice with OverfundingPolicy::ReturnSurplus.
    //    - Two recipients, amounts [600, 400] → total = 1_000
    // 4. Mint 10_000 to payer.
    // 5. Return all handles.
}
```

### Test 1 — surplus is refunded immediately

```rust
#[test]
fn test_return_surplus_refunds_excess_to_payer() {
    let (env, contract, _admin, _creator, payer, token, invoice_id) =
        setup_return_surplus_invoice();

    let total = 1_000_i128;
    let overpayment = total + 100;

    // Balance before paying
    let balance_before = token_client.balance(&payer);

    // Pay total + 100
    client.pay(&payer, &invoice_id, &overpayment, &0, &false, &false, &None);

    let balance_after = token_client.balance(&payer);

    // AC-1: payer's net outflow should be exactly `total`, not `total + 100`
    assert_eq!(
        balance_before - balance_after,
        total,
        "payer should only be charged `total`; surplus must be refunded"
    );
}
```

### Test 2 — `funded` never exceeds `total`

```rust
#[test]
fn test_return_surplus_funded_does_not_exceed_total() {
    let (env, contract, _admin, _creator, payer, _token, invoice_id) =
        setup_return_surplus_invoice();

    let total = 1_000_i128;

    // Send more than total
    client.pay(&payer, &invoice_id, &(total + 500), &0, &false, &false, &None);

    // AC-2: funded must be capped at total
    let funded = client.get_invoice_funded(&invoice_id).unwrap();
    assert_eq!(
        funded, total,
        "funded must equal total after an overpayment under ReturnSurplus"
    );
}
```

### Test 3 — only `payment_received` event is emitted (no extra state changes)

```rust
#[test]
fn test_return_surplus_emits_only_payment_received_event() {
    let (env, contract, _admin, _creator, payer, _token, invoice_id) =
        setup_return_surplus_invoice();

    let total = 1_000_i128;

    client.pay(&payer, &invoice_id, &(total + 100), &0, &false, &false, &None);

    // AC-3: collect all events; only payment_received should appear for this invoice.
    // A `RefundIssued` (from events::refund_issued) may accompany the overpayment
    // when overflow_behavior == Refund, but ReturnSurplus uses a direct transfer,
    // not that helper—so no RefundIssued event is expected here.
    // The invoice is auto-released once funded reaches total, so `released` IS
    // expected if no guards are set.  The test must verify `funded == total` to
    // distinguish a correct cap from an uncapped funded value.
    //
    // If the invoice has guards (tranches, prerequisite, etc.), no `released`
    // event fires and ONLY `payment_received` should appear.
    //
    // Suggested: create the invoice with a prerequisite so auto-release is
    // blocked, then assert exactly one `payment_received` event.
}
```

> **Note on auto-release**: Because `_pay` auto-releases once `invoice.funded >= total`
> (when no guards are present), a test that wants to isolate the event list
> should either:
> (a) use a prerequisite or tranche to block auto-release, or
> (b) explicitly accept that `invoice_released` also fires and only assert
>     that no *unexpected* state-change events (e.g. `refunded`) appear.

### Test 4 — multiple overpayments, `funded` stays at `total`

```rust
#[test]
fn test_return_surplus_multiple_overpayments_keep_funded_at_total() {
    // Setup invoice with a prerequisite to block auto-release.
    // Pay total + 100, then pay another 50.
    // Assert funded == total after each call.
    // Assert second payment surplus (50) is fully refunded.
}
```

## Key Code Locations

| Symbol | File | Notes |
|--------|------|-------|
| `OverfundingPolicy::ReturnSurplus` | `contracts/split/src/types.rs` | Enum variant |
| `_pay()` — surplus computation | `contracts/split/src/lib.rs` | ~line containing `OverfundingPolicy::ReturnSurplus =>` |
| `_pay()` — surplus transfer | `contracts/split/src/lib.rs` | `if invoice.overfunding_policy == OverfundingPolicy::ReturnSurplus && excess > 0` |
| `set_overfunding_policy()` | `contracts/split/src/lib.rs` | Sets policy before first payment |
| `get_invoice_funded()` | `contracts/split/src/lib.rs` | Read funded from hot storage |
| `events::payment_received` | `contracts/split/src/events.rs` | Expected event |
| `events::refund_issued` | `contracts/split/src/events.rs` | NOT expected under ReturnSurplus |

## Related Issues

- **#420** — original `OverfundingPolicy` implementation (`Cap`, `AcceptAll`, `ReturnSurplus`)
- **#470** — `contribute()` entry point, which implements a similar surplus-refund
  pattern but for a different code path

## Out of Scope

- Testing `OverfundingPolicy::Cap` or `OverfundingPolicy::AcceptAll` — those
  are covered elsewhere.
- Testing the `contribute()` path — that is a separate entry point.
- Any changes to `lib.rs` production code — this issue is **documentation and
  tests only**.
