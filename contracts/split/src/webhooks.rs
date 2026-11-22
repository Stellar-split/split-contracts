//! Issue #930: Webhook notification system.
//!
//! Soroban contracts cannot make outbound HTTP calls, so "webhooks" are modelled
//! as an on-chain subscription registry plus a delivery ledger:
//!
//! - **Registration** — an owner registers a webhook with an off-chain
//!   `endpoint` (e.g. an HTTPS URL served by a relayer) and/or an on-chain
//!   `target` contract that implements `on_webhook(delivery_id, invoice_id, event)`.
//! - **Filtering** — each webhook carries an `event_mask` (bitwise OR of the
//!   `EVENT_*` constants) and an optional `invoice_id` scope, so subscribers only
//!   receive the lifecycle events they care about.
//! - **Delivery** — every matching lifecycle event creates a [`WebhookDelivery`]
//!   record and emits a `(split, wh_send, webhook_id)` event that off-chain
//!   relayers consume. When a `target` contract is set it is invoked directly
//!   with `try_invoke_contract`, so a failing subscriber can never revert the
//!   invoice operation that triggered it.
//! - **Retries** — failed deliveries are rescheduled with exponential backoff
//!   (`RETRY_BASE_LEDGERS * 2^(attempt-1)`, capped at `RETRY_MAX_BACKOFF_LEDGERS`)
//!   and can be retried by anyone once due. After `MAX_DELIVERY_ATTEMPTS` the
//!   delivery is marked `Failed`.
//! - **Status tracking** — off-chain relayers report the outcome of endpoint
//!   deliveries through `ack_delivery`; per-webhook success/failure counters are
//!   kept, and a webhook is auto-disabled after `AUTO_DISABLE_FAILURES`
//!   consecutive failed deliveries.
//!
//! The number of registered webhooks is capped at `MAX_WEBHOOKS` so the
//! per-event dispatch loop has a bounded instruction cost.

use soroban_sdk::{contracttype, symbol_short, Address, Env, IntoVal, String, Symbol, Val, Vec};

// ---------------------------------------------------------------------------
// Event filter bits
// ---------------------------------------------------------------------------

/// Invoice created.
pub const EVENT_CREATED: u32 = 1 << 0;
/// Payment received on an invoice.
pub const EVENT_PAID: u32 = 1 << 1;
/// Invoice funds released to recipients.
pub const EVENT_RELEASED: u32 = 1 << 2;
/// Invoice refunded to payers.
pub const EVENT_REFUNDED: u32 = 1 << 3;
/// Invoice cancelled before any payment.
pub const EVENT_CANCELLED: u32 = 1 << 4;
/// Every supported event.
pub const EVENT_ALL: u32 =
    EVENT_CREATED | EVENT_PAID | EVENT_RELEASED | EVENT_REFUNDED | EVENT_CANCELLED;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum number of registered webhooks (bounds the dispatch loop).
pub const MAX_WEBHOOKS: u32 = 20;
/// Maximum number of webhooks a single owner may register.
pub const MAX_WEBHOOKS_PER_OWNER: u32 = 5;
/// Maximum endpoint length in bytes.
pub const MAX_ENDPOINT_LEN: u32 = 256;
/// Maximum delivery attempts (initial attempt included) before giving up.
pub const MAX_DELIVERY_ATTEMPTS: u32 = 5;
/// Backoff before the first retry, in ledgers (~1 minute at ~5 s/ledger).
pub const RETRY_BASE_LEDGERS: u32 = 12;
/// Upper bound on the backoff between retries, in ledgers (~1 hour).
pub const RETRY_MAX_BACKOFF_LEDGERS: u32 = 720;
/// Consecutive failed deliveries after which a webhook is auto-disabled.
pub const AUTO_DISABLE_FAILURES: u32 = 10;

