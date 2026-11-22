# Issue #977: Invoice archival (status)

**Status:** the feature exists. `archive_invoice` and `archive_invoices_batch` in
`contracts/split/src/lib.rs` archive invoices. Storage-tier and key behaviour are described in
[`STORAGE_KEY_REGISTRY.md`](../STORAGE_KEY_REGISTRY.md).

## What this change does

Records the status above. It changes no code and adds no tests.

## What still needs checking

- Which invoice states may be archived. An active invoice with funds in escrow must not be archivable.
- Whether an archived invoice stays readable, and whether its data is moved or only marked.
- Whether archival is reversible, and if it is not, whether the operation is admin-only.

## Not done

No review of the archival logic, no new tests.
