# Issue #1119: Invoice group batching (status)

**Status:** partly in place. Creators can group invoices and the group controls release. Two defects in the
group code are fixed here. Group-level processing and status are not built.

## What exists

The repository has three separate "batch" features. Only the first is a group of invoices owned by a creator.

| Feature | Entry points | What it does |
|---|---|---|
| Invoice groups (#212, #434) | `create_invoice_group(invoice_ids, majority)`, `get_group_members_list`, `get_invoice_group_id_info` | Links two or more invoices. In `AllOrNothing` mode none of them can be released until every member is fully funded. In `Majority` mode more than half must be fully funded. The check is in the release path (`group members not fully funded`, `group majority not funded`). A grouped invoice is not auto-released on funding, so release is a manual call. |
| Batch creation | `create_batch` (at most 5 invoices), `create_invoices_batch` (up to 10, all or none) | Creates several invoices in one call. It does not link them. |
| Payment batches (#925) | `open_payment_batch`, `add_to_payment_batch`, `remove_from_payment_batch`, `cancel_payment_batch`, `execute_payment_batch`, `get_payment_batch`, `set_payment_batch_limit`, `get_batching_stats` | Lets one payer collect payments to several invoices and settle them with one token transfer. Status is `Open`, `Executed` or `Cancelled`. Up to 25 items by default. |

## Requirements against the code

| Requirement | Result |
|---|---|
| Design batching system | Done for groups, see above. The record is `InvoiceGroup { invoice_ids, mode }` under the key `grp`, with an `invgrp` key per invoice. |
| Batch creation | `create_invoice_group`. It needs at least two invoices. |
| Batch processing | Release is gated by the group, one invoice at a time. There is no call that releases a whole group, and `rollback_invoice_group` (refunds every pending member) is marked `dead_code` and nothing calls it. |
| Batch queries | `get_group_members_list` and `get_invoice_group_id_info`. See the first defect below. |
| Track batch status | Not built for groups. A group has no status field. A caller has to read each member's status. Payment batches do have one. |

## Defects fixed in this change

1. **The two group queries returned nothing for real groups.** They read the keys `grp_mem` and `inv_grp`,
   which only `create_group_for_invoices` writes, and that function is `dead_code` and never called.
   `create_invoice_group` writes `grp` and `invgrp`. So `get_group_members_list` always returned an empty list
   and `get_invoice_group_id_info` always returned `None`. Both now read the records `create_invoice_group`
   writes first, and fall back to the old keys.
2. **Anyone could group anyone's invoices.** `create_invoice_group` had no authorisation and no check on the
   invoices. A caller could put another creator's invoice into an `AllOrNothing` group with an invoice that is
   never funded, and that invoice could not be released. It now requires the authorisation of the creator of
   the first invoice, and every invoice must have that creator (`group invoices must share a creator`). A
   missing invoice already panics in `load_invoice`. The existing group tests group invoices made by one
   creator, so they are not affected.

## Gaps that remain

- No way to remove an invoice from a group or dissolve a group. Regrouping an invoice overwrites its group
  pointer, and the old group still lists it.
- A group can hold duplicate ids, and there is no upper limit on its size. A large group makes every release
  loop over all members, see [`PERFORMANCE_TUNING.md`](./PERFORMANCE_TUNING.md).
- No group status, no group release and no group refund.
- No tests are added, since this is a feature issue. There are tests for release gating, but none for the
  two queries.

## What this change does

Fixes the two defects above and records the status. The change was not built or run.
