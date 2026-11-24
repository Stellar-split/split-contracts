//! Contract state validation (issue #1220), first slice.
//!
//! Pure helpers that validate invoice state transitions, check structural
//! invariants, prevent invalid states, and record a bounded state-change
//! log. Not wired to contract storage or entrypoints; the authoritative
//! status lives in [`crate::types`] and would be projected into
//! [`ContractState`] before validation.

#![allow(dead_code)]

/// Maximum entries retained in the state-change log.
pub const MAX_LOG_ENTRIES: usize = 64;

/// Normalised contract state mirroring the invoice lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContractState {
    /// Created but not yet open for payment.
    Draft,
    /// Open for contributions, not yet fully funded.
    Pending,
    /// Fully funded, not yet released.
    Funded,
    /// Partially released (milestone/instalment).
    PartiallyReleased,
    /// All recipients paid out; terminal.
    Released,
    /// Payers refunded; terminal.
    Refunded,
    /// Deadline passed unfunded; terminal.
    Expired,
    /// Creator-cancelled; terminal.
    Cancelled,
    /// Under dispute.
    Disputed,
    /// Moved to cold storage; terminal.
    Archived,
}

/// Errors returned by the validators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationError {
    /// The requested `from -> to` transition is not allowed.
    InvalidTransition,
    /// A structural invariant of the snapshot does not hold.
    InvariantViolated,
    /// `funded` exceeds `total_due`.
    OverFunded,
    /// A non-draft invoice must have at least one recipient.
    NoRecipients,
    /// A `Released`/`Funded` invoice must be fully funded.
    NotFullyFunded,
    /// `total_due` must be non-negative for a payable invoice.
    InvalidAmount,
    /// The state-change log is at capacity.
    LogFull,
}

/// A point-in-time view of an invoice used for invariant checking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContractSnapshot {
    pub invoice_id: u64,
    pub state: ContractState,
    pub total_due: i128,
    pub funded: i128,
    pub recipient_count: u32,
    pub deadline_ts: u64,
}

/// One recorded state transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateChange {
    pub invoice_id: u64,
    pub from: ContractState,
    pub to: ContractState,
    pub ledger: u32,
    pub timestamp: u64,
}

/// Bounded append-only log of state changes.
pub struct StateChangeLog {
    entries: [StateChange; MAX_LOG_ENTRIES],
    len: usize,
}

impl Default for StateChangeLog {
    fn default() -> Self {
        Self {
            entries: [StateChange {
                invoice_id: 0,
                from: ContractState::Draft,
                to: ContractState::Draft,
                ledger: 0,
                timestamp: 0,
            }; MAX_LOG_ENTRIES],
            len: 0,
        }
    }
}

impl StateChangeLog {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn entries(&self) -> &[StateChange] {
        &self.entries[..self.len]
    }

    /// Append a recorded transition. Returns [`ValidationError::LogFull`] at
    /// capacity.
    pub fn record(&mut self, c: StateChange) -> Result<(), ValidationError> {
        if self.len >= MAX_LOG_ENTRIES {
            return Err(ValidationError::LogFull);
        }
        self.entries[self.len] = c;
        self.len += 1;
        Ok(())
    }
}

/// Allowed transitions. A transition must change the state; terminal states
/// (`Archived`) allow nothing out.
pub fn is_valid_transition(from: ContractState, to: ContractState) -> bool {
    use ContractState::*;
    if from == to {
        return false;
    }
    matches!(
        (from, to),
        (Draft, Pending)
            | (Pending, Funded)
            | (Pending, PartiallyReleased)
            | (Pending, Cancelled)
            | (Pending, Expired)
            | (Pending, Disputed)
            | (Funded, Released)
            | (Funded, Disputed)
            | (PartiallyReleased, Released)
            | (PartiallyReleased, Disputed)
            | (Disputed, Pending)
            | (Disputed, Released)
            | (Disputed, Refunded)
            | (Released, Archived)
            | (Refunded, Archived)
            | (Expired, Archived)
            | (Cancelled, Archived)
    )
}

/// Validate that a single `from -> to` transition is allowed.
pub fn validate_transition(from: ContractState, to: ContractState) -> Result<(), ValidationError> {
    if is_valid_transition(from, to) {
        Ok(())
    } else {
        Err(ValidationError::InvalidTransition)
    }
}

/// Check the structural invariants of a snapshot. Returns the first violated
/// invariant, or `Ok(())` if the snapshot is consistent.
pub fn check_invariants(s: &ContractSnapshot) -> Result<(), ValidationError> {
    use ContractState::*;
    if s.total_due < 0 {
        return Err(ValidationError::InvalidAmount);
    }
    if s.funded < 0 || s.funded > s.total_due {
        return Err(ValidationError::OverFunded);
    }
    if s.state != Draft && s.recipient_count == 0 {
        return Err(ValidationError::NoRecipients);
    }
    if s.state != Draft && s.deadline_ts == 0 {
        return Err(ValidationError::InvariantViolated);
    }
    match s.state {
        Funded | Released if s.funded != s.total_due => Err(ValidationError::NotFullyFunded),
        PartiallyReleased if s.funded == 0 || s.funded >= s.total_due => {
            Err(ValidationError::InvariantViolated)
        }
        _ => Ok(()),
    }
}

