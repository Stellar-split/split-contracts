# Issue #930: Webhook Notification System

## Overview

Lets external services subscribe to invoice lifecycle events. Soroban contracts cannot make outbound HTTP calls, so webhooks are modelled as an on-chain subscription registry plus a delivery ledger:

- **Off-chain endpoints**: every delivery emits a `(split, wh_send, webhook_id)` event. A relayer watches these events, POSTs to the registered `endpoint`, and reports the result with `ack_webhook_delivery`.
- **On-chain targets**: if a `target` contract is set, it is called directly with `on_webhook(delivery_id: u64, invoice_id: u64, event: u32)` through `try_invoke_contract`. A failing subscriber **never** reverts the invoice operation that triggered it.

Implementation: `contracts/split/src/webhooks.rs`.

## Events and filtering

| Bit | Constant | Fired when |
|-----|----------|------------|
| `1` | `EVENT_CREATED` | Invoice created |
| `2` | `EVENT_PAID` | Payment received |
| `4` | `EVENT_RELEASED` | Funds released to recipients |
| `8` | `EVENT_REFUNDED` | Invoice refunded |
| `16` | `EVENT_CANCELLED` | Invoice cancelled before payment |

`event_mask` is the bitwise OR of the events a webhook wants (`EVENT_ALL = 31`). `invoice_id = Some(id)` restricts a webhook to one invoice; `None` subscribes to every invoice. Inactive webhooks receive nothing.

## Entry points

| Function | Auth | Description |
|----------|------|-------------|
| `register_webhook(owner, endpoint, target, event_mask, invoice_id) -> u64` | owner | Register a webhook |
| `update_webhook(owner, webhook_id, endpoint, target, event_mask, invoice_id, active)` | owner | Change config or pause/resume |
| `remove_webhook(owner, webhook_id)` | owner | Delete a webhook |
| `admin_remove_webhook(admin, webhook_id)` | Operator+ | Force-remove an abusive webhook |
| `get_webhook(webhook_id)` / `list_webhooks()` / `list_owner_webhooks(owner)` | — | Read webhooks |
| `get_webhook_delivery(delivery_id)` / `list_webhook_deliveries(webhook_id)` | — | Read delivery status |
| `retry_webhook_delivery(delivery_id) -> DeliveryStatus` | anyone | Retry a pending delivery once due |
| `ack_webhook_delivery(owner, delivery_id, success) -> DeliveryStatus` | owner | Report an endpoint delivery result |

## Retries and status tracking

Each delivery is a `WebhookDelivery { status, attempts, last_attempt_ledger, next_retry_ledger, … }` with status `Pending`, `Delivered` or `Failed`.

- After a failed attempt the next retry is due after `RETRY_BASE_LEDGERS * 2^(attempts-1)` ledgers (12, 24, 48, …), capped at `RETRY_MAX_BACKOFF_LEDGERS` (720).
- After `MAX_DELIVERY_ATTEMPTS` (5) the delivery becomes `Failed`.
- Endpoint deliveries are scheduled for retry immediately, so a delivery the relayer never acknowledges is retried too.
- A delivery whose webhook was removed or disabled is marked `Failed` on its next retry.
- Each webhook tracks `delivered_count`, `failed_count` and `consecutive_failures`; it is auto-disabled (`wh_off` event) after `AUTO_DISABLE_FAILURES` (10) consecutive failed deliveries.

## Limits

| Constant | Value | Reason |
|----------|-------|--------|
| `MAX_WEBHOOKS` | 20 | Bounds the per-event dispatch cost |
| `MAX_WEBHOOKS_PER_OWNER` | 5 | Stops one account filling the registry |
| `MAX_ENDPOINT_LEN` | 256 bytes | Bounds storage |
| `MAX_TRACKED_DELIVERIES` | 50 per webhook | Bounds `list_webhook_deliveries` |

## Emitted events

| Topics | Data |
|--------|------|
| `(split, wh_reg, webhook_id)` | `(owner, event_mask)` |
| `(split, wh_upd, webhook_id)` | `(event_mask, active)` |
| `(split, wh_del, webhook_id)` | `caller` |
| `(split, wh_send, webhook_id)` | `(delivery_id, invoice_id, event, attempt, endpoint)` |
| `(split, wh_stat, webhook_id)` | `(delivery_id, status, attempts, next_retry_ledger)` |
| `(split, wh_off, webhook_id)` | `consecutive_failures` |
