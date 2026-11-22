# Performance tuning guide

This guide is for developers who call or change `contracts/split` and want to keep each operation
inside its compute and storage budget. The measured budgets are in
[`COMPUTE_BUDGETS.md`](./COMPUTE_BUDGETS.md).

## 1. Measure before you change anything

- Use the read-only `estimate_compute` entry point to get the instruction, memory and read/write
  figures for an operation. It runs a simulation, so it costs no fee.
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

## Checklist before a change is merged

- [ ] `estimate_compute` figure compared with the baseline.
- [ ] No repeated read of the same key.
- [ ] No unbounded loop added.
- [ ] Baseline updated in `COMPUTE_BUDGETS.md` if the change is intentional.
