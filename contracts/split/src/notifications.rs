//! Configurable payment confirmation notification system.
//!
//! Provides customizable, multi-channel payment confirmation notifications with
//! delivery tracking and strict deduplication:
//!
//! - **Configuration & Customization** — any address (payer, creator, or
//!   observer) can register a [`NotificationConfig`] specifying enabled
//!   delivery channels, optional invoice scope, minimum payment threshold,
//!   custom message template, and payload visibility flags (`include_payer`,
//!   `include_amount`).
//! - **Multi-Channel Delivery** — supports on-chain Soroban events
//!   (`CHANNEL_ON_CHAIN_EVENT`), webhook relayers (`CHANNEL_WEBHOOK`), email
//!   relayers (`CHANNEL_EMAIL`), SMS relayers (`CHANNEL_SMS`), and on-chain
//!   subscriber contracts (`CHANNEL_CONTRACT`, invoked via
//!   `try_invoke_contract` so a failing subscriber never reverts a payment).
//! - **Deduplication** — every `(config_id, invoice_id, payer, payment_nonce)`
//!   tuple is recorded in persistent storage on first dispatch so the same
//!   payment can never produce duplicate notifications.
//! - **Tracking** — every dispatched notification is persisted as a
//!   [`NotificationRecord`] with per-channel delivery/failure bitmasks,
//!   aggregate [`NotificationStatus`], and per-config [`NotificationStats`].
//!   Off-chain relayers report channel outcomes via `ack_notification`.

use soroban_sdk::{contracttype, symbol_short, Address, Env, IntoVal, String, Symbol, Val, Vec};

// ---------------------------------------------------------------------------
// Delivery channel bits
// ---------------------------------------------------------------------------

/// Emit a Soroban contract event (delivered immediately on-chain).
pub const CHANNEL_ON_CHAIN_EVENT: u32 = 1 << 0;
/// Dispatch to an off-chain webhook relayer (`webhook_url`).
pub const CHANNEL_WEBHOOK: u32 = 1 << 1;
/// Dispatch to an off-chain email relayer (`email`).
pub const CHANNEL_EMAIL: u32 = 1 << 2;
/// Dispatch to an off-chain SMS relayer (`phone`).
pub const CHANNEL_SMS: u32 = 1 << 3;
/// Invoke an on-chain contract implementing `on_payment_notification`.
pub const CHANNEL_CONTRACT: u32 = 1 << 4;

/// Bitmask of all supported channels.
pub const CHANNEL_ALL: u32 =
    CHANNEL_ON_CHAIN_EVENT | CHANNEL_WEBHOOK | CHANNEL_EMAIL | CHANNEL_SMS | CHANNEL_CONTRACT;

/// Bitmask of off-chain channels that require relayer acknowledgement.
pub const OFF_CHAIN_CHANNELS: u32 = CHANNEL_WEBHOOK | CHANNEL_EMAIL | CHANNEL_SMS;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum number of registered notification configs (bounds dispatch loop).
pub const MAX_CONFIGS: u32 = 30;
/// Maximum number of notification configs a single owner may register.
pub const MAX_CONFIGS_PER_OWNER: u32 = 5;
/// Maximum custom template length in bytes.
pub const MAX_TEMPLATE_LEN: u32 = 256;
/// Maximum webhook URL or email length in bytes.
pub const MAX_CHANNEL_ADDR_LEN: u32 = 256;
/// Maximum phone number length in bytes.
pub const MAX_PHONE_LEN: u32 = 64;
/// Number of recent notification ids retained per config / per invoice.
const MAX_TRACKED_NOTIFICATIONS: u32 = 50;

/// Function invoked on a config's `target_contract`.
const TARGET_FN: &str = "on_payment_notification";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Aggregate delivery status of a payment confirmation notification.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationStatus {
    /// Waiting for off-chain channel acknowledgements.
    Pending,
    /// At least one channel succeeded, while others are still pending or failed.
    PartiallyDelivered,
    /// Every configured channel delivered successfully.
    Delivered,
    /// Every configured channel failed.
    Failed,
}

