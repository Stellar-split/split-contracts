use soroban_sdk::{symbol_short, Address, Env, Vec};

// ---------------------------------------------------------------------------
// Existing events
// ---------------------------------------------------------------------------

/// Emitted when a new invoice is created.
pub fn invoice_created(env: &Env, invoice_id: u64, creator: &Address, total: i128) {
    env.events().publish(
        (symbol_short!("inv_crt"), invoice_id),
        (creator.clone(), total),
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

// ---------------------------------------------------------------------------
// #522 — Cross-Invoice Split Linkage
// ---------------------------------------------------------------------------

/// Emitted on the first successful release of a child invoice, once the
/// parent is confirmed to be finalised.
///
/// Topics: `("child_unblk", child_id)`
/// Data:   `parent_id`
pub fn child_invoice_unblocked(env: &Env, child_id: u64, parent_id: u64) {
    env.events().publish(
        (symbol_short!("chld_ubl"), child_id),
        parent_id,
    );
}

// ---------------------------------------------------------------------------
// #523 — Late Payment Penalty Fee
// ---------------------------------------------------------------------------

/// Emitted every time a late-payment penalty is charged.
///
/// Topics: `("late_pen", invoice_id)`
/// Data:   `(payer, penalty_amount)`
pub fn late_payment_penalty_charged(
    env: &Env,
    invoice_id: u64,
    payer: &Address,
    penalty_amount: i128,
) {
    env.events().publish(
        (symbol_short!("late_pen"), invoice_id),
        (payer.clone(), penalty_amount),
    );
}

// ---------------------------------------------------------------------------
// #524 — Invoice Batch Creation
// ---------------------------------------------------------------------------

/// Emitted once after a successful `batch_create_invoices` call, carrying
/// all newly created invoice IDs in creation order.
///
/// Topics: `("batch_crt",)`
/// Data:   `ids: Vec<u64>`
pub fn batch_invoice_created(env: &Env, ids: &Vec<u64>) {
    env.events()
        .publish((symbol_short!("btch_crt"),), ids.clone());
}
