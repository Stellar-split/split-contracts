//! Contract upgrade and migration toolkit (issue #1192).
//!
//! Pure helpers around the upgrade flow in `docs/UPGRADE_MIGRATION_GUIDE.md`
//! and the schema migrations in `migrations.rs`:
//!
//! - [`plan_migration`] checks a stored schema version against a target and
//!   lists the `migration_vN` steps that `migrate` will run.
//! - [`MigrationCursor`] splits an invoice id range into fixed-size batches,
//!   for migrations too large for one transaction.
//! - [`next_action`] maps what an operator reads from the contract views to
//!   the next upgrade call, so a script can drive the upgrade step by step.
//!
//! See `docs/UPGRADE_TOOLKIT.md` for usage and examples. Not wired to contract
//! storage: callers pass in the values they read.

#![allow(dead_code)]

use crate::migrations::CURRENT_SCHEMA_VERSION;

/// Default number of invoice ids processed per migration batch.
pub const DEFAULT_BATCH_SIZE: u64 = 50;

/// Errors returned by toolkit helpers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolkitError {
    /// Target schema is lower than the stored one; migrations only go forward.
    Downgrade,
    /// Target schema is newer than this build understands.
    UnknownTarget,
    /// Batch size is zero.
    InvalidBatch,
}

/// The migration steps between two schema versions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MigrationPlan {
    /// Schema version stored on chain.
    pub from: u32,
    /// Schema version after all steps have run.
    pub to: u32,
}

impl MigrationPlan {
    /// Number of `migration_vN` steps to run.
    pub fn step_count(&self) -> u32 {
        self.to - self.from
    }

    /// `true` when the stored schema is already at the target.
    pub fn is_noop(&self) -> bool {
        self.from == self.to
    }

    /// Target version of the `index`-th step (0-based), i.e. the `N` of the
    /// `migration_vN` that runs, or `None` past the last step.
    pub fn step(&self, index: u32) -> Option<u32> {
        if index < self.step_count() {
            Some(self.from + index + 1)
        } else {
            None
        }
    }
}

/// Plans the migration from `stored` to `target`. A stored version of 0 is
/// treated as 1, matching `migrations::schema_version` for contracts that
/// never wrote the key.
pub fn plan_migration(stored: u32, target: u32) -> Result<MigrationPlan, ToolkitError> {
    let from = stored.max(1);
    if target > CURRENT_SCHEMA_VERSION {
        return Err(ToolkitError::UnknownTarget);
    }
    if from > target {
        return Err(ToolkitError::Downgrade);
    }
    Ok(MigrationPlan { from, to: target })
}

/// Walks invoice ids `1..=last_id` in batches of `batch` ids.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MigrationCursor {
    next_id: u64,
    last_id: u64,
    batch: u64,
}

impl MigrationCursor {
    /// Creates a cursor over `1..=last_id` (the invoice counter holds the
    /// highest assigned id).
    pub fn new(last_id: u64, batch: u64) -> Result<Self, ToolkitError> {
        Self::resume(1, last_id, batch)
    }

    /// Resumes a cursor at `next_id`, e.g. from a stored checkpoint.
    pub fn resume(next_id: u64, last_id: u64, batch: u64) -> Result<Self, ToolkitError> {
        if batch == 0 {
            return Err(ToolkitError::InvalidBatch);
        }
        Ok(Self {
            next_id: next_id.max(1),
            last_id,
            batch,
        })
    }

    /// Next id to process; store it to resume later.
    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    /// `true` once every id has been handed out.
    pub fn is_done(&self) -> bool {
        self.next_id > self.last_id
    }

    /// Ids not yet handed out.
    pub fn remaining(&self) -> u64 {
        if self.is_done() {
            0
        } else {
            self.last_id - self.next_id + 1
        }
    }

    /// Returns the next inclusive `(start, end)` range and advances, or `None`
    /// when done.
    pub fn next_range(&mut self) -> Option<(u64, u64)> {
        if self.is_done() {
            return None;
        }
        let start = self.next_id;
        let end = start.saturating_add(self.batch - 1).min(self.last_id);
        self.next_id = end.saturating_add(1);
        Some((start, end))
    }
}

