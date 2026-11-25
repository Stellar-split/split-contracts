//! Issue #1184: arbitrated escrow for disputed invoices (first slice).
//!
//! Pure helpers: a dispute stage machine, arbiter assignment from a candidate
//! pool (never a party to the dispute), bounded evidence submission per side,
//! an escrow lock check, ruling-based division of the escrowed amount and
//! dispute counters.

use soroban_sdk::Address;

/// Maximum evidence items each side may submit.
pub const MAX_EVIDENCE_PER_SIDE: u32 = 10;

/// Maximum arbiter candidates considered per assignment.
pub const MAX_ARBITER_CANDIDATES: usize = 20;

const BPS_DENOMINATOR: i128 = 10_000;

/// Dispute lifecycle, in order.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DisputeStage {
    /// Raised; waiting for an arbiter.
    Open,
    /// Arbiter assigned; both sides may submit evidence.
    Evidence,
    /// Evidence closed; arbiter is deciding.
    Deliberation,
    /// Ruling issued; escrow may be paid out.
    Resolved,
}

/// Side of the dispute.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Party {
    /// Invoice creator (payee).
    Creator,
    /// Payer.
    Payer,
}

/// Errors for arbitration helpers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArbitrationError {
    /// Action not allowed in the current stage.
    WrongStage,
    /// Evidence window has not closed yet.
    EvidenceWindowOpen,
    /// Evidence window has closed.
    EvidenceWindowClosed,
    /// Side reached [`MAX_EVIDENCE_PER_SIDE`].
    EvidenceLimit,
    /// No eligible arbiter in the pool.
    NoEligibleArbiter,
    /// Too many candidates supplied.
    TooManyCandidates,
    /// Ruling bps above 10_000.
    InvalidRuling,
    /// Escrowed amount is negative.
    InvalidAmount,
    /// Arithmetic overflow.
    Overflow,
}

/// Picks an arbiter from `candidates`, skipping the creator and payer.
/// `seed` selects among eligible candidates (e.g. the invoice id).
pub fn assign_arbiter(
    candidates: &[Address],
    creator: &Address,
    payer: &Address,
    seed: u64,
) -> Result<Address, ArbitrationError> {
    if candidates.len() > MAX_ARBITER_CANDIDATES {
        return Err(ArbitrationError::TooManyCandidates);
    }
    let eligible = |a: &&Address| *a != creator && *a != payer;
    let count = candidates.iter().filter(eligible).count();
    if count == 0 {
        return Err(ArbitrationError::NoEligibleArbiter);
    }
    let pick = (seed % count as u64) as usize;
    Ok(candidates.iter().filter(eligible).nth(pick).unwrap().clone())
}

/// Moves `Open` to `Evidence` once an arbiter is assigned.
pub fn start_evidence(stage: DisputeStage) -> Result<DisputeStage, ArbitrationError> {
    match stage {
        DisputeStage::Open => Ok(DisputeStage::Evidence),
        _ => Err(ArbitrationError::WrongStage),
    }
}

/// Moves `Evidence` to `Deliberation` once `now` reaches `evidence_deadline`.
pub fn close_evidence(
    stage: DisputeStage,
    now: u64,
    evidence_deadline: u64,
) -> Result<DisputeStage, ArbitrationError> {
    if stage != DisputeStage::Evidence {
        return Err(ArbitrationError::WrongStage);
    }
    if now < evidence_deadline {
        return Err(ArbitrationError::EvidenceWindowOpen);
    }
    Ok(DisputeStage::Deliberation)
}

/// Evidence counts per side.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EvidenceLog {
    /// Items submitted by the creator.
    pub creator_items: u32,
    /// Items submitted by the payer.
    pub payer_items: u32,
}

impl EvidenceLog {
    /// Records one evidence item from `party`. Allowed only in the
    /// `Evidence` stage before `evidence_deadline`.
    pub fn submit(
        &mut self,
        stage: DisputeStage,
        party: Party,
        now: u64,
        evidence_deadline: u64,
    ) -> Result<(), ArbitrationError> {
        if stage != DisputeStage::Evidence {
            return Err(ArbitrationError::WrongStage);
        }
        if now >= evidence_deadline {
            return Err(ArbitrationError::EvidenceWindowClosed);
        }
        let count = match party {
            Party::Creator => &mut self.creator_items,
            Party::Payer => &mut self.payer_items,
        };
        if *count >= MAX_EVIDENCE_PER_SIDE {
            return Err(ArbitrationError::EvidenceLimit);
        }
        *count += 1;
        Ok(())
    }
}

/// True when escrowed funds may leave the contract: never while a dispute
/// is unresolved.
pub fn escrow_releasable(stage: DisputeStage) -> bool {
    stage == DisputeStage::Resolved
}

