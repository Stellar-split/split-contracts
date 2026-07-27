use soroban_sdk::{contracttype, Address, Bytes, BytesN, Env, String, Symbol, Vec};

/// Total basis points representing 100% — ratio vecs must sum to exactly this value.
pub const BASIS_POINTS_TOTAL: u32 = 10_000;

/// (base, quote) asset pair for oracle-priced invoices.
#[contracttype]
#[derive(Clone, Debug)]
pub struct AssetPair {
    pub base: Symbol,
    pub quote: Symbol,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum OverflowBehavior {
    Reject,
    Refund,
    Donate,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct CloneOverrides {
    pub new_deadline: Option<u64>,
    pub new_amounts: Option<Vec<i128>>,
    pub new_recipients: Option<Vec<Address>>,
    pub new_overflow_behavior: Option<Symbol>,
}

/// Issue: Split rule for a single recipient — evaluated at release time.
#[contracttype]
#[derive(Clone, Debug)]
pub enum SplitRule {
    /// Pay this exact amount regardless of funded total.
    Fixed(i128),
    /// Pay `funded * bps / 10_000` to the recipient.
    Percentage(u32),
    /// Pay `funded * bps / 10_000` only when `funded > threshold`; else 0.
    /// Encoded as (threshold, bps).
    Tiered(i128, u32),
}

/// Issue: Action taken by an auto-resolve rule.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ResolveAction {
    Release,
    Refund,
}

/// Issue: Auto-resolve rule — if funded/total >= min_funded_bps/10_000, execute action.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ResolveRule {
    /// Minimum funding threshold in basis points (e.g. 5000 = 50%).
    pub min_funded_bps: u32,
    pub action: ResolveAction,
}

/// Issue #285: Volume-based fee tier for creators.
#[contracttype]
#[derive(Clone, Debug)]
pub struct FeeTier {
    /// Minimum creator lifetime volume threshold to qualify for this tier.
    pub volume_threshold: u64,
    /// Fee in basis points (e.g. 100 = 1%).
    pub fee_bps: u32,
}

/// Issue #409: Rebate tier for high-volume creators.
#[contracttype]
#[derive(Clone, Debug)]
pub struct RebateTier {
    pub min_volume: i128,
    pub rebate_bps: u32,
}

/// Issue #299: Per-creator analytics aggregator.
#[contracttype]
#[derive(Clone, Debug)]
pub struct CreatorStats {
    /// Total number of invoices created.
    pub total_invoices: u32,
    /// Total amount raised across all invoices.
    pub total_raised: u64,
    /// Total amount released to recipients.
    pub total_released: u64,
    /// Total number of unique payers.
    pub total_payers: u32,
    /// Average funding time in ledgers (running average).
    pub avg_funding_time_ledgers: u32,
}

