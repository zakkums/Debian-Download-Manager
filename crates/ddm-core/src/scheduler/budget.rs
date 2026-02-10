//! Global connection budget shared across jobs.
//!
//! When multiple jobs run (e.g. in a future parallel scheduler), each job
//! reserves connections from this budget so total concurrency stays under
//! `max_total_connections`.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Shared global connection budget. Jobs reserve slots before starting
/// segment downloads and release them when done so multiple jobs don't
/// each use full per-host concurrency.
#[derive(Debug)]
pub struct GlobalConnectionBudget {
    max_total: usize,
    in_use: AtomicUsize,
}

impl GlobalConnectionBudget {
    /// Create a budget with the given maximum total connections (e.g. from config).
    pub fn new(max_total: usize) -> Self {
        Self {
            max_total: max_total.max(1),
            in_use: AtomicUsize::new(0),
        }
    }

    /// Number of connections currently reserved.
    pub fn in_use(&self) -> usize {
        self.in_use.load(Ordering::Relaxed)
    }

    /// Available slots (max_total - in_use). May be 0 if other jobs hold the budget.
    pub fn available(&self) -> usize {
        let used = self.in_use.load(Ordering::Relaxed);
        self.max_total.saturating_sub(used)
    }

    /// Reserve up to `requested` connections. Returns the number actually reserved
    /// (min(requested, available)). Caller must call `release` with that number when done.
    pub fn reserve(&self, requested: usize) -> usize {
        let mut current = self.in_use.load(Ordering::Relaxed);
        loop {
            let available = self.max_total.saturating_sub(current);
            let take = requested.min(available).min(self.max_total);
            match self.in_use.compare_exchange_weak(
                current,
                current + take,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => return take,
                Err(actual) => current = actual,
            }
        }
    }

    /// Release `n` connections back to the budget. Call with the value returned from `reserve`.
    pub fn release(&self, n: usize) {
        self.in_use.fetch_sub(n.min(self.in_use.load(Ordering::Relaxed)), Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_reserve_and_release() {
        let budget = GlobalConnectionBudget::new(16);
        assert_eq!(budget.available(), 16);
        assert_eq!(budget.reserve(8), 8);
        assert_eq!(budget.in_use(), 8);
        assert_eq!(budget.available(), 8);
        assert_eq!(budget.reserve(10), 8);
        assert_eq!(budget.in_use(), 16);
        assert_eq!(budget.available(), 0);
        assert_eq!(budget.reserve(1), 0);
        budget.release(8);
        assert_eq!(budget.available(), 8);
        budget.release(8);
        assert_eq!(budget.in_use(), 0);
        assert_eq!(budget.available(), 16);
    }
}
