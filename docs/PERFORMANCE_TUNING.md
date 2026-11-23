# Performance tuning guide

This guide is for developers who call or change `contracts/split` and want to keep each operation
inside its compute and storage budget. The measured budgets are in
[`COMPUTE_BUDGETS.md`](./COMPUTE_BUDGETS.md).

## 1. Measure before you change anything

- Use the read-only `estimate_compute` entry point to get the instruction, memory and fee figures for
  an operation. It is a static estimator built from modeled constants, not a simulation of the call, so
  treat it as a budget check. `estimate_operation_cost` uses a per-operation cost model that can be
  calibrated from measured usage, see section 8.
- Compare the figure with the baseline in `COMPUTE_BUDGETS.md`. A change that moves a figure should
  be explained in the pull request.
- Wall-clock time in native tests says little about on-chain cost. Use the simulation figures.

## 2. Keep reads and writes low

- Read a storage entry once per call and reuse the value. Repeated reads of the same key cost more
  than one read.
- Write only what changed. Skip a write when the new value equals the stored one.
- Prefer one write of a composite value over several writes of its fields, unless the fields are
  updated independently.

## 3. Choose the storage tier by access pattern

Hot singletons belong in instance storage, so the common read avoids a persistent lookup. Rebuildable
data can go in temporary storage. See [`ADR 0001`](./adr/0001-storage-tiers.md).

## 4. Keep key construction cheap

Storage keys are built on every invoice access. Use the precomputed `symbol_short!` prefixes and
fixed-size buffers described in [`STORAGE_KEY_REGISTRY.md`](../STORAGE_KEY_REGISTRY.md). Do not allocate
a new `Vec` for each key.

## 5. Batch where the operation allows it

Batched payments amortise the fixed cost of one call across several payments. See the batching notes
in [`ISSUE_925_PAYMENT_BATCHING.md`](./ISSUE_925_PAYMENT_BATCHING.md). Batches are bounded, so a
batch that is too large is refused rather than run over budget.

## 6. Avoid unbounded loops

A loop whose length depends on stored data (for example, all payers of an invoice) grows with the
data. Cap it, or page through it, and check the cap against the budget.

## 7. Build settings and contract size

- The workspace root `Cargo.toml` sets the release profile: `opt-level = "z"` (smallest code),
  `lto = true`, `codegen-units = 1`, `panic = "abort"`, `strip = "symbols"` and `overflow-checks = true`.
  Keep `overflow-checks` on, a silent wrap is worse than a few extra instructions.
- `contracts/split/Cargo.toml` repeats a `[profile.release]` block. Cargo only reads profiles from the
  workspace root and ignores them in a member crate, so change the root block, not that one.
- Code that is never called still costs bytes once it is reachable from a `#[contractimpl]`. Do not add an
  entry point for a helper that only tests use.
- `release-with-logs` turns on debug assertions for debugging. Do not deploy it.

## 8. Calibrating the cost model

The estimators start from modeled numbers. To make them match the network:

1. Simulate the real transaction with your RPC and note the instruction count.
2. Report it with `record_compute_usage` (Operator role or higher).
3. Read `get_compute_cost_report` for the average actual and estimated instructions and the accuracy.
4. If the model is far off, a SuperAdmin adjusts it with `set_operation_cost_model`.

See [`ISSUE_1095_COMPUTE_BUDGET_ESTIMATION_STATUS.md`](./ISSUE_1095_COMPUTE_BUDGET_ESTIMATION_STATUS.md) for the
fields. A transaction's real cost is not visible to the contract, so step 1 happens off-chain.

## 9. Size limits that protect the budget

These caps keep one call from growing past its budget. Stay well inside them in your own integration:

| Limit | Value | Where |
|---|---|---|
| Recipients per invoice | 200 | `MAX_INVOICE_RECIPIENTS` in `validation.rs` |
| Split rules per invoice | 32 | `MAX_SPLIT_RULES` in `constants.rs` |
| Invoices per `create_batch` call | 5 | an `assert!` in `create_batch` |
| Invoices per `archive_invoices_batch` or `purge_archived_invoices` call | 20 | `assert!` in each |
| Items in a payment batch | 25 by default | `DEFAULT_PAYMENT_BATCH_LIMIT`, changeable by an admin |

A call with 200 recipients is the heavy case: release does one token transfer per recipient, so its cost
grows with the list. Prefer several smaller invoices when the recipients do not have to share one.

## 10. Storage lifetime

Each persistent entry has a time to live and costs rent while it lives. `storage.rs` has the helpers
`bump_instance_ttl`, `bump_persistent_ttl` and `bump_temporary_ttl`, and `constants.rs` sets the invoice TTL
range (`MIN_INVOICE_TTL_LEDGERS`, `MAX_INVOICE_TTL_LEDGERS`). Bump an entry when you read it for a purpose
that needs it to stay, not on every read, and let finished invoices expire or be archived.

## 11. Common mistakes

| Mistake | Cost | Instead |
|---|---|---|
| Calling `get_invoice` for each field you need | One full read each time | Read once and use the fields |
| Polling a read-only entry point in a tight loop | RPC load, no on-chain fee | Listen to events and read on change |
| Building a `Vec` of all invoices to find one | Grows with the data | Use the paged getters or the id you stored |
| Sending 25 one-off payments | 25 fixed call costs | Use a payment batch |
| Trusting the built-in estimate for a new operation | Model constant, not a measure | Calibrate as in section 8 |

## Checklist before a change is merged

- [ ] `estimate_compute` figure compared with the baseline.
- [ ] No repeated read of the same key.
- [ ] No unbounded loop added.
- [ ] Baseline updated in `COMPUTE_BUDGETS.md` if the change is intentional.
- [ ] No new entry point added only for tests, and the release profile left as it is.
