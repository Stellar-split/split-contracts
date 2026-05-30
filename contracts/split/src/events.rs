use soroban_sdk::{symbol_short, Address, Bytes, Env, Vec};

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

/// Emitted once per unique payer when their refund is transferred.
pub fn payer_refunded(env: &Env, invoice_id: u64, payer: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("pay_ref"), invoice_id),
        (payer.clone(), amount),
    );
}
