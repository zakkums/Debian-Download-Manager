//! Per-host policy cache.
//!
//! This module tracks simple, in-memory state per `(scheme, host, port)`:
//! - observed range support (from HEAD responses)
//! - throttling / error / success counters
//! - a recommended maximum segment count for that host
//!
//! The cache is intentionally lightweight and process-local; it is created by
//! the CLI `run` loop and passed to the scheduler so multiple jobs in a single
//! invocation can share observations.

mod entry;
mod key;
mod state;

pub use entry::{HostEntry, RangeSupport};
pub use key::HostKey;
pub use state::HostPolicy;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch_head::HeadResult;

    fn make_head(accept_ranges: bool) -> HeadResult {
        HeadResult {
            content_length: Some(1024),
            accept_ranges,
            etag: Some("etag-1".to_string()),
            last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".to_string()),
            content_disposition: None,
        }
    }

    #[test]
    fn host_key_from_url_parses_scheme_host_port() {
        let key = HostKey::from_url("https://example.com:8443/path").unwrap();
        assert_eq!(key.scheme, "https");
        assert_eq!(key.host, "example.com");
        assert_eq!(key.port, 8443);
    }

    #[test]
    fn host_key_uses_default_port_when_missing() {
        let key = HostKey::from_url("http://example.com/path").unwrap();
        assert_eq!(key.scheme, "http");
        assert_eq!(key.host, "example.com");
        // HTTP default port
        assert_eq!(key.port, 80);
    }

    #[test]
    fn record_head_result_updates_range_support() {
        let mut policy = HostPolicy::new(4, 16);
        let url = "https://cdn.example.com/file.iso";

        let head = make_head(true);
        policy.record_head_result(url, &head).unwrap();

        let key = HostKey::from_url(url).unwrap();
        let entry = policy.get(&key).expect("entry should exist");
        assert_eq!(entry.range_support, RangeSupport::Supported);

        let head2 = make_head(false);
        policy.record_head_result(url, &head2).unwrap();
        let entry2 = policy.get(&key).expect("entry should still exist");
        assert_eq!(entry2.range_support, RangeSupport::NotSupported);
    }

    #[test]
    fn recommended_segments_respects_bounds_and_throttling() {
        let mut policy = HostPolicy::new(2, 16);
        let url = "https://slow.example.com/file";

        // With no history we should get the max bound.
        let base = policy
            .recommended_max_segments_for_url(url)
            .expect("url parses");
        assert_eq!(base, 16);

        // Simulate repeated throttling; recommendation should go down but
        // never below the configured minimum.
        for _ in 0..6 {
            policy.record_throttled(url).expect("url parses");
        }
        let reduced = policy
            .recommended_max_segments_for_url(url)
            .expect("url parses");
        assert!(reduced < 16);
        assert!(reduced >= 2);
    }

    #[test]
    fn adaptive_segment_count_starts_at_four_and_steps_up_on_good_throughput() {
        use std::time::Duration;

        let mut policy = HostPolicy::new(2, 16);
        let url = "https://fast.example.com/file";

        // New host: adaptive count is 4 (default).
        let n = policy.adaptive_segment_count_for_url(url).unwrap();
        assert_eq!(n, 4, "new host should start at 4 segments");

        // Good throughput, no throttle/error: should step 4 -> 8.
        policy
            .record_job_outcome(
                url,
                4,
                10_000_000, // 10 MiB
                Duration::from_secs(5), // 2 MiB/s > 1 MiB/s threshold
                0,
                0,
            )
            .unwrap();
        let n = policy.adaptive_segment_count_for_url(url).unwrap();
        assert_eq!(n, 8, "after good run should step to 8");

        // Another good run: 8 -> 16.
        policy
            .record_job_outcome(url, 8, 20_000_000, Duration::from_secs(5), 0, 0)
            .unwrap();
        let n = policy.adaptive_segment_count_for_url(url).unwrap();
        assert_eq!(n, 16, "after second good run should step to 16");
    }

    #[test]
    fn adaptive_segment_count_steps_down_on_throttle() {
        use std::time::Duration;

        let mut policy = HostPolicy::new(2, 16);
        let url = "https://throttled.example.com/file";
        // One good run: 4 -> 8.
        policy
            .record_job_outcome(url, 4, 10_000_000, Duration::from_secs(5), 0, 0)
            .unwrap();
        let n = policy.adaptive_segment_count_for_url(url).unwrap();
        assert_eq!(n, 8, "should be at 8 after one good run");

        // Simulate throttle: 8 -> 4.
        policy
            .record_job_outcome(url, 8, 1000, Duration::from_secs(1), 1, 0)
            .unwrap();
        let n = policy.adaptive_segment_count_for_url(url).unwrap();
        assert!(n < 8);
        assert!(n >= 2);
    }
}