/// What an operator reads from the contract before deciding the next step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpgradeSnapshot {
    /// The deployed WASM is the new build.
    pub upgraded: bool,
    /// `get_upgrade_proposal` returned a proposal.
    pub proposal_pending: bool,
    /// `eligible_at` of the pending proposal.
    pub eligible_at: u64,
    /// Current ledger timestamp.
    pub now: u64,
    /// `get_schema_version`.
    pub stored_schema: u32,
    /// Schema version the new build expects.
    pub target_schema: u32,
    /// `get_upgrade_checkpoint` returned a hash.
    pub frozen: bool,
}

/// The next upgrade call to make.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeAction {
    /// Call `propose_upgrade`.
    Propose,
    /// Wait until `eligible_at`.
    WaitTimelock,
    /// Call `execute_upgrade`.
    Execute,
    /// Call `migrate`.
    Migrate,
    /// Call `thaw_contract`.
    Thaw,
    /// Nothing left to do; run the post-upgrade checks.
    Done,
}

/// Returns the next step of the upgrade flow for `s`.
pub fn next_action(s: &UpgradeSnapshot) -> UpgradeAction {
    if !s.upgraded {
        if !s.proposal_pending {
            UpgradeAction::Propose
        } else if s.now < s.eligible_at {
            UpgradeAction::WaitTimelock
        } else {
            UpgradeAction::Execute
        }
    } else if s.stored_schema.max(1) < s.target_schema {
        UpgradeAction::Migrate
    } else if s.frozen {
        UpgradeAction::Thaw
    } else {
        UpgradeAction::Done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_pending_steps() {
        let p = plan_migration(1, CURRENT_SCHEMA_VERSION).unwrap();
        assert_eq!(p.from, 1);
        assert_eq!(p.step_count(), CURRENT_SCHEMA_VERSION - 1);
        assert_eq!(p.step(0), Some(2));
        assert_eq!(p.step(p.step_count()), None);
        assert!(
            plan_migration(CURRENT_SCHEMA_VERSION, CURRENT_SCHEMA_VERSION)
                .unwrap()
                .is_noop()
        );
        assert_eq!(plan_migration(0, 1).unwrap().from, 1);
    }

    #[test]
    fn rejects_bad_targets() {
        assert_eq!(plan_migration(3, 2), Err(ToolkitError::Downgrade));
        assert_eq!(
            plan_migration(1, CURRENT_SCHEMA_VERSION + 1),
            Err(ToolkitError::UnknownTarget)
        );
    }

    #[test]
    fn cursor_batches_and_resumes() {
        let mut c = MigrationCursor::new(7, 3).unwrap();
        assert_eq!(c.remaining(), 7);
        assert_eq!(c.next_range(), Some((1, 3)));
        assert_eq!(c.next_range(), Some((4, 6)));
        let saved = c.next_id();
        let mut r = MigrationCursor::resume(saved, 7, 3).unwrap();
        assert_eq!(r.next_range(), Some((7, 7)));
        assert!(r.is_done());
        assert_eq!(r.next_range(), None);
        assert_eq!(MigrationCursor::new(0, 3).unwrap().next_range(), None);
        assert_eq!(MigrationCursor::new(5, 0), Err(ToolkitError::InvalidBatch));
    }

    #[test]
    fn next_action_follows_flow() {
        let mut s = UpgradeSnapshot {
            upgraded: false,
            proposal_pending: false,
            eligible_at: 0,
            now: 100,
            stored_schema: 2,
            target_schema: 3,
            frozen: true,
        };
        assert_eq!(next_action(&s), UpgradeAction::Propose);
        s.proposal_pending = true;
        s.eligible_at = 200;
        assert_eq!(next_action(&s), UpgradeAction::WaitTimelock);
        s.now = 200;
        assert_eq!(next_action(&s), UpgradeAction::Execute);
        s.upgraded = true;
        s.proposal_pending = false;
        assert_eq!(next_action(&s), UpgradeAction::Migrate);
        s.stored_schema = 3;
        assert_eq!(next_action(&s), UpgradeAction::Thaw);
        s.frozen = false;
        assert_eq!(next_action(&s), UpgradeAction::Done);
    }
}
