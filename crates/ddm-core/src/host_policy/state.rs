//! In-memory host policy cache and adaptive segment logic.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::fetch_head::HeadResult;

use super::entry::{HostEntry, RangeSupport};
use super::HostKey;

/// Minimum bytes/sec to consider throughput "good" for stepping up segment count (4 -> 8 -> 16).
const THROUGHPUT_GOOD_BPS: f64 = 1_000_000.0; // 1 MiB/s

/// In-memory cache of per-host policy information.
///
/// The cache is intentionally small and process-local. It is created by the
/// CLI `run` loop and passed into the scheduler so that multiple jobs within a
/// single invocation can share observations about each host.
#[derive(Debug)]
pub struct HostPolicy {
    entries: HashMap<HostKey, HostEntry>,
    min_segments: usize,
    max_segments: usize,
}

impl HostPolicy {
    /// Create a new cache with the given global segment bounds.
    ///
    /// These bounds usually come from `DdmConfig` and are used as the base
    /// when recommending per-host segment limits.
    pub fn new(min_segments: usize, max_segments: usize) -> Self {
        let min = min_segments.max(1);
        let max = max_segments.max(min);
        Self {
            entries: HashMap::new(),
            min_segments: min,
            max_segments: max,
        }
    }

    /// Look up an entry for the given key, if present.
    pub fn get(&self, key: &HostKey) -> Option<&HostEntry> {
        self.entries.get(key)
    }

    /// Default adaptive segment count for a new host (start at 4 per spec).
    fn default_adaptive_limit(&self) -> usize {
        (4_usize).max(self.min_segments).min(self.max_segments)
    }

    fn entry_mut_for_url(&mut self, url: &str) -> Result<&mut HostEntry> {
        let key = HostKey::from_url(url)?;
        let default = self.default_adaptive_limit();
        Ok(self
            .entries
            .entry(key.clone())
            .or_insert_with(|| HostEntry::new(key, default)))
    }

    /// Record the outcome of a HEAD probe for the given URL.
    ///
    /// This currently only updates range support but can be extended later
    /// with additional metadata.
    pub fn record_head_result(&mut self, url: &str, head: &HeadResult) -> Result<()> {
        let entry = self.entry_mut_for_url(url)?;
        entry.range_support = if head.accept_ranges {
            RangeSupport::Supported
        } else {
            RangeSupport::NotSupported
        };
        Ok(())
    }

    /// Record that the host signalled throttling (e.g. HTTP 429 / 503).
    pub fn record_throttled(&mut self, url: &str) -> Result<()> {
        let entry = self.entry_mut_for_url(url)?;
        entry.throttled_events = entry.throttled_events.saturating_add(1);
        entry.last_throttled_at = Some(Instant::now());
        Ok(())
    }

    /// Record a generic error for the host (connection failures, 5xx, etc.).
    pub fn record_error(&mut self, url: &str) -> Result<()> {
        let entry = self.entry_mut_for_url(url)?;
        entry.error_events = entry.error_events.saturating_add(1);
        entry.last_error_at = Some(Instant::now());
        Ok(())
    }

    /// Record a successful operation for the host (e.g. completed segment).
    pub fn record_success(&mut self, url: &str) -> Result<()> {
        let entry = self.entry_mut_for_url(url)?;
        entry.success_events = entry.success_events.saturating_add(1);
        entry.last_success_at = Some(Instant::now());
        Ok(())
    }

    /// Compute the recommended maximum number of segments for a host,
    /// identified by URL.
    pub fn recommended_max_segments_for_url(&self, url: &str) -> Result<usize> {
        let key = HostKey::from_url(url)?;
        Ok(self.recommended_max_segments(&key))
    }

    /// Compute the recommended maximum number of segments for a host key.
    ///
    /// For now this is a conservative heuristic:
    /// - start from the configured global `max_segments`
    /// - apply a few halving steps when we have seen many throttling events
    /// - never go below `min_segments`
    pub fn recommended_max_segments(&self, key: &HostKey) -> usize {
        let base = self.max_segments.max(self.min_segments).max(1);
        let Some(entry) = self.entries.get(key) else {
            return base;
        };

        // Each group of three throttling events halves the recommendation,
        // capped to a small number of steps so we never shrink to zero.
        let penalty_steps = (entry.throttled_events / 3).min(3);
        let mut recommended = base;
        for _ in 0..penalty_steps {
            recommended = (recommended / 2).max(self.min_segments.max(1));
        }
        recommended
    }

    /// Record the outcome of a completed (or failed) download run for adaptive tuning.
    ///
    /// Updates throughput and steps the adaptive segment limit: up (4 -> 8 -> 16) when
    /// throughput is good and there were no throttle/error events; down when there were.
    pub fn record_job_outcome(
        &mut self,
        url: &str,
        _segment_count_used: usize,
        bytes_downloaded: u64,
        duration: Duration,
        throttle_events: u32,
        error_events: u32,
    ) -> Result<()> {
        let key = HostKey::from_url(url)?;
        let min_seg = self.min_segments.max(1);
        let max_seg = self.max_segments;
        let cap = self.recommended_max_segments(&key);

        let entry = self.entry_mut_for_url(url)?;
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
            entry.adaptive_segment_limit =
                (entry.adaptive_segment_limit / 2).max(min_seg).min(max_seg);
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

    /// Adaptive segment count for the next job to this host: start at 4, up to 8/16 on good throughput.
    /// Capped by recommended_max_segments and config bounds.
    pub fn adaptive_segment_count_for_url(&self, url: &str) -> Result<usize> {
        let key = HostKey::from_url(url)?;
        Ok(self.adaptive_segment_count(&key))
    }

    /// Adaptive segment count for a host key.
    pub fn adaptive_segment_count(&self, key: &HostKey) -> usize {
        let cap = self.recommended_max_segments(key);
        let Some(entry) = self.entries.get(key) else {
            return self.default_adaptive_limit().min(cap);
        };
        entry.adaptive_segment_limit.min(cap).max(self.min_segments).min(self.max_segments)
    }
}
