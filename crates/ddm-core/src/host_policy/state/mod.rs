//! In-memory host policy cache and adaptive segment logic.

mod adaptive;
mod snapshot;

use std::collections::HashMap;

use anyhow::Result;

use crate::fetch_head::HeadResult;

use super::entry::{HostEntry, RangeSupport};
use super::HostKey;
use adaptive::{
    adaptive_segment_count, default_adaptive_limit, record_job_outcome, recommended_max_segments,
};

pub use snapshot::PersistedHostPolicy;

/// In-memory cache of per-host policy information.
///
/// The cache is intentionally small and process-local. It is created by the
/// CLI `run` loop and passed into the scheduler so that multiple jobs within a
/// single invocation can share observations about each host.
#[derive(Debug)]
pub struct HostPolicy {
    pub(super) entries: HashMap<HostKey, HostEntry>,
    pub(super) min_segments: usize,
    pub(super) max_segments: usize,
}

impl HostPolicy {
    /// Create a new cache with the given global segment bounds.
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

    pub(super) fn entry_mut_for_url(&mut self, url: &str) -> Result<&mut HostEntry> {
        let key = HostKey::from_url(url)?;
        let default = default_adaptive_limit(self);
        Ok(self
            .entries
            .entry(key.clone())
            .or_insert_with(|| HostEntry::new(key, default)))
    }

    /// Record the outcome of a HEAD probe for the given URL.
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
        entry.last_throttled_at = Some(std::time::Instant::now());
        Ok(())
    }

    /// Record a generic error for the host (connection failures, 5xx, etc.).
    pub fn record_error(&mut self, url: &str) -> Result<()> {
        let entry = self.entry_mut_for_url(url)?;
        entry.error_events = entry.error_events.saturating_add(1);
        entry.last_error_at = Some(std::time::Instant::now());
        Ok(())
    }

    /// Record a successful operation for the host (e.g. completed segment).
    pub fn record_success(&mut self, url: &str) -> Result<()> {
        let entry = self.entry_mut_for_url(url)?;
        entry.success_events = entry.success_events.saturating_add(1);
        entry.last_success_at = Some(std::time::Instant::now());
        Ok(())
    }

    /// Compute the recommended maximum number of segments for a host, by URL.
    pub fn recommended_max_segments_for_url(&self, url: &str) -> Result<usize> {
        let key = HostKey::from_url(url)?;
        Ok(recommended_max_segments(self, &key))
    }

    /// Compute the recommended maximum number of segments for a host key.
    pub fn recommended_max_segments(&self, key: &HostKey) -> usize {
        recommended_max_segments(self, key)
    }

    /// Record the outcome of a completed (or failed) download run for adaptive tuning.
    pub fn record_job_outcome(
        &mut self,
        url: &str,
        segment_count_used: usize,
        bytes_downloaded: u64,
        duration: std::time::Duration,
        throttle_events: u32,
        error_events: u32,
    ) -> Result<()> {
        record_job_outcome(
            self,
            url,
            segment_count_used,
            bytes_downloaded,
            duration,
            throttle_events,
            error_events,
        )
    }

    /// Adaptive segment count for the next job to this host (by URL).
    pub fn adaptive_segment_count_for_url(&self, url: &str) -> Result<usize> {
        let key = HostKey::from_url(url)?;
        Ok(adaptive_segment_count(self, &key))
    }

    /// Adaptive segment count for a host key.
    pub fn adaptive_segment_count(&self, key: &HostKey) -> usize {
        adaptive_segment_count(self, key)
    }

    /// Build a serializable snapshot for persistence.
    pub fn to_snapshot(&self) -> PersistedHostPolicy {
        snapshot::to_snapshot(self)
    }

    /// Restore policy from a persisted snapshot. Bounds (min/max) from config.
    pub fn from_snapshot(
        snapshot: PersistedHostPolicy,
        min_segments: usize,
        max_segments: usize,
    ) -> Self {
        snapshot::from_snapshot(snapshot, min_segments, max_segments)
    }
}

#[cfg(test)]
mod persist_tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn to_snapshot_roundtrip() {
        let mut policy = HostPolicy::new(2, 16);
        let _ = policy.record_head_result(
            "https://example.com/file",
            &crate::fetch_head::HeadResult {
                content_length: Some(1000),
                accept_ranges: true,
                etag: None,
                last_modified: None,
                content_disposition: None,
            },
        );
        let snapshot = policy.to_snapshot();
        assert_eq!(snapshot.version, 1);
        assert_eq!(snapshot.min_segments, 2);
        assert_eq!(snapshot.max_segments, 16);
        assert_eq!(snapshot.entries.len(), 1);
        let restored = HostPolicy::from_snapshot(snapshot, 2, 16);
        let key = HostKey::from_url("https://example.com/").unwrap();
        assert!(restored.get(&key).is_some());
        assert_eq!(
            restored.get(&key).unwrap().range_support,
            RangeSupport::Supported
        );
    }

    #[test]
    fn save_and_load_from_path() {
        let mut policy = HostPolicy::new(4, 16);
        let _ = policy.record_head_result(
            "https://cdn.test/saved",
            &crate::fetch_head::HeadResult {
                content_length: Some(2000),
                accept_ranges: true,
                etag: None,
                last_modified: None,
                content_disposition: None,
            },
        );
        let f = NamedTempFile::new().unwrap();
        let path = f.path();
        policy.save_to_path(path).unwrap();
        let loaded = HostPolicy::load_from_path(path, 4, 16).unwrap().unwrap();
        let key = HostKey::from_url("https://cdn.test/").unwrap();
        assert!(loaded.get(&key).is_some());
    }
}
