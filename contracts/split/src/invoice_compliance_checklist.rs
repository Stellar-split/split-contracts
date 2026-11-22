//! Issue #1168: invoice compliance checklist (first slice).
//!
//! Pure helpers over a caller-held slice of checklist items: a default
//! template, completion tracking and verification. Not covered: persistent
//! storage, per-invoice assignment, custom template management entry points
//! and events.

/// Maximum items in one checklist.
pub const MAX_CHECKLIST_ITEMS: usize = 32;

/// Errors for checklist helpers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChecklistError {
    /// Item id not present.
    ItemNotFound,
}

/// One compliance item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChecklistItem {
    /// Item identifier.
    pub id: u32,
    /// Whether the item must be done for verification to pass.
    pub required: bool,
    /// Whether the item is done.
    pub done: bool,
}

/// Default template: items 1-3 required, item 4 optional.
pub fn standard_template() -> [ChecklistItem; 4] {
    [
        ChecklistItem { id: 1, required: true, done: false },
        ChecklistItem { id: 2, required: true, done: false },
        ChecklistItem { id: 3, required: true, done: false },
        ChecklistItem { id: 4, required: false, done: false },
    ]
}

/// Marks the item with `id` as done.
pub fn mark_done(items: &mut [ChecklistItem], id: u32) -> Result<(), ChecklistError> {
    for item in items.iter_mut() {
        if item.id == id {
            item.done = true;
            return Ok(());
        }
    }
    Err(ChecklistError::ItemNotFound)
}

/// Number of required items not yet done.
pub fn missing_required(items: &[ChecklistItem]) -> u32 {
    items.iter().filter(|i| i.required && !i.done).count() as u32
}

/// True when every required item is done.
pub fn is_compliant(items: &[ChecklistItem]) -> bool {
    missing_required(items) == 0
}

/// Completion of all items in basis points (10_000 = all done; empty = 10_000).
pub fn completion_bps(items: &[ChecklistItem]) -> u32 {
    if items.is_empty() {
        return 10_000;
    }
    let done = items.iter().filter(|i| i.done).count() as u32;
    done * 10_000 / items.len() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_and_verifies() {
        let mut t = standard_template();
        assert!(!is_compliant(&t));
        assert_eq!(missing_required(&t), 3);
        for id in 1..=3 {
            mark_done(&mut t, id).unwrap();
        }
        assert!(is_compliant(&t));
        assert_eq!(completion_bps(&t), 7_500);
        assert_eq!(mark_done(&mut t, 99), Err(ChecklistError::ItemNotFound));
    }
}