/// Division of the escrowed amount decided by a ruling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RulingPayout {
    /// Returned to the payer.
    pub to_payer: i128,
    /// Paid to the creator.
    pub to_creator: i128,
}

/// Issues a ruling in `Deliberation`: the payer receives `payer_bps` of
/// `escrowed` (rounded down) and the creator the rest. Returns the payout
/// and the `Resolved` stage.
pub fn issue_ruling(
    stage: DisputeStage,
    escrowed: i128,
    payer_bps: u32,
) -> Result<(RulingPayout, DisputeStage), ArbitrationError> {
    if stage != DisputeStage::Deliberation {
        return Err(ArbitrationError::WrongStage);
    }
    if payer_bps as i128 > BPS_DENOMINATOR {
        return Err(ArbitrationError::InvalidRuling);
    }
    if escrowed < 0 {
        return Err(ArbitrationError::InvalidAmount);
    }
    let to_payer = escrowed
        .checked_mul(payer_bps as i128)
        .ok_or(ArbitrationError::Overflow)?
        / BPS_DENOMINATOR;
    Ok((
        RulingPayout { to_payer, to_creator: escrowed - to_payer },
        DisputeStage::Resolved,
    ))
}

/// Dispute counters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DisputeStats {
    /// Disputes raised.
    pub opened: u32,
    /// Disputes resolved.
    pub resolved: u32,
    /// Rulings giving the payer more than half.
    pub favoured_payer: u32,
    /// Rulings giving the creator at least half.
    pub favoured_creator: u32,
}

impl DisputeStats {
    /// Records a newly raised dispute (saturating).
    pub fn record_opened(&mut self) {
        self.opened = self.opened.saturating_add(1);
    }

    /// Records a ruling (saturating).
    pub fn record_ruling(&mut self, payout: &RulingPayout) {
        self.resolved = self.resolved.saturating_add(1);
        if payout.to_payer > payout.to_creator {
            self.favoured_payer = self.favoured_payer.saturating_add(1);
        } else {
            self.favoured_creator = self.favoured_creator.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::Env;

    #[test]
    fn arbiter_is_never_a_party() {
        let env = Env::default();
        let creator = Address::generate(&env);
        let payer = Address::generate(&env);
        let arb = Address::generate(&env);
        let pool = [creator.clone(), payer.clone(), arb.clone()];
        for seed in 0..5 {
            assert_eq!(assign_arbiter(&pool, &creator, &payer, seed).unwrap(), arb);
        }
        assert_eq!(
            assign_arbiter(&pool[..2], &creator, &payer, 0),
            Err(ArbitrationError::NoEligibleArbiter)
        );
    }

    #[test]
    fn full_lifecycle() {
        let stage = start_evidence(DisputeStage::Open).unwrap();
        assert!(!escrow_releasable(stage));

        let mut log = EvidenceLog::default();
        log.submit(stage, Party::Payer, 10, 100).unwrap();
        log.submit(stage, Party::Creator, 20, 100).unwrap();
        assert_eq!(log, EvidenceLog { creator_items: 1, payer_items: 1 });
        assert_eq!(
            log.submit(stage, Party::Payer, 100, 100),
            Err(ArbitrationError::EvidenceWindowClosed)
        );

        assert_eq!(close_evidence(stage, 50, 100), Err(ArbitrationError::EvidenceWindowOpen));
        let stage = close_evidence(stage, 100, 100).unwrap();
        assert_eq!(stage, DisputeStage::Deliberation);

        let (payout, stage) = issue_ruling(stage, 1_001, 7_000).unwrap();
        assert_eq!(payout, RulingPayout { to_payer: 700, to_creator: 301 });
        assert!(escrow_releasable(stage));

        let mut stats = DisputeStats::default();
        stats.record_opened();
        stats.record_ruling(&payout);
        assert_eq!(stats.favoured_payer, 1);
        assert_eq!(stats.resolved, 1);
    }

    #[test]
    fn evidence_limit_and_bad_ruling() {
        let mut log = EvidenceLog::default();
        for _ in 0..MAX_EVIDENCE_PER_SIDE {
            log.submit(DisputeStage::Evidence, Party::Creator, 0, 1).unwrap();
        }
        assert_eq!(
            log.submit(DisputeStage::Evidence, Party::Creator, 0, 1),
            Err(ArbitrationError::EvidenceLimit)
        );
        assert_eq!(
            issue_ruling(DisputeStage::Deliberation, 100, 10_001),
            Err(ArbitrationError::InvalidRuling)
        );
        assert_eq!(issue_ruling(DisputeStage::Evidence, 100, 0), Err(ArbitrationError::WrongStage));
    }
}