/// Function invoked on a webhook's `target` contract.
const TARGET_FN: &str = "on_webhook";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A registered webhook subscription.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Webhook {
    pub id: u64,
    pub owner: Address,
    /// Off-chain endpoint consumed by relayers (may be empty when `target` is set).
    pub endpoint: String,
    /// Optional on-chain subscriber implementing `on_webhook`.
    pub target: Option<Address>,
    /// Bitwise OR of `EVENT_*` constants.
    pub event_mask: u32,
    /// Restrict deliveries to a single invoice; `None` subscribes to all invoices.
    pub invoice_id: Option<u64>,
    pub active: bool,
    pub created_at: u64,
    pub delivered_count: u64,
    pub failed_count: u64,
    pub consecutive_failures: u32,
}

/// Lifecycle status of a single delivery.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryStatus {
    /// Waiting for a relayer acknowledgement or a scheduled retry.
    Pending,
    /// Delivered successfully.
    Delivered,
    /// Gave up after `MAX_DELIVERY_ATTEMPTS`, or the webhook was removed/disabled.
    Failed,
}

/// Record of one event delivery to one webhook.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebhookDelivery {
    pub id: u64,
    pub webhook_id: u64,
    pub invoice_id: u64,
    /// The single `EVENT_*` bit that triggered this delivery.
    pub event: u32,
    pub status: DeliveryStatus,
    pub attempts: u32,
    pub created_ledger: u32,
    pub last_attempt_ledger: u32,
    /// Ledger at or after which a retry may be attempted (0 when not scheduled).
    pub next_retry_ledger: u32,
}

#[contracttype]
#[derive(Clone)]
enum WebhookKey {
    /// Next webhook id (instance).
    NextWebhookId,
    /// Next delivery id (instance).
    NextDeliveryId,
    /// Ids of all registered webhooks (instance).
    WebhookIds,
    /// Webhook record by id (persistent).
    Webhook(u64),
    /// Webhook ids owned by an address (persistent).
    OwnerWebhooks(Address),
    /// Delivery record by id (persistent).
    Delivery(u64),
    /// Delivery ids for a webhook, most recent last (persistent).
    WebhookDeliveries(u64),
}

/// Number of delivery ids retained per webhook for `list_deliveries`.
const MAX_TRACKED_DELIVERIES: u32 = 50;

// ---------------------------------------------------------------------------
// Storage helpers
// ---------------------------------------------------------------------------

fn next_id(env: &Env, key: WebhookKey) -> u64 {
    let id: u64 = env.storage().instance().get(&key).unwrap_or(1u64);
    env.storage().instance().set(&key, &(id + 1));
    id
}

fn webhook_ids(env: &Env) -> Vec<u64> {
    env.storage()
        .instance()
        .get(&WebhookKey::WebhookIds)
        .unwrap_or_else(|| Vec::new(env))
}

fn owner_webhooks(env: &Env, owner: &Address) -> Vec<u64> {
    env.storage()
        .persistent()
        .get(&WebhookKey::OwnerWebhooks(owner.clone()))
        .unwrap_or_else(|| Vec::new(env))
}

fn save_webhook(env: &Env, webhook: &Webhook) {
    env.storage()
        .persistent()
        .set(&WebhookKey::Webhook(webhook.id), webhook);
}

fn save_delivery(env: &Env, delivery: &WebhookDelivery) {
    env.storage()
        .persistent()
        .set(&WebhookKey::Delivery(delivery.id), delivery);
}

pub fn get_webhook(env: &Env, webhook_id: u64) -> Option<Webhook> {
    env.storage().persistent().get(&WebhookKey::Webhook(webhook_id))
}

pub fn get_delivery(env: &Env, delivery_id: u64) -> Option<WebhookDelivery> {
    env.storage().persistent().get(&WebhookKey::Delivery(delivery_id))
}

fn load_webhook(env: &Env, webhook_id: u64) -> Webhook {
    get_webhook(env, webhook_id).unwrap_or_else(|| panic!("webhook not found"))
}

fn load_delivery(env: &Env, delivery_id: u64) -> WebhookDelivery {
    get_delivery(env, delivery_id).unwrap_or_else(|| panic!("delivery not found"))
}

