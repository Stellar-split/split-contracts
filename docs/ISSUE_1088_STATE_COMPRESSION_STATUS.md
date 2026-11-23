# Issue #1088: Contract state compression (status)

**Status:** partly covered. Two compression mechanisms exist, but neither is a lossless codec for
invoice data, and nothing measures storage savings on chain.

## Requirements against the code

| Requirement | Where it is | Result |
|---|---|---|
| Compression scheme | `compress_payments(invoice_id)` in `contracts/split/src/lib.rs` merges all payments from the same payer into one record | See the loss note below. |
| Encode/decode | `contracts/split/src/compression.rs` (issue #1021): `compress`, `decompress`, `negotiate`. Run-length encoding with a raw fallback, wire format `[encoding byte][body]` | Present as a byte-level codec. |
| Decompression | `compression::decompress` | Present. Nothing in the contract stores invoices through it. |
| Savings measured | `compression::CompressionMetrics::savings_bps`. Also `storage_optimizer.rs` (`StorageMetrics`, `optimize`) | Present in the library code only. `compress_payments` records no savings. |
| `compact_migrate(invoice_id)` | Writes a one-byte status and a deadline-as-ledger overlay next to the invoice, for the hot paths | Present. It adds data, and does not remove the original. |

## What `compress_payments` loses

`compress_payments` keeps each payer's total amount and total tip, and asserts the total funded is
unchanged. It rewrites every merged payment with `attestation_hash: None`, `donate_on_failure: false`
and the ledger and timestamp of the compression call. After it runs:

- the time of each original payment is gone,
- an attestation hash and the `donate_on_failure` flag on an earlier payment are dropped.

So the "no data loss" criterion holds for money totals only. A caller who needs the payment-by-payment
history, for example for dispute evidence, should not compress that invoice.

`compress_payments` also has no authorization check. It calls `require_not_paused` and nothing else, so
any account can compress any invoice.

## What this change does

Records the status above. It changes no code. The behaviour was read, not run, and no tests were added.
