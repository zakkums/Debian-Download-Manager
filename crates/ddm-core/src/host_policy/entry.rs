//! Per-host entry and range support types.

use std::time::Instant;

use super::HostKey;

/// Observed range support for a given host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeSupport {
    /// No information yet.
    Unknown,
    /// Server has advertised `Accept-Ranges: bytes`.
    Supported,
    /// Server has explicitly indicated that ranges are not supported.
    NotSupported,
}

impl Default for RangeSupport {
    fn default() -> Self {
        RangeSupport::Unknown
    }
}

/// Per-host statistics and observations.
#[derive(Debug, Clone)]
pub struct HostEntry {
    pub key: HostKey,
    pub range_support: RangeSupport,
    pub last_throttled_at: Option<Instant>,
    pub throttled_events: u32,
    pub last_error_at: Option<Instant>,
    pub error_events: u32,
    pub last_success_at: Option<Instant>,
    pub success_events: u32,
    /// Last observed throughput (bytes/sec) for adaptive stepping.
    pub last_throughput_bytes_per_sec: Option<f64>,
    /// Adaptive segment limit: start at 4, step up to 8/16 on good throughput, down on throttle/error.
    pub adaptive_segment_limit: usize,
}

impl HostEntry {
    pub(super) fn new(key: HostKey, default_adaptive_limit: usize) -> Self {
        Self {
            key,
            range_support: RangeSupport::Unknown,
            last_throttled_at: None,
            throttled_events: 0,
            last_error_at: None,
            error_events: 0,
            last_success_at: None,
            success_events: 0,
            last_throughput_bytes_per_sec: None,
            adaptive_segment_limit: default_adaptive_limit,
        }
    }
}