fn validate_config(endpoint: &String, target: &Option<Address>, event_mask: u32) {
    if event_mask == 0 || event_mask & !EVENT_ALL != 0 {
        panic!("invalid webhook event mask");
    }
    if endpoint.len() > MAX_ENDPOINT_LEN {
        panic!("webhook endpoint too long");
    }
    if endpoint.is_empty() && target.is_none() {
        panic!("webhook requires an endpoint or target");
    }
}

/// Backoff (in ledgers) applied after `attempts` failed attempts.
pub fn backoff_ledgers(attempts: u32) -> u32 {
    let shift = attempts.saturating_sub(1).min(16);
    RETRY_BASE_LEDGERS
        .saturating_mul(1u32 << shift)
        .min(RETRY_MAX_BACKOFF_LEDGERS)
}

// ---------------------------------------------------------------------------
// Management
// ---------------------------------------------------------------------------

/// Register a webhook owned by `owner`. Returns the new webhook id.
pub fn register(
    env: &Env,
    owner: Address,
    endpoint: String,
    target: Option<Address>,
    event_mask: u32,
    invoice_id: Option<u64>,
) -> u64 {
    owner.require_auth();
    validate_config(&endpoint, &target, event_mask);

    let mut ids = webhook_ids(env);
    if ids.len() >= MAX_WEBHOOKS {
        panic!("webhook limit reached");
    }
    let mut owned = owner_webhooks(env, &owner);
    if owned.len() >= MAX_WEBHOOKS_PER_OWNER {
        panic!("owner webhook limit reached");
    }

    let id = next_id(env, WebhookKey::NextWebhookId);
    let webhook = Webhook {
        id,
        owner: owner.clone(),
        endpoint,
        target,
        event_mask,
        invoice_id,
        active: true,
        created_at: env.ledger().timestamp(),
        delivered_count: 0,
        failed_count: 0,
        consecutive_failures: 0,
    };
    save_webhook(env, &webhook);

    ids.push_back(id);
    env.storage().instance().set(&WebhookKey::WebhookIds, &ids);
    owned.push_back(id);
    env.storage()
        .persistent()
        .set(&WebhookKey::OwnerWebhooks(owner.clone()), &owned);

    env.events()
        .publish((symbol_short!("split"), symbol_short!("wh_reg"), id), (owner, event_mask));
    id
}

/// Update a webhook's endpoint, target, filter and active flag. Owner only.
pub fn update(
    env: &Env,
    owner: Address,
    webhook_id: u64,
    endpoint: String,
    target: Option<Address>,
    event_mask: u32,
    invoice_id: Option<u64>,
    active: bool,
) {
    owner.require_auth();
    let mut webhook = load_webhook(env, webhook_id);
    if webhook.owner != owner {
        panic!("not webhook owner");
    }
    validate_config(&endpoint, &target, event_mask);

    webhook.endpoint = endpoint;
    webhook.target = target;
    webhook.event_mask = event_mask;
    webhook.invoice_id = invoice_id;
    if active && !webhook.active {
        webhook.consecutive_failures = 0;
    }
    webhook.active = active;
    save_webhook(env, &webhook);

    env.events().publish(
        (symbol_short!("split"), symbol_short!("wh_upd"), webhook_id),
        (event_mask, active),
    );
}

/// Remove a webhook. Allowed for the owner, or for an admin via `force`.
pub fn remove(env: &Env, caller: Address, webhook_id: u64, force: bool) {
    if !force {
        caller.require_auth();
    }
    let webhook = load_webhook(env, webhook_id);
    if !force && webhook.owner != caller {
        panic!("not webhook owner");
    }

    let mut ids = webhook_ids(env);
    if let Some(i) = ids.first_index_of(webhook_id) {
        ids.remove(i);
    }
    env.storage().instance().set(&WebhookKey::WebhookIds, &ids);

    let mut owned = owner_webhooks(env, &webhook.owner);
    if let Some(i) = owned.first_index_of(webhook_id) {
        owned.remove(i);
    }
    env.storage()
        .persistent()
        .set(&WebhookKey::OwnerWebhooks(webhook.owner.clone()), &owned);

    env.storage().persistent().remove(&WebhookKey::Webhook(webhook_id));

    env.events()
        .publish((symbol_short!("split"), symbol_short!("wh_del"), webhook_id), caller);
}