/// Issue #: A single (invoice_id, amount) pair for pool_pay.
#[contracttype]
#[derive(Clone, Debug)]
pub struct InvoicePayment {
    pub invoice_id: u64,
    pub amount: i128,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Bid {
    pub bidder: Address,
    pub amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum InvoiceStatus {
    Pending,
    Released,
    Refunded,
    Expired,
    Cancelled,
}

/// Issue #449: Multi-phase invoice state machine.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum InvoicePhase {
    Draft,
    Active,
    Locked,
    Released,
}

/// Issue #447: Per-invoice analytics accumulator.
#[contracttype]
#[derive(Clone, Debug)]
pub struct InvoiceAnalytics {
    pub payment_count: u64,
    pub total_funded: i128,
    pub unique_payers: u32,
    pub first_payment_ledger: u32,
    pub last_payment_ledger: u32,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum AdminRole {
    SuperAdmin,
    Operator,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Payment {
    pub payer: Address,
    pub amount: i128,
    pub tip: i128,
    pub attestation_hash: Option<BytesN<32>>,
    pub donate_on_failure: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct AuditEntry {
    pub action: Symbol,
    pub actor: Address,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct SubscriptionParams {
    pub creator: Address,
    pub recipients: Vec<Address>,
    pub amounts: Vec<i128>,
    pub tokens: Vec<Address>,
    /// Optional recurrence interval in days. Defaults to 30 if None.
    pub interval_days: Option<u32>,
}

/// Issue #414: Per-recipient payout configuration.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Recipient {
    pub address: Address,
    pub token: Address,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct CompletionProof {
    pub id: u64,
    pub status: InvoiceStatus,
    pub funded: i128,
    pub timestamp: u64,
    pub hash: BytesN<32>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PaymentProof {
    pub invoice_id: u64,
    pub payer: Address,
    pub total_paid: i128,
    pub proof_hash: BytesN<32>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct InvoiceTemplate {
    pub recipients: Vec<Address>,
    pub amounts: Vec<i128>,
    pub token: Address,
    /// Unix timestamp after which unfunded invoices can be refunded.
    pub deadline: u64,
    /// Total amount collected so far.
    pub funded: i128,
    /// Current lifecycle status.
    pub status: InvoiceStatus,
    /// All payments made toward this invoice.
    pub payments: Vec<Payment>,
    /// Optional whitelist of addresses allowed to pay this invoice.
    /// When None, any address may pay.
    pub allowed_payers: Option<Vec<Address>>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct CreateInvoiceParams {
    pub recipients: Vec<Address>,
    pub amounts: Vec<i128>,
    pub token: Address,
    pub deadline: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PaymentCommitment {
    pub commitment_hash: BytesN<32>,
    pub commit_ledger: u32,
}

/// A single graduated release tranche: `basis_points` out of 10 000 of the
/// invoice total becomes releasable once the ledger time reaches `timestamp`.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Tranche {
    pub timestamp: u64,
    pub basis_points: u32,
}

/// On-chain reputation scoring metrics for an address (issue #349).
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct RepScore {
    pub paid_on_time: u32,
    pub late_pays: u32,
    pub invoices_released: u32,
    pub invoices_refunded: u32,
}

/// Issue #431: Payment fingerprint for duplicate detection.
#[contracttype]
#[derive(Clone, Debug)]
pub struct PaymentFingerprint {
    /// Timestamp (ledger sequence) when the payment was recorded.
    pub recorded_at_ledger: u32,
    /// Hash of (invoice_id || payer || amount || ledger_sequence).
    pub fingerprint_hash: BytesN<32>,
}

/// Optional parameters for `create_invoice`, grouped to keep the function
/// within Soroban's 10-parameter limit.
#[contracttype]
#[derive(Clone, Debug)]
pub struct InvoiceOptions {
    pub co_creators: Vec<Address>,
    pub allow_early_withdrawal: bool,
    pub bonus_pool: i128,
    pub bonus_max_payers: u32,
    /// Optional creator cosigner address that must co-author creator actions.
    pub creator_cosigner: Option<Address>,
    /// Velocity limit in token units for a single payer over `velocity_window`.
    pub velocity_limit: i128,
    /// Window length in seconds for velocity limiting.
    pub velocity_window: u64,
    /// Issue #22: block release until this invoice is Released.
    pub prerequisite_id: Option<u64>,
    /// Issue #23: graduated release schedule; empty = release all at once.
    pub tranches: Vec<Tranche>,
    /// Co-signers whose approval is required before release.
    pub co_signers: Vec<Address>,
    /// How many co-signer approvals are needed (≤ `co_signers.len()`).
    pub required_signatures: u32,
    /// Penalty basis points for late payments (issue #42).
    pub penalty_bps: Option<u32>,
    /// Soft deadline timestamp; payments after this incur a penalty (issue #42).
    pub penalty_deadline: Option<u64>,
    /// Minimum funding threshold in basis points (issue #43).
    pub min_funding_bps: Option<u32>,
    /// Issue #86: creator-triggered staged release schedule; each entry is
    /// basis points (must sum to 10 000 when non-empty).
    pub release_stages: Vec<u32>,
    /// Issue #142: optional price oracle contract for dynamic pricing.
    pub price_oracle: Option<Address>,
    /// Issue #41: optional preferred output token per recipient for DEX swap on release.
    pub swap_tokens: Vec<Option<Address>>,
    pub tax_bps: Option<u32>,
    pub tax_authority: Option<Address>,
    pub insurance_premium_bps: Option<u32>,
    pub smart_route: Option<bool>,
    pub notification_contract: Option<Address>,
    pub overflow_behavior: OverflowBehavior,
    /// Issue #1: when true, _release() registers funds with the stream contract instead of direct transfer.
    pub convert_to_stream: bool,
    /// Issue #2: tokens accepted in pay_with_token(); base token is always accepted implicitly.
    pub accepted_tokens: Vec<Address>,
    /// Optional automatic forwarding address target for leftover funds.
    pub forward_to: Option<Address>,
    /// Optional automatic forwarding to another invoice id.
    pub forward_invoice_id: Option<u64>,
    /// Issue: per-recipient split rules evaluated at release time; empty = use amounts[].
    pub split_rules: Vec<SplitRule>,
    /// Issue: pre-agreed auto-resolution rules evaluated in order when auto_resolve() is called.
    pub auto_resolve_rules: Vec<ResolveRule>,
    /// Optional oracle address that must confirm the condition before release.
    pub oracle_address: Option<Address>,
    /// Optional cross-chain reference carried through invoice creation.
    pub cross_chain_ref: Option<String>,
    /// Issue #98: restrict payments to this allowlist; None = open.
    pub allowed_payers: Option<Vec<Address>>,
    /// Issue: per-recipient release priorities (parallel to recipients); empty = no ordering.
    pub priorities: Vec<u32>,
    /// Issue #199: grace period in seconds after deadline before refund is allowed.
    pub refund_grace_secs: Option<u64>,
    /// Scheduled release timestamp (issue #207).
    pub scheduled_release_at: Option<u64>,
    /// KYC verification requirement.
    pub require_kyc: bool,
    /// Per-recipient split ratios in basis points (must sum to [`BASIS_POINTS_TOTAL`] = 10 000
    /// when non-empty).  Empty vec means "no ratio constraint — use amounts directly."
    pub ratios: Vec<u32>,
    /// Overflow fields that would otherwise push this struct past Soroban's
    /// 40-field `#[contracttype]` limit — see [`InvoiceOptions2`].
    pub ext: InvoiceOptions2,
}

/// Overflow options for `create_invoice`, split off from [`InvoiceOptions`] to stay within
/// Soroban's 40-field `#[contracttype]` limit.
#[contracttype]
#[derive(Clone, Debug)]
pub struct InvoiceOptions2 {
    /// Issue #274: invoice target in USD cents; used with price_oracle for dynamic funding.
    pub target_usd_cents: Option<u64>,
    /// Issue #307: explicit payment token override; uses this token instead of the invoice base token.
    pub payment_token: Option<Address>,
    /// Issue #327: ledgers to lock funds after full funding (max 100_000 ≈ 5 days).
    pub release_delay_ledgers: Option<u32>,
    /// Issue #329: optional IPFS CID / SHA-256 hash of off-chain invoice metadata.
    pub metadata_hash: Option<BytesN<32>>,
    /// Per-payer cooldown window in seconds (issue #168).
    pub payment_cooldown_secs: Option<u64>,
    /// Maximum payments allowed per window (issue #168).
    pub max_payments_per_window: Option<u32>,
    /// Window duration in seconds for payment rate limiting (issue #168).
    pub payment_window_secs: Option<u64>,
    /// Oracle contract used for oracle-priced invoices: the funding target is
    /// computed at payment time from a live exchange rate instead of being
    /// fixed at creation. When set, `oracle_asset_pair` must also be set and
    /// `amounts` is interpreted as the USD-cents funding target.
    pub oracle: Option<Address>,
    /// Base asset symbol passed to the oracle's `price` call (e.g. XLM).
    pub oracle_asset_pair_base: Option<Symbol>,
    /// Quote asset symbol passed to the oracle's `price` call (e.g. USD).
    pub oracle_asset_pair_quote: Option<Symbol>,
    /// Minimum required payer reputation score to pay this invoice (issue #349).
    pub min_payer_rep: Option<u32>,
    /// Issue #430: payments are rejected before this timestamp, if set.
    pub payment_open_at: Option<u64>,
    /// Issue #430: payments are rejected after this timestamp, if set.
    /// Must be strictly before `deadline` when set.
    pub payment_close_at: Option<u64>,
    /// Optional milestone thresholds in basis points for auto-release gates.
    pub milestones: Option<Vec<u32>>,
    /// Optional per-recipient payout caps parallel to `recipients`.
    pub recipient_max_payouts: Option<Vec<Option<i128>>>,
    /// Issue #416: SHA-256 hash of the required off-chain release preimage.
    pub release_condition_hash: Option<BytesN<32>>,
    /// Issue #417: enable recipient whitelist enforcement for this invoice.
    pub recipient_whitelist_enabled: bool,
    /// Issue #188: escrow hold period in ledgers.
    pub escrow_hold_period: Option<u32>,
}

/// Legacy invoice layout used by stored invoices created before the `version`
/// field was added. Kept for on-chain migration so old data can be
/// deserialised and re-saved in the current schema.
#[contracttype]
#[derive(Clone, Debug)]
pub struct LegacyInvoice {
    pub creator: Address,
    pub co_creators: Vec<Address>,
    pub recipients: Vec<Address>,
    pub amounts: Vec<i128>,
    pub tokens: Vec<Address>,
    pub deadline: u64,
    pub funded: i128,
    pub status: InvoiceStatus,
    pub payments: Vec<Payment>,
    pub drip_duration: Option<u64>,
    pub release_timestamp: Option<u64>,
    pub claimed: Vec<i128>,
    pub frozen: bool,
    pub completion_time: Option<u64>,
    pub allow_early_withdrawal: bool,
    pub bonus_pool: i128,
    pub bonus_max_payers: u32,
    pub prerequisite_id: Option<u64>,
    pub tranches: Vec<Tranche>,
    pub released_bps: u32,
    pub stake_amount: i128,
    pub referrer: Option<Address>,
    pub tax_bps: u32,
    pub tax_authority: Option<Address>,
    pub insurance_premium_bps: u32,
    pub insurance_fund: i128,
    pub smart_route: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct InvoiceCore {
    pub version: u32,
    pub creator: Address,
    pub co_creators: Vec<Address>,
    pub recipients: Vec<Address>,
    pub amounts: Vec<i128>,
    pub tokens: Vec<Address>,
    pub funding_token: Address,
    pub deadline: u64,
    pub funded: i128,
    pub status: InvoiceStatus,
    pub payments: Vec<Payment>,
    pub drip_duration: Option<u64>,
    pub release_timestamp: Option<u64>,
    pub claimed: Vec<i128>,
    pub frozen: bool,
    pub completion_time: Option<u64>,
    pub allow_early_withdrawal: bool,
    pub bonus_pool: i128,
    pub bonus_max_payers: u32,
    pub prerequisite_id: Option<u64>,
    pub tranches: Vec<Tranche>,
    pub released_bps: u32,
    pub clone_depth: u32,
    pub predecessor_id: Option<u64>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct InvoiceExt {
    pub co_signers: Vec<Address>,
    pub required_signatures: u32,
    pub signatures: Vec<Address>,
    pub approver: Option<Address>,
    pub approved: bool,
    pub oracle_address: Option<Address>,
    pub condition_met: bool,
    pub penalty_bps: u32,
    pub penalty_deadline: u64,
    pub min_funding_bps: u32,
    pub release_stages: Vec<u32>,
    pub released_stages: u32,
    pub allowed_payers: Option<Vec<Address>>,
    pub price_oracle: Option<Address>,
    pub base_amounts: Vec<i128>,
    pub swap_tokens: Vec<Option<Address>>,
    pub tax_bps: u32,
    pub tax_authority: Option<Address>,
    pub insurance_premium_bps: u32,
    pub insurance_fund: i128,
    pub smart_route: bool,
    pub convert_to_stream: bool,
    pub accepted_tokens: Vec<Address>,
    pub forward_to: Option<Address>,
    pub forward_invoice_id: Option<u64>,
    pub split_rules: Vec<SplitRule>,
    pub auto_resolve_rules: Vec<ResolveRule>,
    pub creator_cosigner: Option<Address>,
    pub velocity_limit: i128,
    pub velocity_window: u64,
    pub parent_invoice_id: Option<u64>,
    pub pause_reason: Option<String>,
    pub auto_resume_at: Option<u64>,
    pub payment_cooldown_secs: Option<u64>,
    pub max_payments_per_window: Option<u32>,
    pub payment_window_secs: Option<u64>,
    pub scheduled_release_at: Option<u64>,
    pub penalty_tiers: Vec<PenaltyTier>,
    pub allowed_callers: Option<Vec<Address>>,
    pub refund_grace_secs: Option<u64>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct InvoiceExt2 {
    pub notification_contract: Option<Address>,
    pub overflow_behavior: OverflowBehavior,
    pub cross_chain_ref: Option<String>,
    pub require_kyc: bool,
    /// Issue #188: arbiter address that can raise and resolve disputes.
    pub arbiter: Option<Address>,
    /// Issue #188: whether this invoice is under active dispute.
    pub disputed: bool,
    pub admin_frozen: bool,
    pub auction_on_expiry: bool,
    pub auction_end: u64,
    pub bids: Vec<Bid>,
    pub min_payment: i128,
    pub min_funding_amount: i128,
    pub priorities: Vec<u32>,
    /// Issue #274: invoice target in USD cents for oracle-based dynamic funding.
    pub target_usd_cents: Option<u64>,
    /// Issue #308: addresses that have already claimed a per-payer refund on this invoice.
    pub refunded_addresses: Vec<Address>,
    /// Oracle-priced invoices: oracle contract queried at payment time.
    pub oracle: Option<Address>,
    /// Oracle-priced invoices: base asset symbol passed to the oracle.
    pub oracle_asset_pair_base: Option<Symbol>,
    /// Oracle-priced invoices: quote asset symbol passed to the oracle.
    pub oracle_asset_pair_quote: Option<Symbol>,
    /// Issue #349: minimum required payer reputation score.
    pub min_payer_rep: Option<u32>,
    pub escrow_hold_period: Option<u32>,
    pub held_until: Option<u32>,
    /// Funding milestone thresholds in basis points.
    pub milestones: Vec<u32>,
    /// Number of milestones already released.
    pub milestones_released: u32,
    /// Optional per-recipient payout caps parallel to `recipients`.
    pub recipient_max_payouts: Vec<Option<i128>>,
    /// Time-weighted average funding rate accumulator numerator.
    pub twafr_numerator: i128,
    /// Last ledger sequence used to update TWAFR.
    pub twafr_last_ledger: u32,
    /// Issue #416: SHA-256 hash required to release the invoice.
    pub release_condition_hash: Option<BytesN<32>>,
    /// Issue #417: recipient whitelist enforcement flag.
    pub recipient_whitelist_enabled: bool,
}

/// Issue #211: A single escalating penalty tier (seconds_after_deadline, bps).
#[contracttype]
#[derive(Clone, Debug)]
pub struct PenaltyTier {
    pub seconds_after_deadline: u64,
    pub bps: u32,
}

/// Timelocked admin action queued for future execution.
#[contracttype]
#[derive(Clone, Debug)]
pub enum TimelockAction {
    SetTreasury(Address),
    SetPlatformFee(u32),
}

/// A queued timelock action with metadata.
#[contracttype]
#[derive(Clone, Debug)]
pub struct QueuedAction {
    pub action: TimelockAction,
    pub queued_at: u64,
    pub executed: bool,
}

/// Full invoice — assembled from InvoiceCore + InvoiceExt + InvoiceExt2.
/// Never stored directly; use save_invoice / load_invoice helpers in lib.rs.
#[derive(Clone, Debug)]
pub struct Invoice {
    pub version: u32,
    pub creator: Address,
    pub co_creators: Vec<Address>,
    pub recipients: Vec<Address>,
    pub amounts: Vec<i128>,
    pub tokens: Vec<Address>,
    pub funding_token: Address,
    pub deadline: u64,
    pub funded: i128,
    pub status: InvoiceStatus,
    pub payments: Vec<Payment>,
    pub drip_duration: Option<u64>,
    pub release_timestamp: Option<u64>,
    pub claimed: Vec<i128>,
    pub frozen: bool,
    pub completion_time: Option<u64>,
    pub allow_early_withdrawal: bool,
    pub bonus_pool: i128,
    pub bonus_max_payers: u32,
    pub prerequisite_id: Option<u64>,
    pub tranches: Vec<Tranche>,
    pub released_bps: u32,
    pub co_signers: Vec<Address>,
    pub required_signatures: u32,
    pub signatures: Vec<Address>,
    pub approver: Option<Address>,
    pub approved: bool,
    pub oracle_address: Option<Address>,
    pub condition_met: bool,
    pub penalty_bps: u32,
    pub penalty_deadline: u64,
    pub min_funding_bps: u32,
    pub release_stages: Vec<u32>,
    pub released_stages: u32,
    pub allowed_payers: Option<Vec<Address>>,
    pub price_oracle: Option<Address>,
    pub base_amounts: Vec<i128>,
    pub swap_tokens: Vec<Option<Address>>,
    pub tax_bps: u32,
    pub tax_authority: Option<Address>,
    pub insurance_premium_bps: u32,
    pub insurance_fund: i128,
    pub smart_route: bool,
    pub convert_to_stream: bool,
    pub accepted_tokens: Vec<Address>,
    pub forward_to: Option<Address>,
    pub forward_invoice_id: Option<u64>,
    pub split_rules: Vec<SplitRule>,
    pub auto_resolve_rules: Vec<ResolveRule>,
    pub creator_cosigner: Option<Address>,
    pub velocity_limit: i128,
    pub velocity_window: u64,
    pub parent_invoice_id: Option<u64>,
    pub pause_reason: Option<String>,
    pub auto_resume_at: Option<u64>,
    pub payment_cooldown_secs: Option<u64>,
    pub max_payments_per_window: Option<u32>,
    pub payment_window_secs: Option<u64>,
    /// Scheduled release timestamp (issue #207).
    pub scheduled_release_at: Option<u64>,
    /// Issue #199: grace period in seconds after deadline before refund is allowed.
    pub refund_grace_secs: Option<u64>,
    /// Issue #211: escalating penalty tiers.
    pub penalty_tiers: Vec<PenaltyTier>,
    /// Issue #208: restrict payments to specific calling contracts; None = open.
    pub allowed_callers: Option<Vec<Address>>,
    pub notification_contract: Option<Address>,
    pub overflow_behavior: OverflowBehavior,
    pub cross_chain_ref: Option<String>,
    pub require_kyc: bool,
    pub arbiter: Option<Address>,
    pub disputed: bool,
    pub admin_frozen: bool,
    pub auction_on_expiry: bool,
    pub auction_end: u64,
    pub bids: Vec<Bid>,
    pub min_payment: i128,
    pub min_funding_amount: i128,
    pub priorities: Vec<u32>,
    pub clone_depth: u32,
    /// Issue #274: invoice target in USD cents for oracle-based dynamic funding.
    pub target_usd_cents: Option<u64>,
    /// Issue #308: addresses that have already claimed a per-payer refund on this invoice.
    pub refunded_addresses: Vec<Address>,
    /// Oracle-priced invoices: oracle contract queried at payment time.
    pub oracle: Option<Address>,
    /// Oracle-priced invoices: base asset symbol passed to the oracle.
    pub oracle_asset_pair_base: Option<Symbol>,
    /// Oracle-priced invoices: quote asset symbol passed to the oracle.
    pub oracle_asset_pair_quote: Option<Symbol>,
    /// Issue #349: minimum required payer reputation score.
    pub min_payer_rep: Option<u32>,
    pub escrow_hold_period: Option<u32>,
    pub held_until: Option<u32>,
    pub milestones: Vec<u32>,
    pub milestones_released: u32,
    pub recipient_max_payouts: Vec<Option<i128>>,
    pub twafr_numerator: i128,
    pub twafr_last_ledger: u32,
    /// Issue #416: SHA-256 hash required to release the invoice.
    pub release_condition_hash: Option<BytesN<32>>,
    /// Issue #417: recipient whitelist enforcement flag.
    pub recipient_whitelist_enabled: bool,
    pub predecessor_id: Option<u64>,
}

impl Invoice {
    pub fn split(self) -> (InvoiceCore, InvoiceExt, InvoiceExt2) {
        (
            InvoiceCore {
                version: self.version,
                creator: self.creator,
                co_creators: self.co_creators,
                recipients: self.recipients,
                amounts: self.amounts,
                tokens: self.tokens,
                funding_token: self.funding_token,
                deadline: self.deadline,
                funded: self.funded,
                status: self.status,
                payments: self.payments,
                drip_duration: self.drip_duration,
                release_timestamp: self.release_timestamp,
                claimed: self.claimed,
                frozen: self.frozen,
                completion_time: self.completion_time,
                allow_early_withdrawal: self.allow_early_withdrawal,
                bonus_pool: self.bonus_pool,
                bonus_max_payers: self.bonus_max_payers,
                prerequisite_id: self.prerequisite_id,
                tranches: self.tranches,
                released_bps: self.released_bps,
                clone_depth: self.clone_depth,
                predecessor_id: self.predecessor_id,
            },
            InvoiceExt {
                co_signers: self.co_signers,
                required_signatures: self.required_signatures,
                signatures: self.signatures,
                approver: self.approver,
                approved: self.approved,
                oracle_address: self.oracle_address,
                condition_met: self.condition_met,
                penalty_bps: self.penalty_bps,
                penalty_deadline: self.penalty_deadline,
                min_funding_bps: self.min_funding_bps,
                release_stages: self.release_stages,
                released_stages: self.released_stages,
                allowed_payers: self.allowed_payers,
                price_oracle: self.price_oracle,
                base_amounts: self.base_amounts,
                swap_tokens: self.swap_tokens,
                tax_bps: self.tax_bps,
                tax_authority: self.tax_authority,
                insurance_premium_bps: self.insurance_premium_bps,
                insurance_fund: self.insurance_fund,
                smart_route: self.smart_route,
                convert_to_stream: self.convert_to_stream,
                accepted_tokens: self.accepted_tokens,
                forward_to: self.forward_to,
                forward_invoice_id: self.forward_invoice_id,
                split_rules: self.split_rules,
                auto_resolve_rules: self.auto_resolve_rules,
                creator_cosigner: self.creator_cosigner,
                velocity_limit: self.velocity_limit,
                velocity_window: self.velocity_window,
                parent_invoice_id: self.parent_invoice_id,
                pause_reason: self.pause_reason,
                auto_resume_at: self.auto_resume_at,
                payment_cooldown_secs: self.payment_cooldown_secs,
                max_payments_per_window: self.max_payments_per_window,
                payment_window_secs: self.payment_window_secs,
                scheduled_release_at: self.scheduled_release_at,
                penalty_tiers: self.penalty_tiers,
                allowed_callers: self.allowed_callers,
                refund_grace_secs: self.refund_grace_secs,
            },
            InvoiceExt2 {
                notification_contract: self.notification_contract,
                overflow_behavior: self.overflow_behavior,
                cross_chain_ref: self.cross_chain_ref,
                require_kyc: self.require_kyc,
                arbiter: self.arbiter,
                disputed: self.disputed,
                admin_frozen: self.admin_frozen,
                auction_on_expiry: self.auction_on_expiry,
                auction_end: self.auction_end,
                bids: self.bids,
                min_payment: self.min_payment,
                min_funding_amount: self.min_funding_amount,
                priorities: self.priorities,
                target_usd_cents: self.target_usd_cents,
                refunded_addresses: self.refunded_addresses,
                oracle: self.oracle,
                oracle_asset_pair_base: self.oracle_asset_pair_base,
                oracle_asset_pair_quote: self.oracle_asset_pair_quote,
                min_payer_rep: self.min_payer_rep,
                escrow_hold_period: self.escrow_hold_period,
                held_until: self.held_until,
                milestones: self.milestones,
                milestones_released: self.milestones_released,
                recipient_max_payouts: self.recipient_max_payouts,
                twafr_numerator: self.twafr_numerator,
                twafr_last_ledger: self.twafr_last_ledger,
                release_condition_hash: self.release_condition_hash,
                recipient_whitelist_enabled: self.recipient_whitelist_enabled,
            },
        )
    }

    pub fn assemble(core: InvoiceCore, ext: InvoiceExt, ext2: InvoiceExt2) -> Self {
        Invoice {
            version: core.version,
            creator: core.creator,
            co_creators: core.co_creators,
            recipients: core.recipients,
            amounts: core.amounts,
            tokens: core.tokens,
            funding_token: core.funding_token,
            deadline: core.deadline,
            funded: core.funded,
            status: core.status,
            payments: core.payments,
            drip_duration: core.drip_duration,
            release_timestamp: core.release_timestamp,
            claimed: core.claimed,
            frozen: core.frozen,
            completion_time: core.completion_time,
            allow_early_withdrawal: core.allow_early_withdrawal,
            bonus_pool: core.bonus_pool,
            bonus_max_payers: core.bonus_max_payers,
            prerequisite_id: core.prerequisite_id,
            tranches: core.tranches,
            released_bps: core.released_bps,
            clone_depth: core.clone_depth,
            predecessor_id: core.predecessor_id,
            co_signers: ext.co_signers,
            required_signatures: ext.required_signatures,
            signatures: ext.signatures,
            approver: ext.approver,
            approved: ext.approved,
            oracle_address: ext.oracle_address,
            condition_met: ext.condition_met,
            penalty_bps: ext.penalty_bps,
            penalty_deadline: ext.penalty_deadline,
            min_funding_bps: ext.min_funding_bps,
            release_stages: ext.release_stages,
            released_stages: ext.released_stages,
            allowed_payers: ext.allowed_payers,
            price_oracle: ext.price_oracle,
            base_amounts: ext.base_amounts,
            swap_tokens: ext.swap_tokens,
            tax_bps: ext.tax_bps,
            tax_authority: ext.tax_authority,
            insurance_premium_bps: ext.insurance_premium_bps,
            insurance_fund: ext.insurance_fund,
            smart_route: ext.smart_route,
            convert_to_stream: ext.convert_to_stream,
            accepted_tokens: ext.accepted_tokens,
            forward_to: ext.forward_to,
            forward_invoice_id: ext.forward_invoice_id,
            split_rules: ext.split_rules,
            auto_resolve_rules: ext.auto_resolve_rules,
            creator_cosigner: ext.creator_cosigner,
            velocity_limit: ext.velocity_limit,
            velocity_window: ext.velocity_window,
            parent_invoice_id: ext.parent_invoice_id,
            pause_reason: ext.pause_reason,
            auto_resume_at: ext.auto_resume_at,
            payment_cooldown_secs: ext.payment_cooldown_secs,
            max_payments_per_window: ext.max_payments_per_window,
            payment_window_secs: ext.payment_window_secs,
            scheduled_release_at: ext.scheduled_release_at,
            penalty_tiers: ext.penalty_tiers,
            allowed_callers: ext.allowed_callers,
            refund_grace_secs: ext.refund_grace_secs,
            notification_contract: ext2.notification_contract,
            overflow_behavior: ext2.overflow_behavior,
            cross_chain_ref: ext2.cross_chain_ref,
            require_kyc: ext2.require_kyc,
            arbiter: ext2.arbiter,
            disputed: ext2.disputed,
            admin_frozen: ext2.admin_frozen,
            auction_on_expiry: ext2.auction_on_expiry,
            auction_end: ext2.auction_end,
            bids: ext2.bids,
            min_payment: ext2.min_payment,
            min_funding_amount: ext2.min_funding_amount,
            priorities: ext2.priorities,
            target_usd_cents: ext2.target_usd_cents,
            refunded_addresses: ext2.refunded_addresses,
            oracle: ext2.oracle,
            oracle_asset_pair_base: ext2.oracle_asset_pair_base,
            oracle_asset_pair_quote: ext2.oracle_asset_pair_quote,
            min_payer_rep: ext2.min_payer_rep,
            escrow_hold_period: ext2.escrow_hold_period,
            held_until: ext2.held_until,
            milestones: ext2.milestones,
            milestones_released: ext2.milestones_released,
            recipient_max_payouts: ext2.recipient_max_payouts,
            twafr_numerator: ext2.twafr_numerator,
            twafr_last_ledger: ext2.twafr_last_ledger,
            release_condition_hash: ext2.release_condition_hash,
            recipient_whitelist_enabled: ext2.recipient_whitelist_enabled,
        }
    }
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PaymentCertificate {
    /// The invoice this certificate covers.
    pub invoice_id: u64,
    /// Total amount paid out to all recipients.
    pub total: i128,
    /// All recipient addresses that received funds.
    pub recipients: Vec<Address>,
    /// Ledger timestamp at which the invoice was released.
    pub release_timestamp: u64,
    /// SHA-256 hash over (invoice_id || total || release_timestamp), deterministic for the same data.
    pub cert_hash: BytesN<32>,
}

/// Issue #144: Payment analytics for an invoice, callable by external contracts.
#[contracttype]
#[derive(Clone, Debug)]
pub struct TreasuryRecord {
    pub invoice_ids: Vec<u64>,
    pub treasury: Address,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum GroupMode {
    AllOrNothing,
    Majority,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct InvoiceGroup {
    pub invoice_ids: Vec<u64>,
    pub mode: GroupMode,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct InvoiceStats {
    pub funded: i128,
    pub total: i128,
    pub payment_count: u32,
    pub unique_payers: u32,
    pub completion_bps: u32,
}

/// Compact storage representation of Invoice — serializes InvoiceCore fields using minimal byte encoding.
#[contracttype]
#[derive(Clone, Debug)]
pub struct InvoiceSnapshot {
    pub core: InvoiceCore,
    pub ext: InvoiceExt,
    pub ext2: InvoiceExt2,
    pub audit_log: Vec<AuditEntry>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct CompactInvoice {
    /// Serialized bytes: [status(1), funded(16), deadline(8), ...rest]
    pub data: Bytes,
}

impl Invoice {
    /// Convert Invoice to compact byte representation.
    pub fn to_compact(&self, env: &Env) -> CompactInvoice {
        let mut bytes = Bytes::new(env);

        // Pack status as 1 byte
        let status_byte: u8 = match self.status {
            InvoiceStatus::Pending => 0,
            InvoiceStatus::Released => 1,
            InvoiceStatus::Refunded => 2,
            InvoiceStatus::Cancelled => 3,
            InvoiceStatus::Expired => 4,
        };
        bytes.push_back(status_byte);

        // Pack funded as 16 bytes (i128)
        let funded_bytes = self.funded.to_be_bytes();
        for byte in funded_bytes.iter() {
            bytes.push_back(*byte);
        }

        // Pack deadline as 8 bytes (u64)
        let deadline_bytes = self.deadline.to_be_bytes();
        for byte in deadline_bytes.iter() {
            bytes.push_back(*byte);
        }

        CompactInvoice { data: bytes }
    }

    /// Restore Invoice from compact byte representation.
    pub fn from_compact(
        compact: &CompactInvoice,
        core: InvoiceCore,
        ext: InvoiceExt,
        ext2: InvoiceExt2,
    ) -> Self {
        let bytes = &compact.data;

        // Unpack status (1 byte)
        let status_byte = bytes.get(0).unwrap();
        let status = match status_byte {
            0 => InvoiceStatus::Pending,
            1 => InvoiceStatus::Released,
            2 => InvoiceStatus::Refunded,
            3 => InvoiceStatus::Cancelled,
            4 => InvoiceStatus::Expired,
            _ => InvoiceStatus::Pending,
        };

        // Unpack funded (16 bytes)
        let mut funded_bytes = [0u8; 16];
        for (i, byte) in funded_bytes.iter_mut().enumerate() {
            *byte = bytes.get((1 + i) as u32).unwrap();
        }
        let funded = i128::from_be_bytes(funded_bytes);

        // Unpack deadline (8 bytes)
        let mut deadline_bytes = [0u8; 8];
        for (i, byte) in deadline_bytes.iter_mut().enumerate() {
            *byte = bytes.get((17 + i) as u32).unwrap();
        }
        let deadline = u64::from_be_bytes(deadline_bytes);

        // Reconstruct full invoice with updated fields
        let mut invoice = Invoice::assemble(core, ext, ext2);
        invoice.status = status;
        invoice.funded = funded;
        invoice.deadline = deadline;
        invoice
    }

    /// Upgrade a legacy (pre-version) invoice to the current schema.
    /// New fields are filled with their default (empty / zero) values.
    pub fn from_legacy(old: LegacyInvoice, env: &Env) -> Self {
        let funding_token = old
            .tokens
            .get(0)
            .expect("no token")
            .clone();
        Invoice {
            version: 2,
            creator: old.creator,
            co_creators: old.co_creators,
            recipients: old.recipients,
            base_amounts: old.amounts.clone(),
            amounts: old.amounts,
            tokens: old.tokens,
            funding_token,
            deadline: old.deadline,
            funded: old.funded,
            status: old.status,
            payments: old.payments,
            drip_duration: old.drip_duration,
            release_timestamp: old.release_timestamp,
            claimed: old.claimed,
            frozen: old.frozen,
            completion_time: old.completion_time,
            allow_early_withdrawal: old.allow_early_withdrawal,
            bonus_pool: old.bonus_pool,
            bonus_max_payers: old.bonus_max_payers,
            prerequisite_id: old.prerequisite_id,
            tranches: old.tranches,
            released_bps: old.released_bps,
            co_signers: Vec::new(env),
            required_signatures: 0,
            signatures: Vec::new(env),
            approver: None,
            approved: false,
            oracle_address: None,
            condition_met: false,
            penalty_bps: 0,
            penalty_deadline: 0,
            min_funding_bps: 0,
            release_stages: Vec::new(env),
            released_stages: 0,
            allowed_payers: None,
            price_oracle: None,
            swap_tokens: Vec::new(env),
            tax_bps: 0,
            tax_authority: None,
            insurance_premium_bps: 0,
            insurance_fund: 0,
            smart_route: false,
            convert_to_stream: false,
            accepted_tokens: Vec::new(env),
            require_kyc: false,
            arbiter: None,
            disputed: false,
            admin_frozen: false,
            auction_on_expiry: false,
            auction_end: 0,
            bids: Vec::new(env),
            min_payment: 0,
            min_funding_amount: 0,
            split_rules: Vec::new(env),
            auto_resolve_rules: Vec::new(env),
            creator_cosigner: None,
            velocity_limit: 0,
            velocity_window: 0,
            pause_reason: None,
            auto_resume_at: None,
            payment_cooldown_secs: None,
            max_payments_per_window: None,
            payment_window_secs: None,
            scheduled_release_at: None,
            refund_grace_secs: None,
            penalty_tiers: Vec::<PenaltyTier>::new(env),
            allowed_callers: None,
            forward_to: None,
            forward_invoice_id: None,
            notification_contract: None,
            overflow_behavior: OverflowBehavior::Reject,
            cross_chain_ref: None,
            clone_depth: 0,
            parent_invoice_id: None,
            priorities: Vec::new(env),
            target_usd_cents: None,
            refunded_addresses: Vec::new(env),
            oracle: None,
            oracle_asset_pair_base: None,
            oracle_asset_pair_quote: None,
            min_payer_rep: None,
            escrow_hold_period: None,
            held_until: None,
            milestones: Vec::new(env),
            milestones_released: 0,
            recipient_max_payouts: Vec::new(env),
            twafr_numerator: 0,
            twafr_last_ledger: 0,
            release_condition_hash: None,
            recipient_whitelist_enabled: false,
            predecessor_id: None,
        }
    }
}

/// Issue #327 / #329 / #330: Extended invoice fields for new features.
/// Stored in separate persistent storage (key: inv_ex3 + invoice_id) so existing
/// InvoiceCore / InvoiceExt / InvoiceExt2 XDR layouts are not disturbed.
#[contracttype]
#[derive(Clone, Debug)]
pub struct InvoiceExt3 {
    /// Issue #327: creator-set ledger delay before funds can be released.
    pub release_delay_ledgers: Option<u32>,
    /// Issue #327: ledger sequence when the invoice became fully funded.
    pub funded_at_ledger: Option<u32>,
    /// Issue #327: computed unlock ledger (funded_at_ledger + release_delay_ledgers).
    /// None if no delay is set.
    pub unlock_at_ledger: Option<u32>,
    /// Issue #329: IPFS CID or SHA-256 hash of off-chain metadata.
    pub metadata_hash: Option<BytesN<32>>,
    /// Issue #330: recipients whose share has already been transferred.
    pub paid_recipients: Vec<Address>,
}

/// Issue #298: Result type returned by simulate_release().
#[contracttype]
#[derive(Clone, Debug)]
pub struct SimulateReleaseResult {
    pub estimated_instructions: u64,
    pub estimated_fee_stroops: u64,
    pub would_succeed: bool,
}

/// Issue #325: Status of a payer-raised dispute.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum DisputeStatus {
    Active,
    Resolved,
    Expired,
}

/// Issue #325: Outcome of a resolved dispute.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum DisputeOutcome {
    Approved,
    Refunded,
}

/// Issue #325: On-chain record of a payer-initiated dispute.
#[contracttype]
#[derive(Clone, Debug)]
pub struct DisputeRecord {
    pub reason_hash: BytesN<32>,
    pub raised_at: u32,
    pub status: DisputeStatus,
}

/// Issue #326: Protocol fee configuration set by admin.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ProtocolFeeConfig {
    pub rate_bps: u32,
    pub treasury: Address,
}

/// Issue #316 / #351: Compute budget estimate for a contract function.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComputeEstimate {
    pub cpu_insns: u64,
    pub mem_bytes: u64,
    pub fee_stroops: i128,
}

/// Issue #297: Circuit breaker status returned by get_circuit_breaker_status().
#[contracttype]
#[derive(Clone, Debug)]
pub struct CircuitBreakerStatus {
    pub active: bool,
    pub reason: Option<String>,
}

/// Issue #295: A single confidential payment record stored per payer.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ConfidentialPayment {
    pub commitment: BytesN<32>,
    pub encrypted_amount: Bytes,
}

/// Issue #310: Parameters for create_invoice v2 — groups all fields to stay within
/// Soroban's argument limit.
#[contracttype]
#[derive(Clone, Debug)]
pub struct InvoiceParams {
    pub creator: Address,
    pub recipients: Vec<Address>,
    // ... add all other fields here ...
}

/// Issue #310: Pending WASM upgrade proposal stored in instance storage.
#[contracttype]
#[derive(Clone, Debug)]
pub struct UpgradeProposal {
    pub new_wasm_hash: BytesN<32>,
    /// Ledger timestamp after which the upgrade may be executed.
    pub eligible_at: u64,
}

/// Hot invoice fields stored in instance storage for TTL-efficient reads.
///
/// These four fields are read on every `pay()` call. Keeping them in the
/// contract *instance* bucket means their TTL is extended by a single
/// `extend_ttl` call that covers all active invoices simultaneously —
/// O(1) per payment rather than one persistent-rent charge per invoice entry.
///
/// Cold creation params and audit metadata stay in persistent storage
/// (`InvoiceCore` / `InvoiceExt` / `InvoiceExt2`).
#[contracttype]
#[derive(Clone, Debug)]
pub struct InvoiceHot {
    /// Current lifecycle status — checked at the top of every `pay()`.
    pub status: InvoiceStatus,
    /// Cumulative funded amount — mutated on every payment.
    pub funded: i128,
    /// Sum of `amounts[]` cached at creation; avoids recomputing on each pay.
    pub total: i128,
    /// Recipient list — needed for penalty distribution and auto-release.
    pub recipients: Vec<Address>,
}

// ---------------------------------------------------------------------------
// Issue #334: Compact XDR storage helpers
// ---------------------------------------------------------------------------

impl InvoiceStatus {
    /// Encode as a single byte — saves XDR overhead vs. the full enum variant.
    pub fn to_u8(&self) -> u8 {
        match self {
            InvoiceStatus::Pending => 0,
            InvoiceStatus::Released => 1,
            InvoiceStatus::Refunded => 2,
            InvoiceStatus::Cancelled => 3,
            InvoiceStatus::Expired => 4,
        }
    }

    /// Decode from a single byte.  Unknown values map to Pending.
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => InvoiceStatus::Released,
            2 => InvoiceStatus::Refunded,
            3 => InvoiceStatus::Cancelled,
            4 => InvoiceStatus::Expired,
            _ => InvoiceStatus::Pending,
        }
    }
}

/// Issue #334: Result returned by `compact_migrate`.
#[contracttype]
#[derive(Clone, Debug)]
pub struct CompactMigrateResult {
    /// Invoice ID that was migrated.
    pub invoice_id: u64,
    /// Status byte written to compact storage (0–3).
    pub status_byte: u32,
    /// Whether the deadline was representable as a ledger sequence.
    pub deadline_migrated: bool,
}

/// Issue #332: Optimized release result.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ReleaseResult {
    /// Number of recipients paid in this call.
    pub recipients_paid: u32,
    /// Total amount transferred.
    pub total_transferred: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalmentTranche {
    pub amount: i128,
    pub ledger: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalmentPlan {
    pub tranches: Vec<InstalmentTranche>,
    pub paid_index: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeeBracket {
    pub max_amount: i128,
    pub rate_bps: u32,
}

/// Issue #437: Delayed payout stored per recipient until claimable.
#[contracttype]
#[derive(Clone, Debug)]
pub struct DelayedPayout {
    /// Amount to be transferred to recipient.
    pub amount: i128,
    /// Ledger sequence at which this payout becomes claimable.
    pub claimable_at_ledger: u32,
}
