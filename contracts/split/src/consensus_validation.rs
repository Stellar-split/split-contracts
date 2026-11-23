//! Issue #1173: quorum voting tally (first slice).
//!
//! A heap-free, fixed-capacity voting tally with a quorum threshold: each
//! registered validator index may vote once (approve or reject), and the
//! outcome is derived from the quorum rule. This is a vote-counting helper
//! only. It is NOT a Byzantine fault tolerant consensus protocol: there is no
//! networking, leader election, rounds, signature checking or equivocation
//! proof handling.

/// Maximum number of validators.
pub const MAX_VALIDATORS: usize = 16;

/// Errors returned by the tally.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsensusError {
    /// Validator count is zero or above `MAX_VALIDATORS`.
    InvalidValidatorCount,
    /// Quorum is zero or larger than the validator count.
    InvalidQuorum,
    /// Validator index is out of range.
    UnknownValidator,
    /// The validator already voted.
    AlreadyVoted,
}

/// Outcome of a tally.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Enough approvals to reach quorum.
    Approved,
    /// Quorum can no longer be reached by approvals.
    Rejected,
    /// Still undecided.
    Pending,
}

/// Vote tally with a quorum threshold.
#[derive(Clone, Copy, Debug)]
pub struct Tally {
    validators: u32,
    quorum: u32,
    votes: [Option<bool>; MAX_VALIDATORS],
}

impl Tally {
    /// Creates a tally for `validators` voters needing `quorum` approvals.
    /// A strict majority quorum is recommended (`validators / 2 + 1`).
    pub fn new(validators: u32, quorum: u32) -> Result<Self, ConsensusError> {
        if validators == 0 || validators as usize > MAX_VALIDATORS {
            return Err(ConsensusError::InvalidValidatorCount);
        }
        if quorum == 0 || quorum > validators {
            return Err(ConsensusError::InvalidQuorum);
        }
        Ok(Tally { validators, quorum, votes: [None; MAX_VALIDATORS] })
    }

    /// Records one vote per validator index.
    pub fn vote(&mut self, validator: u32, approve: bool) -> Result<(), ConsensusError> {
        if validator >= self.validators {
            return Err(ConsensusError::UnknownValidator);
        }
        let slot = &mut self.votes[validator as usize];
        if slot.is_some() {
            return Err(ConsensusError::AlreadyVoted);
        }
        *slot = Some(approve);
        Ok(())
    }

    /// Returns `(approvals, rejections)`.
    pub fn counts(&self) -> (u32, u32) {
        let mut a = 0u32;
        let mut r = 0u32;
        for v in self.votes.iter() {
            match v {
                Some(true) => a += 1,
                Some(false) => r += 1,
                None => {}
            }
        }
        (a, r)
    }

    /// Current outcome under the quorum rule.
    pub fn outcome(&self) -> Outcome {
        let (a, r) = self.counts();
        if a >= self.quorum {
            Outcome::Approved
        } else if self.validators - r < self.quorum {
            Outcome::Rejected
        } else {
            Outcome::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approves_at_quorum() {
        let mut t = Tally::new(5, 3).unwrap();
        t.vote(0, true).unwrap();
        t.vote(1, false).unwrap();
        t.vote(2, true).unwrap();
        assert_eq!(t.outcome(), Outcome::Pending);
        t.vote(3, true).unwrap();
        assert_eq!(t.outcome(), Outcome::Approved);
        assert_eq!(t.counts(), (3, 1));
    }

    #[test]
    fn rejects_when_quorum_unreachable() {
        let mut t = Tally::new(3, 2).unwrap();
        t.vote(0, false).unwrap();
        assert_eq!(t.outcome(), Outcome::Pending);
        t.vote(1, false).unwrap();
        assert_eq!(t.outcome(), Outcome::Rejected);
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(Tally::new(0, 1).err(), Some(ConsensusError::InvalidValidatorCount));
        assert_eq!(Tally::new(3, 4).err(), Some(ConsensusError::InvalidQuorum));
        let mut t = Tally::new(3, 2).unwrap();
        assert_eq!(t.vote(3, true), Err(ConsensusError::UnknownValidator));
        t.vote(0, true).unwrap();
        assert_eq!(t.vote(0, false), Err(ConsensusError::AlreadyVoted));
    }
}
