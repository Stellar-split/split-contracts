# Code examples for the invoice escrow

Usage examples for the public functions of `contracts/invoice-escrow`. Each example shows one call
and what it returns. The examples are not compiled by CI, so check them against the signatures in
`contracts/invoice-escrow/src/lib.rs` before you copy them into a project.

## Setup

```rust
let client = InvoiceEscrowClient::new(&env, &contract_id);
client.initialize(&admin);
```

`initialize` can run once. A second call returns `AlreadyInitialized`.

## Create an invoice

```rust
let invoice_id: u64 = client.create_invoice(
    &creator,
    &token,
    &1_000_000_i128,       // total_amount, must be greater than zero
    &deadline_unix_secs,   // must be in the future
);
```

The creator must authorise the call. A zero total returns `InvalidTotalAmount`.

## Fund an invoice

```rust
client.deposit(&payer, &invoice_id, &250_000_i128);
// or, with the same arguments:
client.pay_invoice(&payer, &invoice_id, &250_000_i128);
```

A deposit that would take the invoice above its total returns `OverFunded`.

## Release the funds

```rust
client.release(&invoice_id);
```

Release needs a fully funded invoice (`InsufficientFunding` otherwise) and, when co-signers are set,
enough approvals (`ApprovalRequired`).

## Refund payers after the deadline

```rust
client.refund(&invoice_id, &vec![&env, payer_a, payer_b]);
```

Refund needs the deadline to have passed (`DeadlineNotPassed` otherwise) and any refund grace period
to have ended (`RefundGraceActive`).

## Read an invoice

```rust
let invoice = client.get_invoice(&invoice_id); // Err(InvoiceNotFound) for an unknown ID
let funded = client.get_escrow_balance(&invoice_id);
```

## Schedule a release

```rust
client.schedule_release(&invoice_id, &release_at_unix_secs, &min_ledgers);
```

The release time must be in the future (`InvalidSchedule` otherwise). `release` then returns
`ReleaseNotYetDue` until that time has passed.

## Co-signer approval

```rust
client.approve_release(&signer, &invoice_id); // returns the ApprovalStatus
client.has_approved(&invoice_id, &signer);    // bool
```

A signer who is not configured for the invoice gets `NotCosigner`. A second vote from the same
signer gets `AlreadyVoted`.
