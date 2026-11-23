# Issue #1118: Webhook system (status)

**Status:** mostly in place, with one defect fixed here and gaps listed below. The design is in
[`ISSUE_930_WEBHOOKS.md`](./ISSUE_930_WEBHOOKS.md) and the module is `contracts/split/src/webhooks.rs`. An
earlier status note, [`ISSUE_980_WEBHOOKS_STATUS.md`](./ISSUE_980_WEBHOOKS_STATUS.md), listed what still needed
checking. This note answers those points from the code.

A contract cannot make HTTP calls, so "delivery" has two forms. A subscriber that is a contract is called
directly. An off-chain subscriber is served through an event that a relayer watches, and the relayer reports
the result back.

## Requirements against the code

| Requirement | Where it is |
|---|---|
| Webhook schema | `Webhook` (owner, endpoint, optional target contract, `event_mask`, optional `invoice_id` filter, `active`, counters), `WebhookDelivery` and `DeliveryStatus` (`Pending`, `Delivered`, `Failed`). Events are bits: created 1, paid 2, released 4, refunded 8, cancelled 16. |
| Delivery | `webhooks::dispatch` finds every active webhook whose mask and invoice filter match, writes a `WebhookDelivery`, emits `(split, wh_send, webhook_id)` and, when a `target` is set, calls `on_webhook(delivery_id, invoice_id, event)` with `try_invoke_contract`. A failing target never reverts the invoice call. |
| Retry | A failed attempt is scheduled `backoff_ledgers(attempts)` ahead: 12, 24, 48 and so on, capped at 720 ledgers. `retry_webhook_delivery` can be called by anyone once the delay has passed. After `MAX_DELIVERY_ATTEMPTS` (5) the delivery is `Failed`. A webhook is switched off after `AUTO_DISABLE_FAILURES` (10) failed deliveries in a row. |
| Authentication | The owner must authorise `register_webhook`, `update_webhook`, `remove_webhook` and `ack_webhook_delivery`. `admin_remove_webhook` needs the Operator role. Only the owner can acknowledge a delivery. |
| Status tracking | Each delivery keeps `status`, `attempts`, `last_attempt_ledger` and `next_retry_ledger`. Each webhook keeps `delivered_count`, `failed_count` and `consecutive_failures`. The last 50 deliveries per webhook are listed by `list_webhook_deliveries`. Events `wh_reg`, `wh_send`, `wh_stat` and `wh_off` record changes. |

## The questions left open in #980

- **Does every lifecycle event have a delivery record?** `dispatch` is called from `notify_invoice` and
  from invoice creation. `notify_invoice` is called for payments (`_pay`, `pay_with_token`, `bridge_pay`,
  `delegate_pay`), for releases (`_release_full`, `_release_tranches`, `release_tranche`, `stage_release`),
  and for refunds (`refund`, `auto_resolve`). **Cancellation was the exception:** `EVENT_CANCELLED` and the
  mapping for it existed, but `cancel_invoice` never dispatched it, so a subscriber to bit 16 never got a
  delivery. This change adds the call in `cancel_invoice`. I did not check every payment entry point, for
  example `_pool_pay`, which is used by payment batches.
- **Does the backoff grow and is it capped?** Yes. `backoff_ledgers` doubles per attempt (the shift is
  limited to 16) and returns at most `RETRY_MAX_BACKOFF_LEDGERS`.
- **Is off-chain delivery done by a relayer?** Yes, and the relayer is not in this repository.

## Gaps that remain

- **No tests.** There are no webhook tests in `test.rs` or in `webhooks.rs`, so "all tests pass" says nothing
  about this module. Tests are not added in this PR, which is for a feature issue.
- **Authentication of the relayer and the endpoint is not built.** Nothing signs a delivery. An off-chain
  subscriber trusts the contract id that emitted `wh_send`, and the contract trusts any acknowledgement from the
  webhook owner. A shared secret cannot be kept on a public ledger, so a signature would have to be added by
  the relayer when it calls the endpoint.
- **`Delivered` for an endpoint means the owner said so.** `ack_webhook_delivery` is not checked against
  anything.
- **Cost.** `dispatch` loops over up to 20 webhooks inside the invoice call and calls each target contract
  there. A failing target is caught, but its instructions still count against the call.
- **A target must implement `on_webhook`.** There is no check at registration that it does.

## What this change does

Adds the `EVENT_CANCELLED` dispatch to `cancel_invoice`, and records the status above. The change was not
built or run.
