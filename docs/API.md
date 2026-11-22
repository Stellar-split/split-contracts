# Contract API Reference

Full reference for all public entry points in the StellarSplit Soroban contract (`contracts/split/src/lib.rs`).

## Table of Contents

- [Initialization](#initialization)
- [Admin Management](#admin-management)
- [Pause & Circuit Breaker](#pause--circuit-breaker)
- [Invoice Lifecycle](#invoice-lifecycle)
- [Payment](#payment)
- [Release & Refund](#release--refund)
- [Invoice Controls](#invoice-controls)
- [Groups & Treasury](#groups--treasury)
- [Channels](#channels)
- [Disputes](#disputes)
- [Templates & Cloning](#templates--cloning)
- [Subscriptions](#subscriptions)
- [Confidential Payments](#confidential-payments)
- [Timelock](#timelock)
- [Fee Management](#fee-management)
- [Analytics](#analytics)
- [Batch Queries](#batch-queries)
- [Error Recovery](#error-recovery)
- [Read Functions](#read-functions)
- [Validation Rules](#validation-rules)

---

## Initialization

### `initialize`

```rust
pub fn initialize(
    env: Env,
    admin: Address,
    usdc_token: Address,
    treasury: Address,
    platform_fee_bps: u32,
    creation_fee: i128,
)
```

One-time setup. Sets the contract admin, USDC token address, treasury, platform fee basis points, and creation fee. Panics if called more than once.

`platform_fee_bps` must not exceed `MAX_PLATFORM_FEE_BPS` (see [Validation Rules](#validation-rules)); `usdc_token` must implement the SEP-41 interface (see [Token Contract Validation](#token-contract-validation)).

---

## Admin Management

### `add_admin`

```rust
pub fn add_admin(env: Env, admin: Address, new_admin: Address, role: AdminRole)
```

Grants `new_admin` the specified role (`SuperAdmin` or `Operator`). Requires `admin` auth.

### `remove_admin`

```rust
pub fn remove_admin(env: Env, admin: Address, target: Address)
```

Revokes all admin roles from `target`. Requires `admin` auth.

### `propose_admin`

```rust
pub fn propose_admin(env: Env, admin: Address, new_admin: Address)
```

Two-step admin transfer: proposes `new_admin` as the pending admin.

### `accept_admin`

```rust
pub fn accept_admin(env: Env)
```

Completes the two-step admin transfer. Must be called by the pending admin.

### `whitelist_creator`

```rust
pub fn whitelist_creator(env: Env, admin: Address, address: Address)
```

Adds `address` to the creator whitelist. When the whitelist is non-empty, only whitelisted addresses can call `create_invoice`.

### `remove_creator`

```rust
pub fn remove_creator(env: Env, admin: Address, address: Address)
```

Removes `address` from the creator whitelist.

---

## Pause & Circuit Breaker

### `pause`

```rust
pub fn pause(env: Env, admin: Address)
```

Pauses the contract. All state-mutating operations are blocked except for addresses with a pause exemption.

### `unpause`

```rust
pub fn unpause(env: Env, admin: Address)
```

Resumes normal contract operation.

### `is_paused`

```rust
pub fn is_paused(env: Env) -> bool
```

Returns whether the contract is currently paused.

### `pause_function`

```rust
pub fn pause_function(env: Env, admin: Address, function: Symbol)
```

Pauses a single named function while leaving others active.

### `unpause_function`

```rust
pub fn unpause_function(env: Env, admin: Address, function: Symbol)
```

Re-enables a previously paused function.

### `set_pause_exempt`

```rust
pub fn set_pause_exempt(env: Env, admin: Address, address: Address, exempt: bool)
```

Grants or revokes pause exemption for `address`. Exempt addresses can still call `create_invoice` while the contract is paused (but not while the circuit breaker is active).

### `activate_circuit_breaker`

```rust
pub fn activate_circuit_breaker(env: Env, admin: Address, reason: String)
```

Activates the emergency circuit breaker. Blocks **all** state-mutating operations including invoice creation — no exemptions apply. Emits `circuit_breaker_activated`.

### `deactivate_circuit_breaker`

```rust
pub fn deactivate_circuit_breaker(env: Env, admin: Address)
```

Deactivates the circuit breaker and returns the contract to normal operation. Emits `circuit_breaker_deactivated`.

### `get_circuit_breaker_status`

```rust
pub fn get_circuit_breaker_status(env: Env) -> CircuitBreakerStatus
```

Returns current circuit breaker state: `{ active: bool, reason: Option<String> }`.

---

## Invoice Lifecycle

### `create_invoice`

```rust
pub fn create_invoice(
    env: Env,
    creator: Address,
    recipients: Vec<Address>,
    amounts: Vec<i128>,
    token: Address,
    deadline: u64,
    options: InvoiceOptions,
) -> u64
```

Creates a new invoice. Returns the auto-incremented invoice ID.

**Parameters:**

| Name | Type | Description |
|------|------|-------------|
| `creator` | `Address` | Invoice creator; must sign the transaction |
| `recipients` | `Vec<Address>` | Ordered list of recipient addresses |
| `amounts` | `Vec<i128>` | Amount owed to each recipient in token units; `amounts[i]` → `recipients[i]` |
| `token` | `Address` | SAC token contract address (typically USDC). Must implement the SEP-41 interface (see [Token Contract Validation](#token-contract-validation)) |
| `deadline` | `u64` | Unix timestamp after which the invoice can be refunded if unfunded. Must be strictly greater than the current ledger timestamp and no more than `MAX_DEADLINE_HORIZON` seconds ahead (see [Validation Rules](#validation-rules)) |
| `options` | `InvoiceOptions` | Extended options struct (see below) |

**`InvoiceOptions` fields:**

| Field | Type | Description |
|-------|------|-------------|
| `co_creators` | `Vec<Address>` | Additional creators who share admin rights on the invoice |
| `allow_early_withdrawal` | `bool` | Allow creator to release before fully funded |
| `bonus_pool` | `i128` | Extra bonus amount distributed to early payers |
| `bonus_max_payers` | `u32` | Max payers eligible for the bonus |
| `creator_cosigner` | `Option<Address>` | Required co-author for creator actions |
| `velocity_limit` | `i128` | Max token units a single payer can send per `velocity_window` |
| `velocity_window` | `u64` | Window length in seconds for per-payer velocity limiting |
| `prerequisite_id` | `Option<u64>` | Block release until the referenced invoice is Released |
| `tranches` | `Vec<Tranche>` | Graduated release schedule (`timestamp`, `basis_points`) |
| `co_signers` | `Vec<Address>` | Addresses whose approval is required before release |
| `required_signatures` | `u32` | Number of co-signer approvals required (≤ `co_signers.len()`) |
| `penalty_bps` | `Option<u32>` | Late-payment penalty in basis points |
| `penalty_deadline` | `Option<u64>` | Soft deadline; payments after this incur `penalty_bps` |
| `min_funding_bps` | `Option<u32>` | Minimum funding threshold (e.g. 5000 = 50%) to allow release |
| `release_stages` | `Vec<u32>` | Creator-triggered staged release; each entry is basis points, must sum to 10 000 |
| `price_oracle` | `Option<Address>` | Oracle contract for dynamic pricing |
| `swap_tokens` | `Vec<Option<Address>>` | Per-recipient preferred output token for DEX swap on release |
| `tax_bps` | `Option<u32>` | Tax deduction in basis points, routed to `tax_authority` |
| `tax_authority` | `Option<Address>` | Recipient of the tax deduction |
| `insurance_premium_bps` | `Option<u32>` | Insurance premium in basis points |
| `smart_route` | `Option<bool>` | Enable smart routing for token swaps on release |
| `notification_contract` | `Option<Address>` | External contract called on status change |
| `overflow_behavior` | `OverflowBehavior` | What to do with overpayment: `Reject`, `Refund`, or `Donate` |
| `convert_to_stream` | `bool` | Register funds with the stream contract on release instead of direct transfer |
| `accepted_tokens` | `Vec<Address>` | Additional tokens accepted in `pay_with_token` |
| `forward_to` | `Option<Address>` | Auto-forward leftover funds to this address |
| `forward_invoice_id` | `Option<u64>` | Auto-forward leftover funds to another invoice |
| `split_rules` | `Vec<SplitRule>` | Per-recipient split rules (`Fixed`, `Percentage`, `Tiered`) evaluated at release. Must be non-empty when provided, each ratio must be in `(0, MAX_RATIO]`, and ratios must sum to exactly `10_000` bps (see [Validation Rules](#validation-rules)) |
| `auto_resolve_rules` | `Vec<ResolveRule>` | Pre-agreed auto-resolution rules evaluated when `auto_resolve` is called |
| `oracle_address` | `Option<Address>` | Oracle that must confirm a condition before release |
| `cross_chain_ref` | `Option<String>` | Cross-chain reference identifier |
| `allowed_payers` | `Option<Vec<Address>>` | Restrict payments to this allowlist; `None` = open |
| `payment_cooldown_secs` | `Option<u64>` | Per-payer cooldown window in seconds |
| `max_payments_per_window` | `Option<u32>` | Max payments per payer per window |
| `payment_window_secs` | `Option<u64>` | Window duration for payment rate limiting |
| `priorities` | `Vec<u32>` | Per-recipient release priority ordering |
| `refund_grace_secs` | `Option<u64>` | Grace period in seconds after deadline before refund is allowed |
| `scheduled_release_at` | `Option<u64>` | Scheduled release Unix timestamp |
| `require_kyc` | `bool` | Require payers to pass KYC before paying |

**Input validation (issue #931):**

Core inputs are validated before any storage is written. Each failure aborts with a typed `ContractError`, not a string panic:

| Check | Error |
|-------|-------|
| `recipients.len() != amounts.len()` | `InvalidRecipients` (16) |
| `recipients` is empty | `EmptyRecipientList` (37) |
| more than `MAX_INVOICE_RECIPIENTS` (200) recipients | `BatchLimitExceeded` (18) |
| any amount ≤ 0 | `ZeroAmountNotAllowed` (41) |
| `deadline` ≤ current ledger timestamp | `DeadlinePassed` (3) |
| `deadline` more than `MAX_DEADLINE_HORIZON_SECS` (~10 years) ahead | `InvalidDeadline` (67) |
| a recipient has a malformed strkey or bad checksum, is an all-zero key, or is the contract itself (issue #932) | `InvalidRecipientAddress` (68) |
| sum of `amounts` overflows `i128` or exceeds `MAX_INVOICE_TOTAL` (issue #933) | `ArithmeticOverflow` (40) |

The same checks apply to `create_invoices_batch`. Recipient address validation also applies to `add_recipient`. The recipients/amounts checks also apply to `create_subscription` and `save_template`.

**Amount limits (issue #933):**

| Constant | Value | Meaning |
|----------|-------|---------|
| `MAX_AMOUNT_SCALE` | `10_000_000_000` | Largest factor applied to an amount in intermediate math: basis points (10⁴) × oracle rate scale (10⁶) |
| `MAX_INVOICE_TOTAL` | `i128::MAX / MAX_AMOUNT_SCALE` ≈ 1.7 × 10²⁸ base units | Maximum sum of an invoice's `amounts` (~1.7 × 10²¹ whole tokens at 7 decimals) |

Keeping totals under `MAX_INVOICE_TOTAL` means `total × bps` and `total × rate × bps` can never overflow. Overflow is also checked where amounts accumulate after creation:
- `invoice.funded` updates in `pay`, `contribute`, `pay_with_token`, `bridge_pay`, `pool_pay` and `close_channel` use checked addition and fail with `ArithmeticOverflow` instead of trapping.
- `add_recipient` rejects a new amount that would push the total past `MAX_INVOICE_TOTAL`.
- `split_rules` basis-point totals are summed with checked arithmetic. A negative `Fixed` amount fails with `InvalidAmount`.

### `create_batch`

```rust
pub fn create_batch(
    env: Env,
    creator: Address,
    params: Vec<CreateInvoiceParams>,
) -> Vec<u64>
```

Creates multiple invoices atomically. Returns a vector of invoice IDs. All invoices use the same creator.

### `create_invoices_batch`

```rust
pub fn create_invoices_batch(
    env: Env,
    creator: Address,
    params: Vec<CreateInvoiceParams>,
    options: InvoiceOptions,
) -> Vec<u64>
```

Batch creation with shared `InvoiceOptions` applied to all invoices.

### `create_subscription`

```rust
pub fn create_subscription(
    env: Env,
    creator: Address,
    recipients: Vec<Address>,
    amounts: Vec<i128>,
    tokens: Vec<Address>,
) -> u64
```

Creates a recurring subscription invoice. Returns the invoice ID.

### `clone_invoice`

```rust
pub fn clone_invoice(
    env: Env,
    creator: Address,
    source_id: u64,
    overrides: CloneOverrides,
) -> u64
```

Creates a new invoice as a clone of `source_id`, optionally overriding deadline, amounts, recipients, or overflow behavior. Tracks lineage via `parent_invoice_id`. Emits `invoice_cloned`.

### `migrate_invoice`

```rust
pub fn migrate_invoice(env: Env, admin: Address, invoice_id: u64)
```

Admin-only: migrates a legacy invoice format to the current schema.

---

## Payment

### `pay`

```rust
pub fn pay(
    env: Env,
    payer: Address,
    invoice_id: u64,
    amount: i128,
    nonce: u64,
    _auto_convert: bool,
    donate_on_failure: bool,
)
```

Core payment entry point. Transfers `amount` of the invoice token from `payer` to the contract. Auto-releases if fully funded and no guards are active. `nonce` prevents replay. Emits `payment_received`.

**`payment_received` event data:**

```rust
pub struct PaymentEvent {
    pub invoice_id: u64,
    pub payer: Address,
    pub amount: i128,            // gross amount transferred by the payer
    pub breakdown: PaymentBreakdown,
    pub metadata: PaymentMetadata,
}

pub struct PaymentBreakdown {
    pub principal: i128,         // amount applied to the invoice total
    pub platform_fee: i128,      // platform fee deducted at payment time (if any)
    pub penalty: i128,           // late-payment penalty applied (see `penalty_bps`)
    pub bonus: i128,             // bonus-pool contribution credited to this payer
    pub tax: i128,               // tax deduction routed to `tax_authority`
    pub insurance: i128,         // insurance premium deducted
    pub net_to_invoice: i128,    // amount credited toward funding after deductions
}

pub struct PaymentMetadata {
    pub token: Address,          // token actually transferred
    pub nonce: u64,              // replay-protection nonce
    pub via: Option<Address>,    // intermediary (memo pay / channel / bridge relayer)
    pub memo: Option<u64>,       // memo ID when paid via `pay_with_memo`
    pub source_chain: Option<String>, // set for `bridge_pay`
    pub timestamp: u64,          // ledger timestamp of the payment
}
```

`breakdown` components always sum to `amount` (`principal + platform_fee + penalty + tax + insurance == amount`; `bonus` and `net_to_invoice` are informational). All fields are fixed-width or `Option`-wrapped so the event remains cheaply indexable by off-chain consumers.

### `pay_with_attestation`

```rust
pub fn pay_with_attestation(
    env: Env,
    payer: Address,
    invoice_id: u64,
    amount: i128,
    nonce: u64,
    attestation_hash: BytesN<32>,
    signature: BytesN<64>,
    signer_pubkey: BytesN<32>,
    _auto_convert: bool,
)
```

Payment with an off-chain attestation (e.g. proof of work completion). The attestation is recorded on-chain.

### `pay_with_token`

```rust
pub fn pay_with_token(
    env: Env,
    payer: Address,
    invoice_id: u64,
    amount: i128,
    token: Address,
    nonce: u64,
)
```

Pay using an alternative token listed in `accepted_tokens`. The contract swaps to the invoice base token via the configured DEX.

### `pay_with_memo`

```rust
pub fn pay_with_memo(
    env: Env,
    payer: Address,
    memo: u64,
    amount: i128,
    nonce: u64,
    _auto_convert: bool,
    via: Option<Address>,
)
```

Pay by memo ID rather than explicit invoice ID. Useful for payment-link flows.

### `bridge_pay`

```rust
pub fn bridge_pay(
    env: Env,
    payer: Address,
    invoice_id: u64,
    amount: i128,
    source_chain: String,
    bridge_ref: String,
)
```

Records a cross-chain bridge payment. The bridge relayer calls this after funds arrive on Stellar.

### `pool_pay`

```rust
pub fn pool_pay(env: Env, payer: Address, payments: Vec<InvoicePayment>)
```

Batched payment across multiple invoices in a single transaction. Each `InvoicePayment` is `{ invoice_id: u64, amount: i128 }`.

### `pay_confidential`

```rust
pub fn pay_confidential(
    env: Env,
    payer: Address,
    invoice_id: u64,
    commitment: BytesN<32>,
    range_proof: Bytes,
)
```

Confidential payment where the amount is hidden. Stores a Pedersen commitment and range proof on-chain. The actual amount is revealed later via `reveal_confidential_total`. Emits `payment_received` with amount 0.

### `reveal_confidential_total`

```rust
pub fn reveal_confidential_total(
    env: Env,
    invoice_id: u64,
    decrypted_sum: i128,
    range_proof: BytesN<32>,
)
```

Reveals the total funded amount for a confidential invoice. Once revealed, normal release logic applies.

### `get_confidential_payment_count`

```rust
pub fn get_confidential_payment_count(env: Env, invoice_id: u64) -> u32
```

Returns the number of confidential payment commitments recorded for an invoice.

### `compress_payments`

```rust
pub fn compress_payments(env: Env, invoice_id: u64)
```

Aggregates all payment shards into a single compact record to save storage rent.

---

## Release & Refund

### `release`

```rust
pub fn release(env: Env, invoice_id: u64)
```

Routes funds to all recipients. Can be called by anyone once the invoice is fully funded and all guards are cleared (co-signers, prerequisite, tranches, oracle condition, min funding, scheduled release). Emits `invoice_released`.

### `release_to_recipient`

```rust
pub fn release_to_recipient(env: Env, invoice_id: u64, recipient: Address)
```

Releases the pending payout to a single recipient. Used when the invoice has per-recipient pending payouts.

### `trigger_scheduled_release`

```rust
pub fn trigger_scheduled_release(env: Env, invoice_id: u64)
```

Triggers release for an invoice that has a `scheduled_release_at` timestamp that has passed.

### `sign_release`

```rust
pub fn sign_release(env: Env, invoice_id: u64, signer: Address)
```

Records a co-signer approval. Once `required_signatures` approvals are collected, `release` becomes callable. Emits `invoice_partially_released` when threshold is reached.

### `sign_release_with_signature`

```rust
pub fn sign_release_with_signature(
    env: Env,
    invoice_id: u64,
    signer: Address,
    nonce: u64,
    signature: BytesN<64>,
)
```

Records a co-signer approval backed by an off-chain Ed25519 signature over the canonical co-signer message (see [Co-Signer Signature Format](#co-signer-signature-format)). The signature is verified against `signer` before the approval is recorded.

#### Co-Signer Signature Format

Co-signer signatures use **domain separation** and **nonce binding** to prevent cross-contract and replay attacks.

**Signed payload (canonical byte layout):**

```
domain_tag (32 bytes) || network_id (32 bytes) || contract_id (32 bytes)
  || invoice_id (u64, big-endian, 8 bytes)
  || signer (Address, XDR-encoded, variable length)
  || nonce (u64, big-endian, 8 bytes)
```

| Field | Description |
|-------|-------------|
| `domain_tag` | Fixed constant `SHA-256("StellarSplit:co-signer:v1")`; separates this signature domain from all other signed messages |
| `network_id` | Stellar network passphrase hash (`Public Network`, `Test SDF Network ; September 2015`, etc.) |
| `contract_id` | The StellarSplit contract address, binding the signature to this deployment |
| `invoice_id` | Invoice the approval applies to |
| `signer` | The co-signer address whose key produced the signature |
| `nonce` | Strictly increasing per-`(invoice_id, signer)` nonce |

**Verification rules:**

- The `domain_tag`, `network_id`, and `contract_id` must match the current environment; mismatches are rejected with `ContractError::InvalidSignatureDomain`.
- `nonce` must be strictly greater than the last recorded nonce for `(invoice_id, signer)`; otherwise the call fails with `ContractError::SignatureNonceReused`.
- The signature must verify as Ed25519 over the payload above using `signer`'s public key; failures return `ContractError::InvalidSignature`.
- A signer may not sign the same invoice twice; duplicate approvals return `ContractError::AlreadySigned`.
- Signatures are only accepted while the invoice is in a releasable state (funded, not paused/frozen, not refunded); otherwise `ContractError::InvalidInvoiceState`.

**Replay protection:** because the payload binds the contract ID, invoice ID, signer, and a monotonically increasing nonce, a captured signature cannot be replayed on another contract, another invoice, or a later transaction. The last-used nonce is persisted per `(invoice_id, signer)` and never decreases.

### `refund`

```rust
pub fn refund(env: Env, invoice_id: u64)
```

Refunds all payers proportionally. Can be called by anyone after `deadline` passes if the invoice is not fully funded (and `refund_grace_secs` has elapsed if set). Emits `invoice_refunded` and per-payer `payer_refunded`.

**`RefundReason` enum:**

```rust
pub enum RefundReason {
    DeadlineExpired,      // deadline passed without full funding
    CreatorCancelled,     // creator explicitly cancelled the invoice
    DisputeResolved,      // arbiter resolved the dispute in favour of refund
    AdminOverride,        // admin force-refunded the invoice
    PrerequisiteFailed,   // prerequisite invoice was refunded
    OracleConditionFailed,// oracle confirmed the condition was not met
    OverflowRefund,       // overpayment returned per `OverflowBehavior::Refund`
    Manual,               // any other caller-initiated refund
}
```

Both `invoice_refunded` and `payer_refunded` events carry a `reason: RefundReason` field so consumers can index refunds by cause. The reason is derived from the call path (e.g. `refund` after deadline → `DeadlineExpired`, `resolve_dispute` with `ResolveAction::Refund` → `DisputeResolved`) and is never caller-supplied.

### `simulate_release`

```rust
pub fn simulate_release(env: Env, invoice_id: u64) -> SimulateReleaseResult
```

Estimates the compute cost of releasing an invoice. Returns `{ cpu_instructions: u64, stroops_estimate: i128 }`. Does not mutate state.

---

## Invoice Controls

### `approve_invoice`

```rust
pub fn approve_invoice(env: Env, invoice_id: u64)
```

Approver confirms the invoice. Required before release when an approver is set.

### `pause_invoice`

```rust
pub fn pause_invoice(
    env: Env,
    creator: Address,
    invoice_id: u64,
    reason: Option<String>,
    auto_resume_at: Option<u64>,
)
```

Creator pauses a specific invoice, blocking payments and release. Optionally sets an `auto_resume_at` timestamp. Emits `invoice_paused`.

### `resume_invoice`

```rust
pub fn resume_invoice(env: Env, creator: Address, invoice_id: u64)
```

Creator resumes a paused invoice. Emits `invoice_resumed`.

### `admin_freeze`

```rust
pub fn admin_freeze(env: Env, admin: Address, invoice_id: u64, reason: String)
```

Admin-level freeze that overrides creator controls. Emits `invoice_admin_frozen`.

### `admin_unfreeze`

```rust
pub fn admin_unfreeze(env: Env, admin: Address, invoice_id: u64)
```

Lifts an admin freeze. Emits `invoice_admin_unfrozen`.

### `admin_force_resume`

```rust
pub fn admin_force_resume(env: Env, admin: Address, invoice_id: u64)
```

Admin override to resume an invoice paused by the creator.

### `confirm_condition`

```rust
pub fn confirm_condition(env: Env, invoice_id: u64)
```

Called by the configured `oracle_address` to confirm the off-chain condition is met, unblocking release.

#### Oracle Interface

When `InvoiceOptions.oracle_address` is set, release is blocked until the oracle confirms the condition.

**Address format:**

- `oracle_address` must be a valid Stellar contract address (`C...`, 56-character StrKey) or account address (`G...`, 56-character StrKey).
- The address is validated at `create_invoice` time; malformed addresses are rejected with `ContractError::InvalidOracleAddress`.
- The zero address and the contract's own address are rejected with `ContractError::InvalidOracleAddress`.
- If `oracle_address` is `None`, no oracle gate applies and `confirm_condition` is a no-op that returns `ContractError::NoOracleConfigured`.

**Condition semantics:**

- The oracle condition is a boolean gate: once confirmed, it stays confirmed for the lifetime of the invoice and cannot be un-confirmed.
- `confirm_condition` must be authorized by `oracle_address`; any other caller fails with `ContractError::Unauthorized`.
- Confirmation is only accepted while the invoice is `Pending` or `Funded`; otherwise `ContractError::InvalidInvoiceState`.
- Confirming twice returns `ContractError::ConditionAlreadyConfirmed`.
- The oracle may also be consulted for dynamic pricing when `price_oracle` is set; that oracle must return a positive price, otherwise release fails with `ContractError::InvalidOraclePrice`.
- Oracle calls are read-only from the contract's perspective; the contract never trusts oracle-supplied amounts for accounting, only the boolean condition and price feed.

### `add_allowed_payer`

```rust
pub fn add_allowed_payer(env: Env, creator: Address, invoice_id: u64, payer: Address)
```

Adds `payer` to the invoice's allowed payer list. Only the creator can call this.

### `remove_allowed_payer`

```rust
pub fn remove_allowed_payer(env: Env, creator: Address, invoice_id: u64, payer: Address)
```

Removes `payer` from the allowed payer list.

### `update_metadata_hash`

```rust
pub fn update_metadata_hash(env: Env, invoice_id: u64, creator: Address, new_hash: BytesN<32>)
```

Updates the off-chain metadata hash (IPFS CID or SHA-256) associated with the invoice.

### `set_reminder`

```rust
pub fn set_reminder(env: Env, who: Address, invoice_id: u64, remind_at: u64)
```

Schedules a reminder event for `who` at `remind_at` timestamp.

### `trigger_reminder`

```rust
pub fn trigger_reminder(env: Env, invoice_id: u64, who: Address)
```

Fires the reminder event if `remind_at` has passed. Emits `payment_reminder`.

---

## Groups & Treasury

### `group_treasury_create`

```rust
pub fn group_treasury_create(
    env: Env,
    creator: Address,
    invoice_ids: Vec<u64>,
    treasury: Address,
) -> u64
```

Groups a set of invoices under a shared treasury. Returns a group ID. All invoices must release before the treasury distributes funds.

---

## Channels

### `open_channel`

```rust
pub fn open_channel(env: Env, payer: Address, invoice_id: u64, deposit: i128)
```

Opens a payment channel for streaming micro-payments toward an invoice.

### `channel_pay`

```rust
pub fn channel_pay(env: Env, payer: Address, invoice_id: u64, amount: i128)
```

Sends a micro-payment through an open channel.

### `close_channel`

```rust
pub fn close_channel(env: Env, payer: Address, invoice_id: u64)
```

Closes the channel and settles the final balance.

---

## Disputes

### `set_arbiter`

```rust
pub fn set_arbiter(env: Env, admin: Address, invoice_id: u64, arbiter: Address)
```

Assigns an arbiter to an invoice. Only admins can set arbiters.

### `raise_dispute`

```rust
pub fn raise_dispute(env: Env, invoice_id: u64, arbiter: Address)
```

Raises a dispute for an invoice. Blocks release until the arbiter resolves it.

### `resolve_dispute`

```rust
pub fn resolve_dispute(env: Env, invoice_id: u64, arbiter: Address, resolution: ResolveAction)
```

Arbiter resolves the dispute with either `ResolveAction::Release` or `ResolveAction::Refund`.

---

## Templates & Cloning

### `set_template`

```rust
pub fn set_template(
    env: Env,
    creator: Address,
    name: Symbol,
    recipients: Vec<Address>,
    amounts: Vec<i128>,
    token: Address,
)
```

Saves a reusable invoice template under `(creator, name)`.

### `get_template`

```rust
pub fn get_template(env: Env, creator: Address, name: Symbol) -> InvoiceTemplate
```

Returns the template for `(creator, name)`.

---

## Subscriptions

### `create_subscription`

```rust
pub fn create_subscription(
    env: Env,
    creator: Address,
    recipients: Vec<Address>,
    amounts: Vec<i128>,
    tokens: Vec<Address>,
) -> u64
```

Creates a subscription invoice. The subscription params are stored and can be used to spawn recurring invoices.

---

## Confidential Payments

See the [Payment](#payment) section for `pay_confidential`, `reveal_confidential_total`, and `get_confidential_payment_count`.

See [`docs/ISSUE_295_CONFIDENTIAL_PAYMENTS.md`](./ISSUE_295_CONFIDENTIAL_PAYMENTS.md) for full design notes.

---

## Timelock

### `set_timelock_secs`

```rust
pub fn set_timelock_secs(env: Env, admin: Address, secs: u64)
```

Sets the global timelock delay in seconds. Admin actions queued via `queue_action` must wait this long before execution.

### `queue_action`

```rust
pub fn queue_action(env: Env, admin: Address, action: TimelockAction) -> u64
```

Queues a timelocked admin action. Returns an `action_id`. Emits `action_queued`.

### `execute_action`

```rust
pub fn execute_action(env: Env, action_id: u64)
```

Executes a queued action once the timelock has elapsed. Emits `action_executed`.

### `cancel_action`

```rust
pub fn cancel_action(env: Env, admin: Address, action_id: u64)
```

Cancels a pending timelocked action. Emits `action_cancelled`.

---

## Fee Management

### `set_creation_fee`

```rust
pub fn set_creation_fee(env: Env, admin: Address, creation_fee: i128)
```

Sets the flat fee charged per `create_invoice` call, deducted from the creator and sent to treasury.

### `get_creation_fee`

```rust
pub fn get_creation_fee(env: Env) -> i128
```

Returns the current creation fee.

### `set_fee_tiers`

```rust
pub fn set_fee_tiers(env: Env, admin: Address, tiers: Vec<FeeTier>)
```

Sets volume-based platform fee tiers. Each `FeeTier` has `{ volume_threshold: u64, fee_bps: u32 }`. Tiers must be sorted by threshold ascending. Max 5 tiers. Each `fee_bps` must not exceed `MAX_PLATFORM_FEE_BPS` (see [Validation Rules](#validation-rules)).

### `get_fee_tiers`

```rust
pub fn get_fee_tiers(env: Env) -> Vec<FeeTier>
```

Returns all configured fee tiers.

### `get_applicable_fee`

```rust
pub fn get_applicable_fee(env: Env, creator: Address) -> u32
```

Returns the effective fee basis points for `creator` based on their lifetime volume and the configured tiers.

### `get_platform_fee_bps`

```rust
pub fn get_platform_fee_bps(env: Env) -> u32
```

Returns the global platform fee in basis points.

### `add_platform_fee_waiver`

```rust
pub fn add_platform_fee_waiver(env: Env, admin: Address, address: Address)
```

Adds `address` to the platform fee waiver list. Waived addresses pay 0% platform fee at release.

### `remove_platform_fee_waiver`

```rust
pub fn remove_platform_fee_waiver(env: Env, admin: Address, address: Address)
```

Removes `address` from the fee waiver list.

### `is_platform_fee_waived`

```rust
pub fn is_platform_fee_waived(env: Env, address: Address) -> bool
```

Returns whether `address` is on the fee waiver list.

### `set_creator_volume_cap`

```rust
pub fn set_creator_volume_cap(env: Env, admin: Address, creator: Address, cap: i128)
```

Sets a per-creator lifetime volume cap. Creation is blocked once the cap is reached.

---

## Analytics

### `get_creator_stats`

```rust
pub fn get_creator_stats(env: Env, creator: Address) -> CreatorStats
```

Returns aggregated stats for a creator:

```rust
pub struct CreatorStats {
    pub total_invoices: u32,      // invoices created
    pub total_raised: u64,        // total amount raised across all invoices
    pub total_released: u64,      // total amount released to recipients
    pub total_payers: u32,        // unique payer count
    pub avg_funding_time_ledgers: u32, // running average funding time in ledgers
}
```

### `get_creator_volume_used`

```rust
pub fn get_creator_volume_used(env: Env, creator: Address) -> i128
```

Returns the creator's total lifetime invoice volume (used for fee tier qualification).

---

## Batch Queries

### `batch_query`

```rust
pub fn batch_query(env: Env, queries: Vec<Query>) -> Vec<QueryResult>
```

Executes multiple read queries in a single call, reducing RPC round-trips and per-call overhead. Returns exactly one `QueryResult` per input `Query`, **in the same order as the input**.

```rust
pub enum Query {
    Invoice(u64),                 // equivalent to `get_invoice`
    InvoiceSnapshot(u64),         // equivalent to `get_invoice_snapshot`
    InvoiceExt3(u64),             // equivalent to `get_invoice_ext3`
    AuditLog(u64),                // equivalent to `get_audit_log`
    CreatorStats(Address),        // equivalent to `get_creator_stats`
    CreatorVolumeUsed(Address),   // equivalent to `get_creator_volume_used`
    ReceiptToken(u64, Address),   // equivalent to `get_receipt_token`
    IsPaused,                     // equivalent to `is_paused`
    UsdcToken,                    // equivalent to `get_usdc_token`
    Treasury,                     // equivalent to `get_treasury`
}

pub enum QueryResult {
    Invoice(Invoice),
    InvoiceSnapshot(InvoiceSnapshot),
    InvoiceExt3(InvoiceExt3),
    AuditLog(Vec<AuditEntry>),
    CreatorStats(CreatorStats),
    CreatorVolumeUsed(i128),
    ReceiptToken(Option<Address>),
    IsPaused(bool),
    UsdcToken(Address),
    Treasury(Address),
    NotFound,                     // invoice ID does not exist
}
```

**Ordering:** results are positionally aligned with `queries`; `results[i]` answers `queries[i]`. Callers must not assume any other ordering.

**Deduplication:** identical queries within a single batch (same variant and arguments) are evaluated once and the cached result is reused for every occurrence. Duplicates still produce a result entry at each of their positions, so the output length always equals `queries.len()`.

**Limits:** a batch may contain at most 50 queries; exceeding this panics with `BatchTooLarge`. An empty batch returns an empty vector.

**Errors:** a missing invoice yields `QueryResult::NotFound` rather than panicking, so one bad ID does not fail the whole batch. Malformed input (e.g. exceeding the batch limit) panics.

---

## Error Recovery

### `get_recovery_suggestion`

```rust
pub fn get_recovery_suggestion(env: Env, error_code: u32) -> RecoverySuggestion
```

Returns a human-readable recovery suggestion for a contract error code, without mutating state. Intended for wallets and dashboards to surface actionable guidance when a transaction fails.

```rust
pub struct RecoverySuggestion {
    pub error_code: u32,
    pub message: String,          // short description of the error
    pub suggestion: String,       // recommended next action
    pub retryable: bool,          // whether retrying the same call may succeed
    pub related_entrypoint: Option<Symbol>, // entrypoint to call to recover, if any
}
```

Unknown error codes return a generic suggestion with `retryable: false` and `related_entrypoint: None`. Recovery never mutates contract state; callers remain responsible for re-submitting transactions.

---

## Read Functions

### `get_invoice`

```rust
pub fn get_invoice(env: Env, invoice_id: u64) -> Invoice
```

Returns the full merged invoice view (hot fields + core + ext).

#### Recipient List Caching

Recipient lists (`recipients`, `amounts`, and derived split rules) are cached to avoid repeated storage reads during payment and release.

**Cache behaviour:**

- The cache key is `(invoice_id, "recipients")` and stores the resolved recipient/amount vectors plus a version counter.
- Reads of `get_invoice`, `release`, `release_to_recipient`, and `simulate_release` populate the cache on first access.
- Cache hits return the stored vectors without touching persistent storage; misses fall back to storage and repopulate the cache.
- The cache is bounded per invoice and evicted on invoice archival.

**Invalidation rules:**

- Any mutation of `recipients`, `amounts`, `split_rules`, or `priorities` bumps the invoice version and invalidates the cache entry.
- `clone_invoice` and `migrate_invoice` invalidate the source invoice's cache.
- `refund` and `release` invalidate the cache after final distribution.
- Invalidation is version-checked: a cached entry whose version does not match the invoice's current version is treated as a miss and discarded.
- Cache state is never authoritative; correctness always falls back to persistent storage.

### `get_invoice_snapshot`

```rust
pub fn get_invoice_snapshot(env: Env, invoice_id: u64) -> InvoiceSnapshot
```

Returns a compact snapshot: `{ id, status, funded, total, creator, deadline }`.

### `get_invoice_ext3`

```rust
pub fn get_invoice_ext3(env: Env, invoice_id: u64) -> InvoiceExt3
```

Returns Wave 6 extended fields including `release_delay_ledgers`, `metadata_hash`, `payment_token`, and `target_usd_cents`.

### `get_audit_log`

```rust
pub fn get_audit_log(env: Env, id: u64) -> Vec<AuditEntry>
```

Returns the ordered list of `{ action, actor, timestamp }` entries for an invoice.

### `get_receipt_token`

```rust
pub fn get_receipt_token(env: Env, invoice_id: u64, payer: Address) -> Option<Address>
```

Returns the NFT receipt token address minted to `payer` for this invoice, if any.

### `get_usdc_token`

```rust
pub fn get_usdc_token(env: Env) -> Address
```

Returns the configured USDC token contract address.

### `get_treasury`

```rust
pub fn get_treasury(env: Env) -> Address
```

Returns the platform treasury address.

### `get_creator_volume_cap`

```rust
pub fn get_creator_volume_cap(env: Env, creator: Address) -> i128
```

Returns the volume cap set for `creator` (0 = no cap).

### `get_archive_after_ledgers`

```rust
pub fn get_archive_after_ledgers(env: Env) -> u64
```

Returns the number of ledgers after which released/refunded invoices are eligible for archival.

### `get_dashboard_contract`

```rust
pub fn get_dashboard_contract(env: Env) -> Option<Address>
```

Returns the linked dashboard contract address, if configured.

### `is_paused`

See [Pause & Circuit Breaker](#pause--circuit-breaker).

---

## Error Codes

All entry points return `Result<T, ContractError>`. The full variant table:

| Code | Variant | Meaning |
|------|---------|---------|
| 1 | `NotInitialized` | Contract has not been initialized |
| 2 | `AlreadyInitialized` | `initialize` called more than once |
| 3 | `Unauthorized` | Caller lacks the required auth or role |
| 4 | `NotFound` | Referenced invoice, template, or action does not exist |
| 5 | `InvalidAmount` | Amount is zero, negative, or otherwise invalid |
| 6 | `InvalidDeadline` | Deadline is in the past or otherwise invalid |
| 7 | `InvalidInvoiceState` | Operation not allowed in the invoice's current state |
| 8 | `InvoicePaused` | Invoice is paused |
| 9 | `InvoiceFrozen` | Invoice is admin-frozen |
| 10 | `ContractPaused` | Contract is paused |
| 11 | `CircuitBreakerActive` | Emergency circuit breaker is active |
| 12 | `NotFullyFunded` | Release attempted before full funding |
| 13 | `AlreadyReleased` | Invoice already released |
| 14 | `AlreadyRefunded` | Invoice already refunded |
| 15 | `DeadlineNotPassed` | Refund attempted before deadline |
| 16 | `RefundGraceNotElapsed` | Refund attempted before `refund_grace_secs` elapsed |
| 17 | `InsufficientSignatures` | Fewer than `required_signatures` co-signer approvals |
| 18 | `AlreadySigned` | Signer already approved this invoice |
| 19 | `InvalidSignature` | Signature failed Ed25519 verification |
| 20 | `InvalidSignatureDomain` | Signature domain/network/contract mismatch |
| 21 | `SignatureNonceReused` | Signature nonce not greater than last used |
| 22 | `InvalidOracleAddress` | Oracle address malformed or disallowed |
| 23 | `NoOracleConfigured` | Oracle operation attempted with no oracle set |
| 24 | `InvalidOraclePrice` | Oracle returned a non-positive price |
| 25 | `ConditionAlreadyConfirmed` | Oracle condition already confirmed |
| 26 | `ConditionNotConfirmed` | Release attempted before oracle confirmation |
| 27 | `PrerequisiteNotMet` | Prerequisite invoice not released |
| 28 | `TrancheNotReached` | Tranche timestamp not yet reached |
| 29 | `MinFundingNotReached` | `min_funding_bps` threshold not met |
| 30 | `ScheduledReleaseNotDue` | `scheduled_release_at` not yet reached |
| 31 | `VelocityLimitExceeded` | Payer exceeded `velocity_limit` in window |
| 32 | `PaymentCooldownActive` | Payer within `payment_cooldown_secs` |
| 33 | `PaymentRateLimitExceeded` | Payer exceeded `max_payments_per_window` |
| 34 | `PayerNotAllowed` | Payer not in `allowed_payers` |
| 35 | `KycRequired` | Payer has not passed KYC |
| 36 | `TokenNotAccepted` | Token not in `accepted_tokens` |
| 37 | `NonceReused` | Payment nonce already used |
| 38 | `OverflowRejected` | Overpayment rejected by `OverflowBehavior::Reject` |
| 39 | `DisputeActive` | Release blocked by an active dispute |
| 40 | `ArbiterNotSet` | Dispute raised with no arbiter assigned |
| 41 | `TemplateExists` | Template already exists for `(creator, name)` |
| 42 | `TemplateNotFound` | Template does not exist |
| 43 | `CreatorNotWhitelisted` | Creator not on the whitelist |
| 44 | `VolumeCapExceeded` | Creator lifetime volume cap reached |
| 45 | `TimelockNotElapsed` | Queued action executed before timelock |
| 46 | `ActionNotFound` | Timelock action ID does not exist |
| 47 | `ActionAlreadyExecuted` | Timelock action already executed |
| 48 | `InvalidFeeTiers` | Fee tiers unsorted, empty, or exceed max count |
| 49 | `InvalidBasisPoints` | Basis points value exceeds 10 000 |
| 50 | `InvalidRecipients` | Recipients/amounts length mismatch or empty |
| 51 | `InvalidSplitRules` | Split rules invalid or do not sum correctly |
| 52 | `InvalidReleaseStages` | Release stages do not sum to 10 000 |
| 53 | `InvalidTranches` | Tranches unsorted or invalid |
| 54 | `InvalidMetadataHash` | Metadata hash malformed |
| 55 | `InvalidChannelState` | Channel operation invalid for current state |
| 56 | `InsufficientChannelDeposit` | Channel deposit too low |
| 57 | `ConfidentialNotRevealed` | Confidential total not yet revealed |
| 58 | `InvalidRangeProof` | Range proof failed verification |
| 59 | `ArithmeticOverflow` | Arithmetic overflow or underflow |
| 60 | `TransferFailed` | Token transfer failed |

Codes are stable and never reused; new variants are appended with the next available code.

---

*This document reflects the contract as of Wave 6 (commit `7942de8`). Update this file when new entry points are added.*
