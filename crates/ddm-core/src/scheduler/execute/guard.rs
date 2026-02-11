//! RAII guard that releases reserved connections when dropped.

use super::super::budget::GlobalConnectionBudget;

/// Releases reserved connections when dropped.
pub(super) struct BudgetGuard<'a> {
    pub(super) budget: &'a GlobalConnectionBudget,
    pub(super) reserved: usize,
}

impl Drop for BudgetGuard<'_> {
    fn drop(&mut self) {
        self.budget.release(self.reserved);
    }
}
