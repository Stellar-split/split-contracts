use soroban_sdk::{symbol_short, Address, Bytes, Env, Vec};

/// Emitted when an invoice reaches a completed state (Released) with a full
/// structured summary optimised for webhook / off-chain processing.
pub fn invoice_completed(
    env: &Env,
    invoice_id: u64,
    creator: &Address,
    total: i128,
    recipient_count: u32,
    completion_timestamp: u64,
) {
    env.events().publish(
        (symbol_short!("inv_cmpl"), invoice_id),
        (invoice_id, creator.clone(), total, recipient_count, completion_timestamp),
    );
}

/// Emitted when a new invoice is created.
pub fn invoice_created(env: &Env, invoice_id: u64, creator: &Address, total: i128, metadata: &Option<Bytes>) {
    env.events().publish(
        (symbol_short!("inv_crt"), invoice_id),
        (creator.clone(), total, metadata.clone()),
    );
}

/// Emitted when a payment is received toward an invoice.
pub fn payment_received(env: &Env, invoice_id: u64, payer: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("inv_pay"), invoice_id),
        (payer.clone(), amount),
    );
}

/// Emitted when an invoice is fully funded and funds are released.
pub fn invoice_released(env: &Env, invoice_id: u64, recipients: &Vec<Address>) {
    env.events().publish(
        (symbol_short!("inv_rel"), invoice_id),
        recipients.clone(),
    );
}

/// Emitted when an invoice is refunded after deadline.
pub fn invoice_refunded(env: &Env, invoice_id: u64) {
    env.events()
        .publish((symbol_short!("inv_ref"), invoice_id), ());
}

/// Emitted when a recipient is added to an existing invoice.
pub fn recipient_added(env: &Env, invoice_id: u64, caller: &Address, recipient: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("add_rec"), invoice_id),
        (caller.clone(), recipient.clone(), amount),
    );
}

/// Emitted when the insurance pool is drawn from to cover a refund shortfall.
pub fn insurance_used(env: &Env, invoice_id: u64, shortfall: i128, remaining: i128) {
    env.events().publish(
        (symbol_short!("ins_used"), invoice_id),
        (shortfall, remaining),
    );
}

/// Emitted once per unique payer when their refund is transferred.
pub fn payer_refunded(env: &Env, invoice_id: u64, payer: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("pay_ref"), invoice_id),
        (payer.clone(), amount),
    );
}
