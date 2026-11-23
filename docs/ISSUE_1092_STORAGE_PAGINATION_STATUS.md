# Issue #1092: Storage pagination (status)

**Status:** cursor and offset pagination exist. Sorting and pagination statistics do not.

## Requirements against the code

| Requirement | Where it is | Result |
|---|---|---|
| Pagination system | `list_invoices`, `get_payer_history`, `get_recipient_invoice_ids_page` in `contracts/split/src/lib.rs` | Present. |
| Cursor-based | `list_invoices(cursor, limit)` returns ids greater than `cursor`, in ascending order, up to `limit` | Present. Pass the last id returned as the next cursor. |
| Limit/offset | `get_payer_history(payer, offset, limit)` and `get_recipient_invoice_ids_page(recipient, offset, limit)` | Present. |
| Sorting | Results come back in storage order: ascending id for `list_invoices`, insertion order for the other two. `types.rs` defines `InvoiceSortOrder` (id, deadline and funded amount, each ascending or descending, with ties broken by id), `InvoiceCursor` and `PaginatedInvoices` (issue #944), but no entry point takes or returns them. | No caller-chosen sort is reachable. |
| Pagination stats | None | Not present. |
| No duplicates | `list_invoices` only moves forward from the cursor | Holds for `list_invoices`. See the note on offset paging. |

## Limits

- `list_invoices`: `limit` is clamped to `MAX_PAGE_LIMIT` (100). `limit == 0` returns an empty list.
- `get_recipient_invoice_ids_page`: `limit` is clamped to 50. An `offset` past the end returns an empty list.
- `get_payer_history`: no clamp. `offset` and `limit` are applied with `min`, so a very large `limit`
  returns the whole history in one call.

## Notes for callers

- `list_invoices` skips ids that no longer exist, so a page can hold fewer than `limit` ids while more
  remain. Stop when a page comes back empty.
- Offset paging over a list that grows between calls can repeat or skip an entry. Prefer the cursor form.
- `get_payer_history` has no upper bound on `limit`. Reading a long history in one call can exceed the
  transaction's resource limits, so clients should request pages of 50 or fewer.

## What would finish it

An entry point that takes an `InvoiceSortOrder` and an optional `InvoiceCursor`, and returns a
`PaginatedInvoices`, would give sorting and a stop signal (`next_cursor` is empty on the last page). The
types already describe that contract. It is not added here because it needs a new entry point in `lib.rs`.

## What this change does

Records the status above. It changes no code. The behaviour was read, not run, and no tests were added.
