use soroban_sdk::{contracttype, Address, BytesN, Env, Symbol, Vec, String};

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

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum OverflowBehavior {
    Reject,
    Refund,
    Donate,
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
    Cancelled,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Payment {
    pub payer: Address,
    pub amount: i128,
    pub tip: i128,
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

/// A single graduated release tranche: `basis_points` out of 10 000 of the
/// invoice total becomes releasable once the ledger time reaches `timestamp`.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Tranche {
    pub timestamp: u64,
    pub basis_points: u32,
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
    /// Maximum number of recipients allowed on this invoice; None = unlimited.
    pub max_recipients: Option<u32>,
}

/// Pending proposal to replace one recipient with another.
/// Stored per-invoice, keyed by the old recipient address.
#[contracttype]
#[derive(Clone, Debug)]
pub struct RecipientReplacement {
    /// The address that submitted the proposal.
    pub proposer: Address,
    /// The new recipient that will take the old recipient's slot.
    pub new_recipient: Address,
    /// Addresses (creator / co-creators) that have approved this proposal.
    pub approvals: Vec<Address>,
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
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct InvoiceExt2 {
    pub notification_contract: Option<Address>,
    pub overflow_behavior: OverflowBehavior,
    pub cross_chain_ref: Option<String>,
    pub require_kyc: bool,
    pub auction_on_expiry: bool,
    pub auction_end: u64,
    pub bids: Vec<Bid>,
    pub min_payment: i128,
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
    pub notification_contract: Option<Address>,
    pub overflow_behavior: OverflowBehavior,
    pub cross_chain_ref: Option<String>,
    pub require_kyc: bool,
    pub auction_on_expiry: bool,
    pub auction_end: u64,
    pub bids: Vec<Bid>,
    pub min_payment: i128,
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
            },
            InvoiceExt2 {
                notification_contract: self.notification_contract,
                overflow_behavior: self.overflow_behavior,
                cross_chain_ref: self.cross_chain_ref,
                require_kyc: self.require_kyc,
                auction_on_expiry: self.auction_on_expiry,
                auction_end: self.auction_end,
                bids: self.bids,
                min_payment: self.min_payment,
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
            notification_contract: ext2.notification_contract,
            overflow_behavior: ext2.overflow_behavior,
            cross_chain_ref: ext2.cross_chain_ref,
            require_kyc: ext2.require_kyc,
            auction_on_expiry: ext2.auction_on_expiry,
            auction_end: ext2.auction_end,
            bids: ext2.bids,
            min_payment: ext2.min_payment,
        }
    }
}

/// Issue #144: Payment analytics for an invoice, callable by external contracts.
#[contracttype]
#[derive(Clone, Debug)]
pub struct TreasuryRecord {
    pub invoice_ids: Vec<u64>,
    pub treasury: Address,
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

impl Invoice {
    /// Upgrade a legacy (pre-version) invoice to the current schema.
    /// New fields are filled with their default (empty / zero) values.
    pub fn from_legacy(old: LegacyInvoice, env: &Env) -> Self {
        Invoice {
            version: 2,
            creator: old.creator,
            co_creators: old.co_creators,
            recipients: old.recipients,
            base_amounts: old.amounts.clone(),
            amounts: old.amounts,
            tokens: old.tokens,
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
            auction_on_expiry: false,
            auction_end: 0,
            bids: Vec::new(env),
            min_payment: 0,
            split_rules: Vec::new(env),
            auto_resolve_rules: Vec::new(env),
            creator_cosigner: None,
            velocity_limit: 0,
            velocity_window: 0,
            forward_to: None,
            forward_invoice_id: None,
            notification_contract: None,
            overflow_behavior: OverflowBehavior::Reject,
            cross_chain_ref: None,
        }
    }
}
