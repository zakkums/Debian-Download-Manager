//! Adaptive segment count and recommended max segments logic.

use std::time::{Duration, Instant};

use anyhow::Result;

use super::HostPolicy;
use crate::host_policy::HostKey;

/// Minimum bytes/sec to consider throughput "good" for stepping up segment count (4 -> 8 -> 16).
const THROUGHPUT_GOOD_BPS: f64 = 1_000_000.0; // 1 MiB/s

/// Default adaptive segment count for a new host (start at 4 per spec).
pub(super) fn default_adaptive_limit(policy: &HostPolicy) -> usize {
    (4_usize).max(policy.min_segments).min(policy.max_segments)
}

/// Compute the recommended maximum number of segments for a host key.
///
/// Conservative heuristic: start from global max, halve for each group of three
/// throttling events, never below min_segments.
pub(super) fn recommended_max_segments(policy: &HostPolicy, key: &HostKey) -> usize {
    let base = policy.max_segments.max(policy.min_segments).max(1);
    let Some(entry) = policy.entries.get(key) else {
        return base;
    };
    let penalty_steps = (entry.throttled_events / 3).min(3);
    let mut recommended = base;
    for _ in 0..penalty_steps {
        recommended = (recommended / 2).max(policy.min_segments.max(1));
    }
    recommended
}

/// Record the outcome of a completed (or failed) download run for adaptive tuning.
pub(super) fn record_job_outcome(
    policy: &mut HostPolicy,
    url: &str,
    _segment_count_used: usize,
    bytes_downloaded: u64,
    duration: Duration,
    throttle_events: u32,
    error_events: u32,
) -> Result<()> {
    let key = HostKey::from_url(url)?;
    let min_seg = policy.min_segments.max(1);
    let max_seg = policy.max_segments;
    let cap = recommended_max_segments(policy, &key);

    let entry = policy.entry_mut_for_url(url)?;
    let bps = if duration.as_secs_f64() > 0.0 {
        bytes_downloaded as f64 / duration.as_secs_f64()
    } else {
        0.0
    };
    entry.last_throughput_bytes_per_sec = Some(bps);

    if throttle_events > 0 {
        entry.throttled_events = entry.throttled_events.saturating_add(throttle_events);
        entry.last_throttled_at = Some(Instant::now());
    }
    if error_events > 0 {
        entry.error_events = entry.error_events.saturating_add(error_events);
        entry.last_error_at = Some(Instant::now());
    }

    if throttle_events > 0 || error_events > 0 {
        entry.adaptive_segment_limit = (entry.adaptive_segment_limit / 2).max(min_seg).min(max_seg);
    } else if bps >= THROUGHPUT_GOOD_BPS {
        let next = match entry.adaptive_segment_limit {
            n if n < 8 => 8,
            n if n < 16 => 16,
            _ => max_seg.min(16),
        };
        entry.adaptive_segment_limit = next.min(cap);
    }
    Ok(())
}

/// Adaptive segment count for a host key.
pub(super) fn adaptive_segment_count(policy: &HostPolicy, key: &HostKey) -> usize {
    let cap = recommended_max_segments(policy, key);
    let Some(entry) = policy.entries.get(key) else {
        return default_adaptive_limit(policy).min(cap);
    };
    entry
        .adaptive_segment_limit
        .min(cap)
        .max(policy.min_segments)
        .min(policy.max_segments)
}
