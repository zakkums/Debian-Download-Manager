//! Segment count selection (adaptive and config caps).

use crate::config::DdmConfig;
use crate::host_policy::HostPolicy;

/// Chooses segment count: adaptive (4/8/16) capped by host policy and config.
pub(crate) fn choose_segment_count(
    total_size: u64,
    cfg: &DdmConfig,
    url: &str,
    host_policy: &HostPolicy,
) -> usize {
    let adaptive = host_policy
        .adaptive_segment_count_for_url(url)
        .unwrap_or_else(|_| cfg.min_segments.max(1).min(cfg.max_segments));
    let n = adaptive
        .max(cfg.min_segments)
        .min(cfg.max_segments)
        .max(1);
    if total_size == 0 {
        return n;
    }
    n.min(total_size as usize)
}
