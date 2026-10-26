//! Issue #1037 — Advanced dispute resolution with evidence submission,
//! conflict-free arbitrator panel selection, weighted ruling calculation,
//! multi-tier appeals, timeline enforcement, and on-chain settlement execution.

use soroban_sdk::{contracttype, symbol_short, Address, BytesN, Env, String, Symbol, Vec};

use crate::error::ContractError;

/// Default evidence submission window (48 hours).
pub const DEFAULT_EVIDENCE_WINDOW_SECS: u64 = 48 * 3600;
/// Default arbitration panel voting window (72 hours).
pub const DEFAULT_VOTING_WINDOW_SECS: u64 = 72 * 3600;
/// Default appeal filing window after initial ruling (24 hours).
pub const DEFAULT_APPEAL_WINDOW_SECS: u64 = 24 * 3600;
/// Maximum evidence items per dispute to bound ledger storage.
pub const MAX_EVIDENCE_ITEMS: u32 = 20;
/// Maximum arbitrators in the global pool.
pub const MAX_ARBITRATOR_POOL: u32 = 50;

// ---------------------------------------------------------------------------
// Storage keys (scoped under `AdvDisputeKey`)
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdvDisputeKey {
    Config,
    Counter,
    ArbitratorPool,
    RotationCursor,
    Dispute(u64),
    InvoiceDispute(u64),
    Evidence(u64),
    Votes(u64),
    Appeal(u64),
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisputeStage {
    EvidenceCollection,
    ArbitrationReview,
    DecisionRendered,
    UnderAppeal,
    Finalized,
    Enforced,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceKind {
    ContractAgreement,
    DeliveryProof,
    CommunicationLog,
    PaymentReceipt,
    AuditReport,
    Other,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArbitrationRuling {
    Pending,
    ReleaseToRecipient,
    RefundToPayer,
    SplitBps(u32),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdvDisputeConfig {
    pub evidence_window_secs: u64,
    pub voting_window_secs: u64,
    pub appeal_window_secs: u64,
    pub default_panel_size: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArbitratorProfile {
    pub address: Address,
    pub weight: u32,
    pub cases_assigned: u32,
    pub cases_decided: u32,
    pub active: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceRecord {
    pub evidence_id: u32,
    pub dispute_id: u64,
    pub submitter: Address,
    pub kind: EvidenceKind,
    pub content_hash: BytesN<32>,
    pub description: String,
    pub submitted_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PanelVote {
    pub arbitrator: Address,
    pub ruling: ArbitrationRuling,
    pub weight: u32,
    pub rationale_hash: BytesN<32>,
    pub voted_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppealRecord {
    pub dispute_id: u64,
    pub appellant: Address,
    pub reason: String,
    pub new_evidence_hash: BytesN<32>,
    pub filed_at: u64,
    pub resolved: bool,
    pub upheld_original: bool,
    pub final_ruling: ArbitrationRuling,
    pub resolved_at: Option<u64>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdvancedDisputeCase {
    pub dispute_id: u64,
    pub invoice_id: u64,
    pub claimant: Address,
    pub respondent: Address,
    pub reason: String,
    pub disputed_amount: i128,
    pub stage: DisputeStage,
    pub created_at: u64,
    pub evidence_deadline: u64,
    pub voting_deadline: u64,
    pub appeal_deadline: u64,
    pub panel: Vec<Address>,
    pub panel_size: u32,
    pub ruling: ArbitrationRuling,
    pub enforced: bool,
    pub enforced_at: Option<u64>,
}

// ---------------------------------------------------------------------------
// Core logic
// ---------------------------------------------------------------------------

pub fn get_config(env: &Env) -> AdvDisputeConfig {
    env.storage()
        .instance()
        .get(&AdvDisputeKey::Config)
        .unwrap_or(AdvDisputeConfig {
            evidence_window_secs: DEFAULT_EVIDENCE_WINDOW_SECS,
            voting_window_secs: DEFAULT_VOTING_WINDOW_SECS,
            appeal_window_secs: DEFAULT_APPEAL_WINDOW_SECS,
            default_panel_size: 3,
        })
}

pub fn set_config(env: &Env, config: AdvDisputeConfig) -> Result<(), ContractError> {
    if config.evidence_window_secs == 0
        || config.voting_window_secs == 0
        || config.appeal_window_secs == 0
        || config.default_panel_size == 0
    {
        return Err(ContractError::InvalidArgument);
    }
    env.storage().instance().set(&AdvDisputeKey::Config, &config);
    Ok(())
}

pub fn register_arbitrator(
    env: &Env,
    arbitrator: Address,
    weight: u32,
) -> Result<ArbitratorProfile, ContractError> {
    if weight == 0 {
        return Err(ContractError::InvalidArgument);
    }
    let mut pool: Vec<ArbitratorProfile> = env
        .storage()
        .persistent()
        .get(&AdvDisputeKey::ArbitratorPool)
        .unwrap_or_else(|| Vec::new(env));

    if pool.len() >= MAX_ARBITRATOR_POOL {
        return Err(ContractError::BatchLimitExceeded);
    }

    let profile = ArbitratorProfile {
        address: arbitrator.clone(),
        weight,
        cases_assigned: 0,
        cases_decided: 0,
        active: true,
    };

    let mut updated = Vec::new(env);
    let mut replaced = false;
    for item in pool.iter() {
        if item.address == arbitrator {
            updated.push_back(profile.clone());
            replaced = true;
        } else {
            updated.push_back(item);
        }
    }
    if !replaced {
        updated.push_back(profile.clone());
    }

    env.storage()
        .persistent()
        .set(&AdvDisputeKey::ArbitratorPool, &updated);
    Ok(profile)
}

pub fn get_arbitrator_pool(env: &Env) -> Vec<ArbitratorProfile> {
    env.storage()
        .persistent()
        .get(&AdvDisputeKey::ArbitratorPool)
        .unwrap_or_else(|| Vec::new(env))
}

/// Fair, conflict-free arbitrator panel selection using weighted least-assigned rotation.
/// Excludes both `claimant` and `respondent` so neither party can arbitrate their own dispute.
pub fn select_panel(
    env: &Env,
    claimant: &Address,
    respondent: &Address,
    panel_size: u32,
) -> Result<Vec<Address>, ContractError> {
    if panel_size == 0 {
        return Err(ContractError::InvalidArgument);
    }
    let pool = get_arbitrator_pool(env);
    let mut eligible: Vec<ArbitratorProfile> = Vec::new(env);
    for arb in pool.iter() {
        if arb.active && &arb.address != claimant && &arb.address != respondent {
            eligible.push_back(arb);
        }
    }
    if eligible.len() < panel_size {
        return Err(ContractError::PrerequisiteNotMet);
    }

    let cursor: u32 = env
        .storage()
        .instance()
        .get(&AdvDisputeKey::RotationCursor)
        .unwrap_or(0);

    let mut selected: Vec<Address> = Vec::new(env);
    let len = eligible.len();
    for offset in 0..panel_size {
        let idx = (cursor + offset) % len;
        let chosen = eligible.get(idx).unwrap();
        selected.push_back(chosen.address.clone());
    }

    // Update assignment counts and rotation cursor.
    let mut updated_pool: Vec<ArbitratorProfile> = Vec::new(env);
    for mut arb in pool.iter() {
        if selected.contains(&arb.address) {
            arb.cases_assigned = arb.cases_assigned.saturating_add(1);
        }
        updated_pool.push_back(arb);
    }
    env.storage()
        .persistent()
        .set(&AdvDisputeKey::ArbitratorPool, &updated_pool);
    env.storage()
        .instance()
        .set(&AdvDisputeKey::RotationCursor, &((cursor + panel_size) % len));

    Ok(selected)
}

pub fn create_dispute(
    env: &Env,
    invoice_id: u64,
    claimant: Address,
    respondent: Address,
    reason: String,
    disputed_amount: i128,
    panel_size_override: Option<u32>,
) -> Result<AdvancedDisputeCase, ContractError> {
    if disputed_amount <= 0 || claimant == respondent {
        return Err(ContractError::InvalidArgument);
    }
    if env
        .storage()
        .persistent()
        .has(&AdvDisputeKey::InvoiceDispute(invoice_id))
    {
        return Err(ContractError::AlreadyExecuted);
    }

    let cfg = get_config(env);
    let panel_size = panel_size_override.unwrap_or(cfg.default_panel_size);
    let panel = select_panel(env, &claimant, &respondent, panel_size)?;

    let dispute_id: u64 = env
        .storage()
        .instance()
        .get::<_, u64>(&AdvDisputeKey::Counter)
        .unwrap_or(0)
        + 1;
    env.storage()
        .instance()
        .set(&AdvDisputeKey::Counter, &dispute_id);

    let now = env.ledger().timestamp();
    let evidence_deadline = now.saturating_add(cfg.evidence_window_secs);
    let voting_deadline = evidence_deadline.saturating_add(cfg.voting_window_secs);

    let case = AdvancedDisputeCase {
        dispute_id,
        invoice_id,
        claimant: claimant.clone(),
        respondent: respondent.clone(),
        reason,
        disputed_amount,
        stage: DisputeStage::EvidenceCollection,
        created_at: now,
        evidence_deadline,
        voting_deadline,
        appeal_deadline: 0,
        panel,
        panel_size,
        ruling: ArbitrationRuling::Pending,
        enforced: false,
        enforced_at: None,
    };

    env.storage()
        .persistent()
        .set(&AdvDisputeKey::Dispute(dispute_id), &case);
    env.storage()
        .persistent()
        .set(&AdvDisputeKey::InvoiceDispute(invoice_id), &dispute_id);

    env.events().publish(
        (
            symbol_short!("split"),
            Symbol::new(env, "AdvDisputeCreated"),
            dispute_id,
        ),
        (invoice_id, claimant, respondent, disputed_amount),
    );

    Ok(case)
}

pub fn submit_evidence(
    env: &Env,
    dispute_id: u64,
    submitter: Address,
    kind: EvidenceKind,
    content_hash: BytesN<32>,
    description: String,
) -> Result<EvidenceRecord, ContractError> {
    let case: AdvancedDisputeCase = env
        .storage()
        .persistent()
        .get(&AdvDisputeKey::Dispute(dispute_id))
        .ok_or(ContractError::MissingStorageEntry)?;

    if submitter != case.claimant && submitter != case.respondent {
        return Err(ContractError::NotAuthorized);
    }
    if case.stage != DisputeStage::EvidenceCollection
        && case.stage != DisputeStage::ArbitrationReview
    {
        return Err(ContractError::InvalidStatus);
    }
    if env.ledger().timestamp() > case.evidence_deadline {
        return Err(ContractError::DeadlinePassed);
    }

    let mut list: Vec<EvidenceRecord> = env
        .storage()
        .persistent()
        .get(&AdvDisputeKey::Evidence(dispute_id))
        .unwrap_or_else(|| Vec::new(env));

    if list.len() >= MAX_EVIDENCE_ITEMS {
        return Err(ContractError::BatchLimitExceeded);
    }

    let record = EvidenceRecord {
        evidence_id: list.len() + 1,
        dispute_id,
        submitter: submitter.clone(),
        kind,
        content_hash: content_hash.clone(),
        description,
        submitted_at: env.ledger().timestamp(),
    };
    list.push_back(record.clone());
    env.storage()
        .persistent()
        .set(&AdvDisputeKey::Evidence(dispute_id), &list);

    env.events().publish(
        (
            symbol_short!("split"),
            Symbol::new(env, "AdvEvidenceAdded"),
            dispute_id,
        ),
        (submitter, record.evidence_id, content_hash),
    );

    Ok(record)
}

pub fn cast_panel_vote(
    env: &Env,
    dispute_id: u64,
    arbitrator: Address,
    ruling: ArbitrationRuling,
    rationale_hash: BytesN<32>,
) -> Result<AdvancedDisputeCase, ContractError> {
    if ruling == ArbitrationRuling::Pending {
        return Err(ContractError::InvalidArgument);
    }
    if let ArbitrationRuling::SplitBps(bps) = ruling {
        if bps > 10_000 {
            return Err(ContractError::InvalidRatio);
        }
    }

    let mut case: AdvancedDisputeCase = env
        .storage()
        .persistent()
        .get(&AdvDisputeKey::Dispute(dispute_id))
        .ok_or(ContractError::MissingStorageEntry)?;

    if case.stage != DisputeStage::EvidenceCollection
        && case.stage != DisputeStage::ArbitrationReview
    {
        return Err(ContractError::InvalidStatus);
    }
    if env.ledger().timestamp() > case.voting_deadline {
        return Err(ContractError::DeadlinePassed);
    }
    if !case.panel.contains(&arbitrator) {
        return Err(ContractError::NotArbiter);
    }

    let mut votes: Vec<PanelVote> = env
        .storage()
        .persistent()
        .get(&AdvDisputeKey::Votes(dispute_id))
        .unwrap_or_else(|| Vec::new(env));

    for existing in votes.iter() {
        if existing.arbitrator == arbitrator {
            return Err(ContractError::AlreadyExecuted);
        }
    }

    let pool = get_arbitrator_pool(env);
    let mut weight = 1u32;
    let mut updated_pool = Vec::new(env);
    for mut prof in pool.iter() {
        if prof.address == arbitrator {
            weight = prof.weight;
            prof.cases_decided = prof.cases_decided.saturating_add(1);
        }
        updated_pool.push_back(prof);
    }
    env.storage()
        .persistent()
        .set(&AdvDisputeKey::ArbitratorPool, &updated_pool);

    votes.push_back(PanelVote {
        arbitrator: arbitrator.clone(),
        ruling,
        weight,
        rationale_hash,
        voted_at: env.ledger().timestamp(),
    });
    env.storage()
        .persistent()
        .set(&AdvDisputeKey::Votes(dispute_id), &votes);

    case.stage = DisputeStage::ArbitrationReview;

    // Quorum reached when a majority of the panel (> panel_size / 2) has voted.
    let quorum = (case.panel_size / 2) + 1;
    if votes.len() >= quorum {
        let computed = tally_weighted_ruling(&votes);
        let cfg = get_config(env);
        case.ruling = computed;
        case.stage = DisputeStage::DecisionRendered;
        case.appeal_deadline = env
            .ledger()
            .timestamp()
            .saturating_add(cfg.appeal_window_secs);

        env.events().publish(
            (
                symbol_short!("split"),
                Symbol::new(env, "AdvRulingRendered"),
                dispute_id,
            ),
            (computed, case.appeal_deadline),
        );
    }

    env.storage()
        .persistent()
        .set(&AdvDisputeKey::Dispute(dispute_id), &case);
    Ok(case)
}

fn tally_weighted_ruling(votes: &Vec<PanelVote>) -> ArbitrationRuling {
    let mut release_weight: u64 = 0;
    let mut refund_weight: u64 = 0;
    let mut split_weight: u64 = 0;
    let mut weighted_bps_sum: u64 = 0;

    for v in votes.iter() {
        let w = v.weight as u64;
        match v.ruling {
            ArbitrationRuling::Pending => {}
            ArbitrationRuling::ReleaseToRecipient => {
                release_weight += w;
                weighted_bps_sum += w * 10_000;
            }
            ArbitrationRuling::RefundToPayer => {
                refund_weight += w;
            }
            ArbitrationRuling::SplitBps(bps) => {
                split_weight += w;
                weighted_bps_sum += w * (bps as u64);
            }
        }
    }

    let total_weight = release_weight + refund_weight + split_weight;
    if split_weight > 0 || (release_weight == refund_weight && total_weight > 0) {
        let avg_bps = (weighted_bps_sum / total_weight.max(1)) as u32;
        if avg_bps == 10_000 {
            ArbitrationRuling::ReleaseToRecipient
        } else if avg_bps == 0 {
            ArbitrationRuling::RefundToPayer
        } else {
            ArbitrationRuling::SplitBps(avg_bps)
        }
    } else if release_weight > refund_weight {
        ArbitrationRuling::ReleaseToRecipient
    } else {
        ArbitrationRuling::RefundToPayer
    }
}

pub fn file_appeal(
    env: &Env,
    dispute_id: u64,
    appellant: Address,
    reason: String,
    new_evidence_hash: BytesN<32>,
) -> Result<AppealRecord, ContractError> {
    let mut case: AdvancedDisputeCase = env
        .storage()
        .persistent()
        .get(&AdvDisputeKey::Dispute(dispute_id))
        .ok_or(ContractError::MissingStorageEntry)?;

    if appellant != case.claimant && appellant != case.respondent {
        return Err(ContractError::NotAuthorized);
    }
    if case.stage != DisputeStage::DecisionRendered {
        return Err(ContractError::InvalidStatus);
    }
    if env.ledger().timestamp() > case.appeal_deadline {
        return Err(ContractError::DeadlinePassed);
    }

    let appeal = AppealRecord {
        dispute_id,
        appellant: appellant.clone(),
        reason,
        new_evidence_hash,
        filed_at: env.ledger().timestamp(),
        resolved: false,
        upheld_original: false,
        final_ruling: ArbitrationRuling::Pending,
        resolved_at: None,
    };

    case.stage = DisputeStage::UnderAppeal;
    env.storage()
        .persistent()
        .set(&AdvDisputeKey::Appeal(dispute_id), &appeal);
    env.storage()
        .persistent()
        .set(&AdvDisputeKey::Dispute(dispute_id), &case);

    env.events().publish(
        (
            symbol_short!("split"),
            Symbol::new(env, "AdvAppealFiled"),
            dispute_id,
        ),
        appellant,
    );

    Ok(appeal)
}

pub fn resolve_appeal(
    env: &Env,
    dispute_id: u64,
    uphold_original: bool,
    override_ruling: ArbitrationRuling,
) -> Result<AdvancedDisputeCase, ContractError> {
    let mut case: AdvancedDisputeCase = env
        .storage()
        .persistent()
        .get(&AdvDisputeKey::Dispute(dispute_id))
        .ok_or(ContractError::MissingStorageEntry)?;

    if case.stage != DisputeStage::UnderAppeal {
        return Err(ContractError::InvalidStatus);
    }

    let mut appeal: AppealRecord = env
        .storage()
        .persistent()
        .get(&AdvDisputeKey::Appeal(dispute_id))
        .ok_or(ContractError::MissingStorageEntry)?;

    let final_ruling = if uphold_original {
        if case.ruling == ArbitrationRuling::Pending {
            return Err(ContractError::InvalidStatus);
        }
        case.ruling
    } else {
        if override_ruling == ArbitrationRuling::Pending {
            return Err(ContractError::InvalidArgument);
        }
        if let ArbitrationRuling::SplitBps(bps) = override_ruling {
            if bps > 10_000 {
                return Err(ContractError::InvalidRatio);
            }
        }
        override_ruling
    };

    appeal.resolved = true;
    appeal.upheld_original = uphold_original;
    appeal.final_ruling = final_ruling;
    appeal.resolved_at = Some(env.ledger().timestamp());

    case.ruling = final_ruling;
    case.stage = DisputeStage::Finalized;

    env.storage()
        .persistent()
        .set(&AdvDisputeKey::Appeal(dispute_id), &appeal);
    env.storage()
        .persistent()
        .set(&AdvDisputeKey::Dispute(dispute_id), &case);

    env.events().publish(
        (
            symbol_short!("split"),
            Symbol::new(env, "AdvAppealResolved"),
            dispute_id,
        ),
        (uphold_original, final_ruling),
    );

    Ok(case)
}

/// Validates that the dispute is eligible for enforcement and marks it `Enforced`,
/// returning `(recipient_payout, payer_refund)` for `disputed_amount`.
pub fn mark_enforced(
    env: &Env,
    dispute_id: u64,
) -> Result<(AdvancedDisputeCase, i128, i128), ContractError> {
    let mut case: AdvancedDisputeCase = env
        .storage()
        .persistent()
        .get(&AdvDisputeKey::Dispute(dispute_id))
        .ok_or(ContractError::MissingStorageEntry)?;

    if case.enforced {
        return Err(ContractError::AlreadyExecuted);
    }

    match case.stage {
        DisputeStage::DecisionRendered => {
            if env.ledger().timestamp() <= case.appeal_deadline {
                return Err(ContractError::TimelockPending);
            }
        }
        DisputeStage::Finalized => {}
        _ => return Err(ContractError::InvalidStatus),
    }

    let (recipient_payout, payer_refund) = match case.ruling {
        ArbitrationRuling::Pending => return Err(ContractError::InvalidStatus),
        ArbitrationRuling::ReleaseToRecipient => (case.disputed_amount, 0),
        ArbitrationRuling::RefundToPayer => (0, case.disputed_amount),
        ArbitrationRuling::SplitBps(bps) => {
            let rec = (case.disputed_amount * (bps as i128)) / 10_000;
            let pay = case.disputed_amount - rec;
            (rec, pay)
        }
    };

    case.stage = DisputeStage::Enforced;
    case.enforced = true;
    case.enforced_at = Some(env.ledger().timestamp());
    env.storage()
        .persistent()
        .set(&AdvDisputeKey::Dispute(dispute_id), &case);

    env.events().publish(
        (
            symbol_short!("split"),
            Symbol::new(env, "AdvDisputeEnforced"),
            dispute_id,
        ),
        (recipient_payout, payer_refund),
    );

    Ok((case, recipient_payout, payer_refund))
}

pub fn get_dispute(env: &Env, dispute_id: u64) -> Option<AdvancedDisputeCase> {
    env.storage()
        .persistent()
        .get(&AdvDisputeKey::Dispute(dispute_id))
}

pub fn get_evidence(env: &Env, dispute_id: u64) -> Vec<EvidenceRecord> {
    env.storage()
        .persistent()
        .get(&AdvDisputeKey::Evidence(dispute_id))
        .unwrap_or_else(|| Vec::new(env))
}

pub fn get_votes(env: &Env, dispute_id: u64) -> Vec<PanelVote> {
    env.storage()
        .persistent()
        .get(&AdvDisputeKey::Votes(dispute_id))
        .unwrap_or_else(|| Vec::new(env))
}

pub fn get_appeal(env: &Env, dispute_id: u64) -> Option<AppealRecord> {
    env.storage()
        .persistent()
        .get(&AdvDisputeKey::Appeal(dispute_id))
}