/// Convenience: a snapshot satisfies all invariants.
pub fn is_valid_state(s: &ContractSnapshot) -> bool {
    check_invariants(s).is_ok()
}

/// Validate a transition, check the prospective snapshot's invariants, and
/// record the change. The state is only committed once both validation and
/// logging succeed, so a full log leaves the snapshot untouched.
pub fn apply_transition(
    s: &mut ContractSnapshot,
    log: &mut StateChangeLog,
    to: ContractState,
    ledger: u32,
    timestamp: u64,
) -> Result<(), ValidationError> {
    validate_transition(s.state, to)?;
    let prev = s.state;
    let mut next = *s;
    next.state = to;
    check_invariants(&next)?;
    log.record(StateChange {
        invoice_id: s.invoice_id,
        from: prev,
        to,
        ledger,
        timestamp,
    })?;
    *s = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(
        id: u64,
        state: ContractState,
        total: i128,
        funded: i128,
        rc: u32,
        dl: u64,
    ) -> ContractSnapshot {
        ContractSnapshot {
            invoice_id: id,
            state,
            total_due: total,
            funded,
            recipient_count: rc,
            deadline_ts: dl,
        }
    }

    #[test]
    fn valid_transitions_allowed() {
        use ContractState::*;
        assert!(is_valid_transition(Draft, Pending));
        assert!(is_valid_transition(Pending, Funded));
        assert!(is_valid_transition(Funded, Released));
        assert!(is_valid_transition(Released, Archived));
        assert!(is_valid_transition(Pending, Disputed));
        assert!(is_valid_transition(Disputed, Released));
    }

    #[test]
    fn invalid_transitions_rejected() {
        use ContractState::*;
        assert!(!is_valid_transition(Released, Pending)); // no going back
        assert!(!is_valid_transition(Archived, Pending)); // terminal
        assert!(!is_valid_transition(Draft, Released)); // skips steps
        assert!(!is_valid_transition(Pending, Pending)); // no-op
        assert_eq!(
            validate_transition(Released, Pending),
            Err(ValidationError::InvalidTransition)
        );
    }

    #[test]
    fn invariants_pass_for_valid_snapshot() {
        let s = snap(1, ContractState::Funded, 1000, 1000, 2, 1000);
        assert!(is_valid_state(&s));
        assert_eq!(check_invariants(&s), Ok(()));
    }

    #[test]
    fn invariants_detect_violations() {
        // over-funded
        assert_eq!(
            check_invariants(&snap(1, ContractState::Pending, 1000, 1200, 2, 1000)),
            Err(ValidationError::OverFunded)
        );
        // released but not fully funded
        assert_eq!(
            check_invariants(&snap(1, ContractState::Released, 1000, 500, 2, 1000)),
            Err(ValidationError::NotFullyFunded)
        );
        // no recipients on a payable invoice
        assert_eq!(
            check_invariants(&snap(1, ContractState::Pending, 1000, 0, 0, 1000)),
            Err(ValidationError::NoRecipients)
        );
        // negative total
        assert_eq!(
            check_invariants(&snap(1, ContractState::Pending, -1, 0, 1, 1000)),
            Err(ValidationError::InvalidAmount)
        );
    }

    #[test]
    fn state_change_log_records_in_order() {
        let mut log = StateChangeLog::new();
        assert!(log.is_empty());
        log.record(StateChange {
            invoice_id: 1,
            from: ContractState::Draft,
            to: ContractState::Pending,
            ledger: 1,
            timestamp: 10,
        })
        .unwrap();
        log.record(StateChange {
            invoice_id: 1,
            from: ContractState::Pending,
            to: ContractState::Funded,
            ledger: 2,
            timestamp: 20,
        })
        .unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log.entries()[0].to, ContractState::Pending);
        assert_eq!(log.entries()[1].to, ContractState::Funded);
    }

    #[test]
    fn apply_transition_validates_and_logs() {
        let mut s = snap(1, ContractState::Pending, 1000, 1000, 2, 1000);
        let mut log = StateChangeLog::new();
        // Valid: Pending -> Funded (already fully funded).
        assert!(apply_transition(&mut s, &mut log, ContractState::Funded, 5, 50).is_ok());
        assert_eq!(s.state, ContractState::Funded);
        assert_eq!(log.len(), 1);
        assert_eq!(log.entries()[0].from, ContractState::Pending);
        assert_eq!(log.entries()[0].to, ContractState::Funded);
        // Invalid: Funded -> Pending is rejected; state and log unchanged.
        assert_eq!(
            apply_transition(&mut s, &mut log, ContractState::Pending, 6, 60),
            Err(ValidationError::InvalidTransition)
        );
        assert_eq!(s.state, ContractState::Funded);
        assert_eq!(log.len(), 1);
    }
}
