# Issue #1107: Debug event filtering (status)

**Status:** filtering by event type and by invoice already works through the event topics. Filtering by
creator or by status needs the event data, so it is done by the client, not by the node.

## How events are shaped

Most events follow `(split, <event name>, invoice_id)` as topics, with the details in the data. The
filters a Stellar RPC `getEvents` call applies are on topics, so:

| Filter | How |
|---|---|
| By type | Match the second topic. For example `created`, `st_chg` (state change), `PayoutFailed`, `grd_fail` (guard failure), `wh_stat` (webhook status). |
| By invoice | Match the third topic, which is the invoice id for the invoice events. |
| By creator | Not a topic. `created` carries the creator first in its data `(creator, total, cross_chain_ref, event_seq)`, and `invoice_state_changed` carries the actor. The client reads the data. |
| By status | Not a topic. `st_chg` has data `(from_status, to_status, actor, ledger)`, where `to_status` is one of `pending`, `released`, `refunded`, `expired`, `cancld`, `disputed`, `part_rel`, `finald`, `deleted` or `pay_prg`. The client reads the data. |

## Requirements against the code

| Requirement | Result |
|---|---|
| Filter system and logic | Topic filtering as above. Complex filters (type and creator and status together) are built by the client from `created` and `st_chg` events. |
| Filter API | The Soroban RPC `getEvents` request, not a contract function. On chain, `register_webhook` takes an `event_mask` of `EVENT_CREATED`, `EVENT_PAID`, `EVENT_RELEASED`, `EVENT_REFUNDED` and `EVENT_CANCELLED`, and an optional invoice id. |
| Track filter usage | Not present. Reads do not write state, so a contract cannot count how often events are queried. |

## What would help

Putting the creator and the new status in the topics (for example `(split, st_chg, invoice_id, to_status)`)
would let a node filter on them. That changes the topic layout that indexers depend on, and is left out
of this change.

## What this change does

Records the status above. It changes no code. The behaviour was read, not run, and no tests were added.