/// Input parameters for registering or updating a notification configuration.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationConfigInput {
    /// Bitwise OR of `CHANNEL_*` constants.
    pub channels: u32,
    /// Restrict notifications to a single invoice; `None` matches all invoices.
    pub invoice_id: Option<u64>,
    /// Minimum payment amount required to trigger a notification (`>= 0`).
    pub min_amount: i128,
    /// Whether to include payer information in customized notifications.
    pub include_payer: bool,
    /// Whether to include payment amount in customized notifications.
    pub include_amount: bool,
    /// Optional custom message template / memo.
    pub custom_template: String,
    /// Off-chain webhook URL (required when `CHANNEL_WEBHOOK` is enabled).
    pub webhook_url: String,
    /// Off-chain email address (required when `CHANNEL_EMAIL` is enabled).
    pub email: String,
    /// Off-chain SMS phone number (required when `CHANNEL_SMS` is enabled).
    pub phone: String,
    /// Optional on-chain subscriber contract (required when `CHANNEL_CONTRACT` is enabled).
    pub target_contract: Option<Address>,
    /// Whether this configuration is active.
    pub enabled: bool,
}

/// A persisted payment confirmation notification configuration.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationConfig {
    pub id: u64,
    pub owner: Address,
    pub channels: u32,
    pub invoice_id: Option<u64>,
    pub min_amount: i128,
    pub include_payer: bool,
    pub include_amount: bool,
    pub custom_template: String,
    pub webhook_url: String,
    pub email: String,
    pub phone: String,
    pub target_contract: Option<Address>,
    pub enabled: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Persisted record of a single dispatched payment confirmation notification.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationRecord {
    pub id: u64,
    pub config_id: u64,
    pub owner: Address,
    pub invoice_id: u64,
    pub payer: Address,
    pub amount: i128,
    pub token: Address,
    pub payment_nonce: u64,
    /// Bitmask of channels targeted by this notification.
    pub channels: u32,
    /// Bitmask of channels confirmed delivered.
    pub delivered_channels: u32,
    /// Bitmask of channels confirmed failed.
    pub failed_channels: u32,
    pub include_payer: bool,
    pub include_amount: bool,
    pub custom_template: String,
    pub status: NotificationStatus,
    pub created_ledger: u32,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Delivery tracking statistics for a notification configuration.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct NotificationStats {
    pub total_sent: u64,
    pub total_delivered: u64,
    pub total_partially_delivered: u64,
    pub total_failed: u64,
    pub total_pending: u64,
}

#[contracttype]
#[derive(Clone)]
enum NotificationKey {
    /// Next config id (instance).
    NextConfigId,
    /// Next notification record id (instance).
    NextNotificationId,
    /// All registered config ids (instance).
    ConfigIds,
    /// Config record by id (persistent).
    Config(u64),
    /// Config ids owned by an address (persistent).
    OwnerConfigs(Address),
    /// Notification record by id (persistent).
    Record(u64),
    /// Recent notification ids for a config (persistent).
    ConfigNotifications(u64),
    /// Recent notification ids for an invoice (persistent).
    InvoiceNotifications(u64),
    /// Deduplication marker for `(config_id, invoice_id, payer, payment_nonce)` (persistent).
    Dedup(u64, u64, Address, u64),
    /// Per-config delivery statistics (persistent).
    ConfigStats(u64),
}

// ---------------------------------------------------------------------------
// Storage helpers
// ---------------------------------------------------------------------------

fn next_config_id(env: &Env) -> u64 {
    let id: u64 = env
        .storage()
        .instance()
        .get(&NotificationKey::NextConfigId)
        .unwrap_or(1);
    env.storage()
        .instance()
        .set(&NotificationKey::NextConfigId, &(id + 1));
    id
}

fn next_notification_id(env: &Env) -> u64 {
    let id: u64 = env
        .storage()
        .instance()
        .get(&NotificationKey::NextNotificationId)
        .unwrap_or(1);
    env.storage()
        .instance()
        .set(&NotificationKey::NextNotificationId, &(id + 1));
    id
}

fn config_ids(env: &Env) -> Vec<u64> {
    env.storage()
        .instance()
        .get(&NotificationKey::ConfigIds)
        .unwrap_or_else(|| Vec::new(env))
}

fn owner_configs(env: &Env, owner: &Address) -> Vec<u64> {
    env.storage()
        .persistent()
        .get(&NotificationKey::OwnerConfigs(owner.clone()))
        .unwrap_or_else(|| Vec::new(env))
}

fn load_config(env: &Env, config_id: u64) -> NotificationConfig {
    env.storage()
        .persistent()
        .get(&NotificationKey::Config(config_id))
        .expect("notification config not found")
}

fn save_config(env: &Env, config: &NotificationConfig) {
    env.storage()
        .persistent()
        .set(&NotificationKey::Config(config.id), config);
}

fn load_record(env: &Env, notification_id: u64) -> NotificationRecord {
    env.storage()
        .persistent()
        .get(&NotificationKey::Record(notification_id))
        .expect("notification not found")
}

fn save_record(env: &Env, record: &NotificationRecord) {
    env.storage()
        .persistent()
        .set(&NotificationKey::Record(record.id), record);
}

fn append_bounded_id(env: &Env, key: &NotificationKey, id: u64) {
    let mut ids: Vec<u64> = env
        .storage()
        .persistent()
        .get(key)
        .unwrap_or_else(|| Vec::new(env));
    if ids.len() >= MAX_TRACKED_NOTIFICATIONS {
        ids.remove(0);
    }
    ids.push_back(id);
    env.storage().persistent().set(key, &ids);
}

fn load_stats(env: &Env, config_id: u64) -> NotificationStats {
    env.storage()
        .persistent()
        .get(&NotificationKey::ConfigStats(config_id))
        .unwrap_or_default()
}

fn save_stats(env: &Env, config_id: u64, stats: &NotificationStats) {
    env.storage()
        .persistent()
        .set(&NotificationKey::ConfigStats(config_id), stats);
}

fn compute_status(channels: u32, delivered: u32, failed: u32) -> NotificationStatus {
    if delivered == channels {
        NotificationStatus::Delivered
    } else if failed == channels {
        NotificationStatus::Failed
    } else if delivered != 0 {
        NotificationStatus::PartiallyDelivered
    } else {
        NotificationStatus::Pending
    }
}

fn apply_status_delta(
    stats: &mut NotificationStats,
    old_status: Option<NotificationStatus>,
    new_status: NotificationStatus,
) {
    if let Some(prev) = old_status {
        if prev == new_status {
            return;
        }
        match prev {
            NotificationStatus::Pending => {
                stats.total_pending = stats.total_pending.saturating_sub(1)
            }
            NotificationStatus::PartiallyDelivered => {
                stats.total_partially_delivered = stats.total_partially_delivered.saturating_sub(1)
            }
            NotificationStatus::Delivered => {
                stats.total_delivered = stats.total_delivered.saturating_sub(1)
            }
            NotificationStatus::Failed => {
                stats.total_failed = stats.total_failed.saturating_sub(1)
            }
        }
    } else {
        stats.total_sent = stats.total_sent.saturating_add(1);
    }

    match new_status {
        NotificationStatus::Pending => stats.total_pending = stats.total_pending.saturating_add(1),
        NotificationStatus::PartiallyDelivered => {
            stats.total_partially_delivered = stats.total_partially_delivered.saturating_add(1)
        }
        NotificationStatus::Delivered => {
            stats.total_delivered = stats.total_delivered.saturating_add(1)
        }
        NotificationStatus::Failed => stats.total_failed = stats.total_failed.saturating_add(1),
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

fn validate_input(input: &NotificationConfigInput) {
    assert!(
        input.channels != 0 && (input.channels & !CHANNEL_ALL) == 0,
        "invalid notification channels"
    );
    assert!(input.min_amount >= 0, "min_amount must be non-negative");
    assert!(
        input.custom_template.len() <= MAX_TEMPLATE_LEN,
        "custom template too long"
    );
    assert!(
        input.webhook_url.len() <= MAX_CHANNEL_ADDR_LEN,
        "webhook url too long"
    );
    assert!(
        input.email.len() <= MAX_CHANNEL_ADDR_LEN,
        "email too long"
    );
    assert!(input.phone.len() <= MAX_PHONE_LEN, "phone too long");

    if (input.channels & CHANNEL_WEBHOOK) != 0 {
        assert!(!input.webhook_url.is_empty(), "webhook url required");
    }
    if (input.channels & CHANNEL_EMAIL) != 0 {
        assert!(!input.email.is_empty(), "email required");
    }
    if (input.channels & CHANNEL_SMS) != 0 {
        assert!(!input.phone.is_empty(), "phone required");
    }
    if (input.channels & CHANNEL_CONTRACT) != 0 {
        assert!(
            input.target_contract.is_some(),
            "target contract required"
        );
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Register a new payment confirmation notification configuration.
pub fn register_config(env: &Env, owner: Address, input: NotificationConfigInput) -> u64 {
    owner.require_auth();
    validate_input(&input);

    let mut ids = config_ids(env);
    assert!(ids.len() < MAX_CONFIGS, "max notification configs reached");

    let mut owned = owner_configs(env, &owner);
    assert!(
        owned.len() < MAX_CONFIGS_PER_OWNER,
        "max notification configs per owner reached"
    );

    let now = env.ledger().timestamp();
    let id = next_config_id(env);
    let config = NotificationConfig {
        id,
        owner: owner.clone(),
        channels: input.channels,
        invoice_id: input.invoice_id,
        min_amount: input.min_amount,
        include_payer: input.include_payer,
        include_amount: input.include_amount,
        custom_template: input.custom_template,
        webhook_url: input.webhook_url,
        email: input.email,
        phone: input.phone,
        target_contract: input.target_contract,
        enabled: input.enabled,
        created_at: now,
        updated_at: now,
    };

    save_config(env, &config);
    ids.push_back(id);
    env.storage().instance().set(&NotificationKey::ConfigIds, &ids);
    owned.push_back(id);
    env.storage()
        .persistent()
        .set(&NotificationKey::OwnerConfigs(owner.clone()), &owned);

    env.events().publish(
        (symbol_short!("split"), symbol_short!("ntf_reg"), id),
        (owner, input.channels),
    );
    id
}

/// Update an existing notification configuration. Owner only.
pub fn update_config(env: &Env, owner: Address, config_id: u64, input: NotificationConfigInput) {
    owner.require_auth();
    validate_input(&input);

    let mut config = load_config(env, config_id);
    assert!(config.owner == owner, "not notification config owner");

    config.channels = input.channels;
    config.invoice_id = input.invoice_id;
    config.min_amount = input.min_amount;
    config.include_payer = input.include_payer;
    config.include_amount = input.include_amount;
    config.custom_template = input.custom_template;
    config.webhook_url = input.webhook_url;
    config.email = input.email;
    config.phone = input.phone;
    config.target_contract = input.target_contract;
    config.enabled = input.enabled;
    config.updated_at = env.ledger().timestamp();

    save_config(env, &config);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("ntf_upd"), config_id),
        (config.channels, config.enabled),
    );
}

/// Toggle a notification configuration on or off. Owner only.
pub fn set_config_enabled(env: &Env, owner: Address, config_id: u64, enabled: bool) {
    owner.require_auth();
    let mut config = load_config(env, config_id);
    assert!(config.owner == owner, "not notification config owner");
    config.enabled = enabled;
    config.updated_at = env.ledger().timestamp();
    save_config(env, &config);
    env.events().publish(
        (symbol_short!("split"), symbol_short!("ntf_tog"), config_id),
        enabled,
    );
}

/// Remove a notification configuration. Allowed for the owner, or admin via `force`.
pub fn remove_config(env: &Env, caller: Address, config_id: u64, force: bool) {
    if !force {
        caller.require_auth();
    }
    let config = load_config(env, config_id);
    if !force && config.owner != caller {
        panic!("not notification config owner");
    }

    let mut ids = config_ids(env);
    if let Some(i) = ids.first_index_of(config_id) {
        ids.remove(i);
    }
    env.storage().instance().set(&NotificationKey::ConfigIds, &ids);

    let mut owned = owner_configs(env, &config.owner);
    if let Some(i) = owned.first_index_of(config_id) {
        owned.remove(i);
    }
    env.storage()
        .persistent()
        .set(&NotificationKey::OwnerConfigs(config.owner.clone()), &owned);

    env.storage()
        .persistent()
        .remove(&NotificationKey::Config(config_id));

    env.events().publish(
        (symbol_short!("split"), symbol_short!("ntf_del"), config_id),
        caller,
    );
}

/// Dispatch payment confirmation notifications to all matching active configs.
///
/// Deduplicates strictly by `(config_id, invoice_id, payer, payment_nonce)` so
/// the same payment can never trigger duplicate notifications for a config.
/// Returns the number of new notifications created.
pub fn dispatch_payment_confirmation(
    env: &Env,
    invoice_id: u64,
    payer: &Address,
    amount: i128,
    token: &Address,
    payment_nonce: u64,
) -> u32 {
    let ids = config_ids(env);
    if ids.is_empty() {
        return 0;
    }

    let now_ts = env.ledger().timestamp();
    let now_ledger = env.ledger().sequence();
    let mut dispatched: u32 = 0;

    for config_id in ids.iter() {
        let Some(config) = get_config(env, config_id) else {
            continue;
        };
        if !config.enabled {
            continue;
        }
        if let Some(scoped) = config.invoice_id {
            if scoped != invoice_id {
                continue;
            }
        }
        if amount < config.min_amount {
            continue;
        }

        // Strict deduplication check.
        let dedup_key = NotificationKey::Dedup(config_id, invoice_id, payer.clone(), payment_nonce);
        if env.storage().persistent().has(&dedup_key) {
            continue;
        }
        env.storage().persistent().set(&dedup_key, &true);

        let notification_id = next_notification_id(env);
        let mut delivered_channels: u32 = 0;
        let mut failed_channels: u32 = 0;

        // 1. On-chain event channel (delivered immediately).
        if (config.channels & CHANNEL_ON_CHAIN_EVENT) != 0 {
            env.events().publish(
                (
                    symbol_short!("pay_notf"),
                    notification_id,
                    CHANNEL_ON_CHAIN_EVENT,
                ),
                (invoice_id, config_id, payer.clone(), amount),
            );
            delivered_channels |= CHANNEL_ON_CHAIN_EVENT;
        }

        // 2. On-chain contract callback channel (best-effort, never reverts payment).
        if (config.channels & CHANNEL_CONTRACT) != 0 {
            if let Some(ref target) = config.target_contract {
                let mut args: Vec<Val> = Vec::new(env);
                args.push_back(notification_id.into_val(env));
                args.push_back(invoice_id.into_val(env));
                args.push_back(payer.clone().into_val(env));
                args.push_back(amount.into_val(env));
                let res = env.try_invoke_contract::<Val, soroban_sdk::Error>(
                    target,
                    &Symbol::new(env, TARGET_FN),
                    args,
                );
                if matches!(res, Ok(Ok(_))) {
                    delivered_channels |= CHANNEL_CONTRACT;
                } else {
                    failed_channels |= CHANNEL_CONTRACT;
                }
            } else {
                failed_channels |= CHANNEL_CONTRACT;
            }
        }

        // 3. Off-chain channels (emit dispatch events for relayers; await ack).
        for ch in [CHANNEL_WEBHOOK, CHANNEL_EMAIL, CHANNEL_SMS] {
            if (config.channels & ch) != 0 {
                env.events().publish(
                    (symbol_short!("pay_notf"), notification_id, ch),
                    (invoice_id, config_id, payer.clone(), amount),
                );
            }
        }

        let status = compute_status(config.channels, delivered_channels, failed_channels);

        let record = NotificationRecord {
            id: notification_id,
            config_id,
            owner: config.owner.clone(),
            invoice_id,
            payer: payer.clone(),
            amount,
            token: token.clone(),
            payment_nonce,
            channels: config.channels,
            delivered_channels,
            failed_channels,
            include_payer: config.include_payer,
            include_amount: config.include_amount,
            custom_template: config.custom_template.clone(),
            status,
            created_ledger: now_ledger,
            created_at: now_ts,
            updated_at: now_ts,
        };

        save_record(env, &record);
        append_bounded_id(
            env,
            &NotificationKey::ConfigNotifications(config_id),
            notification_id,
        );
        append_bounded_id(
            env,
            &NotificationKey::InvoiceNotifications(invoice_id),
            notification_id,
        );

        let mut stats = load_stats(env, config_id);
        apply_status_delta(&mut stats, None, status);
        save_stats(env, config_id, &stats);

        dispatched += 1;
    }

    dispatched
}

/// Acknowledge delivery outcome for a single off-chain channel on a notification.
///
/// Only the notification config owner may acknowledge. Each channel can only be
/// acknowledged once to prevent duplicate status transitions.
pub fn ack_notification(
    env: &Env,
    owner: Address,
    notification_id: u64,
    channel: u32,
    success: bool,
) -> NotificationStatus {
    owner.require_auth();
    assert!(
        channel.count_ones() == 1 && (channel & OFF_CHAIN_CHANNELS) != 0,
        "invalid off-chain channel"
    );

    let mut record = load_record(env, notification_id);
    assert!(record.owner == owner, "not notification owner");
    assert!(
        (record.channels & channel) != 0,
        "channel not enabled for notification"
    );
    assert!(
        ((record.delivered_channels | record.failed_channels) & channel) == 0,
        "channel already acknowledged"
    );

    let old_status = record.status;
    if success {
        record.delivered_channels |= channel;
    } else {
        record.failed_channels |= channel;
    }
    record.status = compute_status(
        record.channels,
        record.delivered_channels,
        record.failed_channels,
    );
    record.updated_at = env.ledger().timestamp();
    save_record(env, &record);

    let mut stats = load_stats(env, record.config_id);
    apply_status_delta(&mut stats, Some(old_status), record.status);
    save_stats(env, record.config_id, &stats);

    env.events().publish(
        (symbol_short!("split"), symbol_short!("ntf_ack"), notification_id),
        (channel, success, record.status),
    );

    record.status
}

/// Return a notification config by id.
pub fn get_config(env: &Env, config_id: u64) -> Option<NotificationConfig> {
    env.storage()
        .persistent()
        .get(&NotificationKey::Config(config_id))
}

/// Return all registered notification configs.
pub fn list_configs(env: &Env) -> Vec<NotificationConfig> {
    let mut out = Vec::new(env);
    for id in config_ids(env).iter() {
        if let Some(c) = get_config(env, id) {
            out.push_back(c);
        }
    }
    out
}

/// Return all notification configs owned by `owner`.
pub fn list_owner_configs(env: &Env, owner: &Address) -> Vec<NotificationConfig> {
    let mut out = Vec::new(env);
    for id in owner_configs(env, owner).iter() {
        if let Some(c) = get_config(env, id) {
            out.push_back(c);
        }
    }
    out
}

/// Return a notification record by id.
pub fn get_notification(env: &Env, notification_id: u64) -> Option<NotificationRecord> {
    env.storage()
        .persistent()
        .get(&NotificationKey::Record(notification_id))
}

/// Return recent notifications for a config (oldest first).
pub fn list_notifications_by_config(env: &Env, config_id: u64) -> Vec<NotificationRecord> {
    let ids: Vec<u64> = env
        .storage()
        .persistent()
        .get(&NotificationKey::ConfigNotifications(config_id))
        .unwrap_or_else(|| Vec::new(env));
    let mut out = Vec::new(env);
    for id in ids.iter() {
        if let Some(r) = get_notification(env, id) {
            out.push_back(r);
        }
    }
    out
}

/// Return recent notifications for an invoice (oldest first).
pub fn list_notifications_by_invoice(env: &Env, invoice_id: u64) -> Vec<NotificationRecord> {
    let ids: Vec<u64> = env
        .storage()
        .persistent()
        .get(&NotificationKey::InvoiceNotifications(invoice_id))
        .unwrap_or_else(|| Vec::new(env));
    let mut out = Vec::new(env);
    for id in ids.iter() {
        if let Some(r) = get_notification(env, id) {
            out.push_back(r);
        }
    }
    out
}

/// Return delivery tracking statistics for a notification config.
pub fn get_config_stats(env: &Env, config_id: u64) -> NotificationStats {
    load_stats(env, config_id)
}

/// Return whether a payment confirmation has already been dispatched for the
/// given `(config_id, invoice_id, payer, payment_nonce)` tuple.
pub fn is_dispatched(
    env: &Env,
    config_id: u64,
    invoice_id: u64,
    payer: &Address,
    payment_nonce: u64,
) -> bool {
    env.storage()
        .persistent()
        .has(&NotificationKey::Dedup(config_id, invoice_id, payer.clone(), payment_nonce))
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env, String};

    #[contract]
    struct MockSubscriber;

    #[contractimpl]
    impl MockSubscriber {
        pub fn on_payment_notification(
            env: Env,
            notification_id: u64,
            invoice_id: u64,
            payer: Address,
            amount: i128,
        ) {
            env.storage()
                .instance()
                .set(&symbol_short!("last"), &(notification_id, invoice_id, payer, amount));
        }

        pub fn last_received(env: Env) -> Option<(u64, u64, Address, i128)> {
            env.storage().instance().get(&symbol_short!("last"))
        }
    }

    #[contract]
    struct FailingSubscriber;

    #[contractimpl]
    impl FailingSubscriber {
        pub fn on_payment_notification(
            _env: Env,
            _notification_id: u64,
            _invoice_id: u64,
            _payer: Address,
            _amount: i128,
        ) {
            panic!("subscriber failure");
        }
    }

    fn base_input(env: &Env, channels: u32) -> NotificationConfigInput {
        NotificationConfigInput {
            channels,
            invoice_id: None,
            min_amount: 0,
            include_payer: true,
            include_amount: true,
            custom_template: String::from_str(env, "Payment confirmed for invoice"),
            webhook_url: if (channels & CHANNEL_WEBHOOK) != 0 {
                String::from_str(env, "https://hooks.example.com/pay")
            } else {
                String::from_str(env, "")
            },
            email: if (channels & CHANNEL_EMAIL) != 0 {
                String::from_str(env, "alerts@example.com")
            } else {
                String::from_str(env, "")
            },
            phone: if (channels & CHANNEL_SMS) != 0 {
                String::from_str(env, "+15550100")
            } else {
                String::from_str(env, "")
            },
            target_contract: None,
            enabled: true,
        }
    }

    #[test]
    fn test_register_update_toggle_and_remove_config() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(crate::SplitContract, ());

        env.as_contract(&contract_id, || {
            let owner = Address::generate(&env);
            let input = base_input(&env, CHANNEL_ON_CHAIN_EVENT | CHANNEL_EMAIL);
            let cfg_id = register_config(&env, owner.clone(), input);

            let cfg = get_config(&env, cfg_id).expect("config should exist");
            assert_eq!(cfg.owner, owner);
            assert_eq!(cfg.channels, CHANNEL_ON_CHAIN_EVENT | CHANNEL_EMAIL);
            assert!(cfg.enabled);
            assert_eq!(list_configs(&env).len(), 1);
            assert_eq!(list_owner_configs(&env, &owner).len(), 1);

            let mut updated = base_input(&env, CHANNEL_SMS);
            updated.min_amount = 500;
            updated.include_payer = false;
            updated.custom_template = String::from_str(&env, "Custom SMS");
            update_config(&env, owner.clone(), cfg_id, updated);

            let cfg2 = get_config(&env, cfg_id).unwrap();
            assert_eq!(cfg2.channels, CHANNEL_SMS);
            assert_eq!(cfg2.min_amount, 500);
            assert!(!cfg2.include_payer);

            set_config_enabled(&env, owner.clone(), cfg_id, false);
            assert!(!get_config(&env, cfg_id).unwrap().enabled);

            remove_config(&env, owner.clone(), cfg_id, false);
            assert!(get_config(&env, cfg_id).is_none());
            assert_eq!(list_configs(&env).len(), 0);
        });
    }

    #[test]
    #[should_panic(expected = "invalid notification channels")]
    fn test_register_zero_channels_panics() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(crate::SplitContract, ());

        env.as_contract(&contract_id, || {
            let owner = Address::generate(&env);
            let input = base_input(&env, 0);
            register_config(&env, owner, input);
        });
    }

    #[test]
    #[should_panic(expected = "email required")]
    fn test_register_email_channel_without_email_panics() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(crate::SplitContract, ());

        env.as_contract(&contract_id, || {
            let owner = Address::generate(&env);
            let mut input = base_input(&env, CHANNEL_EMAIL);
            input.email = String::from_str(&env, "");
            register_config(&env, owner, input);
        });
    }

    #[test]
    fn test_multichannel_dispatch_tracking_and_ack() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(crate::SplitContract, ());
        let sub_id = env.register(MockSubscriber, ());

        env.as_contract(&contract_id, || {
            let owner = Address::generate(&env);
            let payer = Address::generate(&env);
            let token = Address::generate(&env);

            let channels =
                CHANNEL_ON_CHAIN_EVENT | CHANNEL_CONTRACT | CHANNEL_WEBHOOK | CHANNEL_EMAIL;
            let mut input = base_input(&env, channels);
            input.target_contract = Some(sub_id.clone());
            let cfg_id = register_config(&env, owner.clone(), input);

            let sent = dispatch_payment_confirmation(&env, 42, &payer, 1_000, &token, 1);
            assert_eq!(sent, 1);

            let recs = list_notifications_by_config(&env, cfg_id);
            assert_eq!(recs.len(), 1);
            let rec = recs.get(0).unwrap();
            assert_eq!(rec.invoice_id, 42);
            assert_eq!(rec.amount, 1_000);
            // On-chain event + contract succeeded; webhook + email pending -> PartiallyDelivered
            assert_eq!(
                rec.delivered_channels,
                CHANNEL_ON_CHAIN_EVENT | CHANNEL_CONTRACT
            );
            assert_eq!(rec.status, NotificationStatus::PartiallyDelivered);

            let stats = get_config_stats(&env, cfg_id);
            assert_eq!(stats.total_sent, 1);
            assert_eq!(stats.total_partially_delivered, 1);
            assert_eq!(stats.total_delivered, 0);

            // Ack webhook -> still PartiallyDelivered (email pending)
            let s1 = ack_notification(&env, owner.clone(), rec.id, CHANNEL_WEBHOOK, true);
            assert_eq!(s1, NotificationStatus::PartiallyDelivered);

            // Ack email -> now Delivered
            let s2 = ack_notification(&env, owner.clone(), rec.id, CHANNEL_EMAIL, true);
            assert_eq!(s2, NotificationStatus::Delivered);

            let stats2 = get_config_stats(&env, cfg_id);
            assert_eq!(stats2.total_sent, 1);
            assert_eq!(stats2.total_partially_delivered, 0);
            assert_eq!(stats2.total_delivered, 1);

            let by_inv = list_notifications_by_invoice(&env, 42);
            assert_eq!(by_inv.len(), 1);
            assert_eq!(by_inv.get(0).unwrap().status, NotificationStatus::Delivered);
        });

        let sub_client = MockSubscriberClient::new(&env, &sub_id);
        let received = sub_client.last_received().expect("subscriber should be invoked");
        assert_eq!(received.1, 42);
        assert_eq!(received.3, 1_000);
    }

    #[test]
    fn test_failing_contract_channel_does_not_revert_and_marks_failed() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(crate::SplitContract, ());
        let bad_sub = env.register(FailingSubscriber, ());

        env.as_contract(&contract_id, || {
            let owner = Address::generate(&env);
            let payer = Address::generate(&env);
            let token = Address::generate(&env);

            let mut input = base_input(&env, CHANNEL_CONTRACT);
            input.target_contract = Some(bad_sub.clone());
            let cfg_id = register_config(&env, owner, input);

            let sent = dispatch_payment_confirmation(&env, 7, &payer, 250, &token, 1);
            assert_eq!(sent, 1);

            let rec = list_notifications_by_config(&env, cfg_id).get(0).unwrap();
            assert_eq!(rec.status, NotificationStatus::Failed);
            assert_eq!(rec.failed_channels, CHANNEL_CONTRACT);

            let stats = get_config_stats(&env, cfg_id);
            assert_eq!(stats.total_failed, 1);
        });
    }

    #[test]
    fn test_deduplication_prevents_duplicate_notifications() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(crate::SplitContract, ());

        env.as_contract(&contract_id, || {
            let owner = Address::generate(&env);
            let payer = Address::generate(&env);
            let token = Address::generate(&env);

            let input = base_input(&env, CHANNEL_ON_CHAIN_EVENT);
            let cfg_id = register_config(&env, owner, input);

            // First dispatch succeeds
            assert_eq!(
                dispatch_payment_confirmation(&env, 10, &payer, 500, &token, 99),
                1
            );
            assert!(is_dispatched(&env, cfg_id, 10, &payer, 99));

            // Duplicate dispatch with same (invoice_id, payer, payment_nonce) is skipped
            assert_eq!(
                dispatch_payment_confirmation(&env, 10, &payer, 500, &token, 99),
                0
            );
            assert_eq!(list_notifications_by_config(&env, cfg_id).len(), 1);
            assert_eq!(get_config_stats(&env, cfg_id).total_sent, 1);

            // Different nonce dispatches a new notification
            assert_eq!(
                dispatch_payment_confirmation(&env, 10, &payer, 500, &token, 100),
                1
            );
            assert_eq!(list_notifications_by_config(&env, cfg_id).len(), 2);
        });
    }

    #[test]
    fn test_customization_filters_by_invoice_min_amount_and_enabled() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(crate::SplitContract, ());

        env.as_contract(&contract_id, || {
            let owner = Address::generate(&env);
            let payer = Address::generate(&env);
            let token = Address::generate(&env);

            let mut input = base_input(&env, CHANNEL_ON_CHAIN_EVENT);
            input.invoice_id = Some(5);
            input.min_amount = 1_000;
            let cfg_id = register_config(&env, owner.clone(), input);

            // Wrong invoice
            assert_eq!(
                dispatch_payment_confirmation(&env, 6, &payer, 2_000, &token, 1),
                0
            );
            // Below min_amount
            assert_eq!(
                dispatch_payment_confirmation(&env, 5, &payer, 999, &token, 2),
                0
            );
            // Disabled
            set_config_enabled(&env, owner.clone(), cfg_id, false);
            assert_eq!(
                dispatch_payment_confirmation(&env, 5, &payer, 1_500, &token, 3),
                0
            );
            // Re-enabled and matching
            set_config_enabled(&env, owner, cfg_id, true);
            assert_eq!(
                dispatch_payment_confirmation(&env, 5, &payer, 1_500, &token, 4),
                1
            );
            assert_eq!(list_notifications_by_config(&env, cfg_id).len(), 1);
        });
    }

    #[test]
    #[should_panic(expected = "channel already acknowledged")]
    fn test_double_ack_panics() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(crate::SplitContract, ());

        env.as_contract(&contract_id, || {
            let owner = Address::generate(&env);
            let payer = Address::generate(&env);
            let token = Address::generate(&env);

            let input = base_input(&env, CHANNEL_WEBHOOK);
            let cfg_id = register_config(&env, owner.clone(), input);

            dispatch_payment_confirmation(&env, 1, &payer, 100, &token, 1);
            let rec = list_notifications_by_config(&env, cfg_id).get(0).unwrap();

            ack_notification(&env, owner.clone(), rec.id, CHANNEL_WEBHOOK, true);
            ack_notification(&env, owner, rec.id, CHANNEL_WEBHOOK, true);
        });
    }
}