/// All registered webhooks.
pub fn list(env: &Env) -> Vec<Webhook> {
    let mut out = Vec::new(env);
    for id in webhook_ids(env).iter() {
        if let Some(w) = get_webhook(env, id) {
            out.push_back(w);
        }
    }
    out
}

/// Webhooks owned by `owner`.
pub fn list_by_owner(env: &Env, owner: &Address) -> Vec<Webhook> {
    let mut out = Vec::new(env);
    for id in owner_webhooks(env, owner).iter() {
        if let Some(w) = get_webhook(env, id) {
            out.push_back(w);
        }
    }
    out
}

/// Most recent deliveries for a webhook (oldest first, up to `MAX_TRACKED_DELIVERIES`).
pub fn list_deliveries(env: &Env, webhook_id: u64) -> Vec<WebhookDelivery> {
    let ids: Vec<u64> = env
        .storage()
        .persistent()
        .get(&WebhookKey::WebhookDeliveries(webhook_id))
        .unwrap_or_else(|| Vec::new(env));
    let mut out = Vec::new(env);
    for id in ids.iter() {
        if let Some(d) = get_delivery(env, id) {
            out.push_back(d);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Delivery
// ---------------------------------------------------------------------------

/// Map a `notify_invoice` event symbol to its `EVENT_*` bit (0 when unknown).
pub fn event_from_symbol(env: &Env, event: &Symbol) -> u32 {
    if *event == symbol_short!("pay") {
        EVENT_PAID
    } else if *event == symbol_short!("release") {
        EVENT_RELEASED
    } else if *event == symbol_short!("refund") {
        EVENT_REFUNDED
    } else if *event == symbol_short!("created") {
        EVENT_CREATED
    } else if *event == Symbol::new(env, "cancelled") {
        EVENT_CANCELLED
    } else {
        0
    }
}

fn is_match(webhook: &Webhook, invoice_id: u64, event: u32) -> bool {
    webhook.active
        && webhook.event_mask & event != 0
        && webhook.invoice_id.map_or(true, |id| id == invoice_id)
}

/// Fan an invoice lifecycle event out to every matching webhook.
///
/// Never panics on subscriber failure: failed target invocations are recorded
/// and scheduled for retry.
pub fn dispatch(env: &Env, invoice_id: u64, event: u32) {
    if event == 0 {
        return;
    }
    let ids = webhook_ids(env);
    if ids.is_empty() {
        return;
    }
    for webhook_id in ids.iter() {
        let webhook = match get_webhook(env, webhook_id) {
            Some(w) => w,
            None => continue,
        };
        if !is_match(&webhook, invoice_id, event) {
            continue;
        }

        let now = env.ledger().sequence();
        let mut delivery = WebhookDelivery {
            id: next_id(env, WebhookKey::NextDeliveryId),
            webhook_id,
            invoice_id,
            event,
            status: DeliveryStatus::Pending,
            attempts: 0,
            created_ledger: now,
            last_attempt_ledger: 0,
            next_retry_ledger: 0,
        };
        track_delivery(env, webhook_id, delivery.id);
        attempt(env, webhook, &mut delivery);
    }
}

fn track_delivery(env: &Env, webhook_id: u64, delivery_id: u64) {
    let key = WebhookKey::WebhookDeliveries(webhook_id);
    let mut ids: Vec<u64> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|| Vec::new(env));
    ids.push_back(delivery_id);
    while ids.len() > MAX_TRACKED_DELIVERIES {
        ids.pop_front();
    }
    env.storage().persistent().set(&key, &ids);
}

/// Perform one delivery attempt and persist the outcome.
fn attempt(env: &Env, webhook: Webhook, delivery: &mut WebhookDelivery) {
    let now = env.ledger().sequence();
    delivery.attempts += 1;
    delivery.last_attempt_ledger = now;

    env.events().publish(
        (symbol_short!("split"), symbol_short!("wh_send"), webhook.id),
        (
            delivery.id,
            delivery.invoice_id,
            delivery.event,
            delivery.attempts,
            webhook.endpoint.clone(),
        ),
    );

    match webhook.target.clone() {
        Some(target) => {
            let mut args: Vec<Val> = Vec::new(env);
            args.push_back(delivery.id.into_val(env));
            args.push_back(delivery.invoice_id.into_val(env));
            args.push_back(delivery.event.into_val(env));
            let res = env.try_invoke_contract::<Val, soroban_sdk::Error>(
                &target,
                &Symbol::new(env, TARGET_FN),
                args,
            );
            record_outcome(env, webhook, delivery, matches!(res, Ok(Ok(_))));
        }
        None => {
            // Off-chain endpoint: the relayer reports the outcome via `ack_delivery`.
            // Schedule a retry in case no acknowledgement arrives.
            delivery.next_retry_ledger = now.saturating_add(backoff_ledgers(delivery.attempts));
            save_delivery(env, delivery);
        }
    }
}

/// Apply a success/failure outcome to a delivery and its webhook.
fn record_outcome(env: &Env, mut webhook: Webhook, delivery: &mut WebhookDelivery, success: bool) {
    let now = env.ledger().sequence();
    if success {
        delivery.status = DeliveryStatus::Delivered;
        delivery.next_retry_ledger = 0;
        webhook.delivered_count += 1;
        webhook.consecutive_failures = 0;
    } else if delivery.attempts >= MAX_DELIVERY_ATTEMPTS {
        delivery.status = DeliveryStatus::Failed;
        delivery.next_retry_ledger = 0;
        webhook.failed_count += 1;
        webhook.consecutive_failures = webhook.consecutive_failures.saturating_add(1);
        if webhook.consecutive_failures >= AUTO_DISABLE_FAILURES {
            webhook.active = false;
            env.events().publish(
                (symbol_short!("split"), symbol_short!("wh_off"), webhook.id),
                webhook.consecutive_failures,
            );
        }
    } else {
        delivery.status = DeliveryStatus::Pending;
        delivery.next_retry_ledger = now.saturating_add(backoff_ledgers(delivery.attempts));
    }
    save_delivery(env, delivery);
    save_webhook(env, &webhook);

    env.events().publish(
        (symbol_short!("split"), symbol_short!("wh_stat"), delivery.webhook_id),
        (delivery.id, delivery.status, delivery.attempts, delivery.next_retry_ledger),
    );
}

/// Retry a pending delivery once its backoff has elapsed. Callable by anyone
/// (keepers/relayers). Returns the resulting status.
pub fn retry(env: &Env, delivery_id: u64) -> DeliveryStatus {
    let mut delivery = load_delivery(env, delivery_id);
    if delivery.status != DeliveryStatus::Pending {
        panic!("delivery is not pending");
    }
    if env.ledger().sequence() < delivery.next_retry_ledger {
        panic!("retry not yet due");
    }

    let webhook = match get_webhook(env, delivery.webhook_id) {
        Some(w) if w.active => w,
        _ => {
            // Webhook removed or disabled: stop retrying.
            delivery.status = DeliveryStatus::Failed;
            delivery.next_retry_ledger = 0;
            save_delivery(env, &delivery);
            return delivery.status;
        }
    };

    if delivery.attempts >= MAX_DELIVERY_ATTEMPTS {
        record_outcome(env, webhook, &mut delivery, false);
        return delivery.status;
    }
    attempt(env, webhook, &mut delivery);
    delivery.status
}

/// Acknowledge the outcome of an off-chain endpoint delivery. Webhook owner only.
pub fn ack(env: &Env, owner: Address, delivery_id: u64, success: bool) -> DeliveryStatus {
    owner.require_auth();
    let mut delivery = load_delivery(env, delivery_id);
    let webhook = load_webhook(env, delivery.webhook_id);
    if webhook.owner != owner {
        panic!("not webhook owner");
    }
    if delivery.status != DeliveryStatus::Pending {
        panic!("delivery is not pending");
    }
    record_outcome(env, webhook, &mut delivery, success);
    delivery.status
}
