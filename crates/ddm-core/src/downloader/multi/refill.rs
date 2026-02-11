//! Refill helpers for the multi event loop: when to wait for next retry.

use std::time::Instant;

use crate::segmenter::Segment;

/// Returns wait time in ms until the next retry is ready, capped at 100.
pub(super) fn next_retry_wait_ms(retry_after: &[(Instant, usize, Segment, u32)]) -> u64 {
    let now = Instant::now();
    retry_after
        .iter()
        .filter_map(|(t, ..)| t.checked_duration_since(now))
        .min()
        .map(|d| d.as_millis().min(100) as u64)
        .unwrap_or(100)
}
