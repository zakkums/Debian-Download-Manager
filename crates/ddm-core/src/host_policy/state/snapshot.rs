//! Serializable snapshot types and conversion for HostPolicy persistence.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::host_policy::entry::{HostEntry, RangeSupport};
use crate::host_policy::key::HostKey;

use super::HostPolicy;

/// Serializable per-host entry (no Instant fields). Used for persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedEntry {
    pub range_support: RangeSupport,
    pub throttled_events: u32,
    pub error_events: u32,
    pub success_events: u32,
    #[serde(default)]
    pub last_throughput_bytes_per_sec: Option<f64>,
    pub adaptive_segment_limit: usize,
}

/// Snapshot of HostPolicy for JSON serialization. Keys are "scheme:host:port" strings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedHostPolicy {
    #[serde(default = "default_version")]
    pub version: u8,
    pub min_segments: usize,
    pub max_segments: usize,
    pub entries: HashMap<String, PersistedEntry>,
}

fn default_version() -> u8 {
    1
}

/// Build a serializable snapshot from the in-memory policy.
pub(super) fn to_snapshot(policy: &HostPolicy) -> PersistedHostPolicy {
    let entries = policy
        .entries
        .iter()
        .map(|(k, e)| {
            (
                k.to_string_key(),
                PersistedEntry {
                    range_support: e.range_support,
                    throttled_events: e.throttled_events,
                    error_events: e.error_events,
                    success_events: e.success_events,
                    last_throughput_bytes_per_sec: e.last_throughput_bytes_per_sec,
                    adaptive_segment_limit: e.adaptive_segment_limit,
                },
            )
        })
        .collect();
    PersistedHostPolicy {
        version: 1,
        min_segments: policy.min_segments,
        max_segments: policy.max_segments,
        entries,
    }
}

/// Restore policy from a persisted snapshot. Bounds (min/max) are applied from
/// config so current config always wins.
pub(super) fn from_snapshot(
    snapshot: PersistedHostPolicy,
    min_segments: usize,
    max_segments: usize,
) -> HostPolicy {
    let min = min_segments.max(1);
    let max = max_segments.max(min);
    let entries = snapshot
        .entries
        .into_iter()
        .filter_map(|(key_str, pe)| {
            let key = HostKey::from_string_key(&key_str)?;
            let entry = HostEntry {
                key: key.clone(),
                range_support: pe.range_support,
                last_throttled_at: None,
                throttled_events: pe.throttled_events,
                last_error_at: None,
                error_events: pe.error_events,
                last_success_at: None,
                success_events: pe.success_events,
                last_throughput_bytes_per_sec: pe.last_throughput_bytes_per_sec,
                adaptive_segment_limit: pe.adaptive_segment_limit.max(min).min(max),
            };
            Some((key, entry))
        })
        .collect();
    HostPolicy {
        entries,
        min_segments: min,
        max_segments: max,
    }
}
