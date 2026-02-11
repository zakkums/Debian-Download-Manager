//! Tests for HostPolicy state and persistence.

use tempfile::NamedTempFile;

use crate::fetch_head::HeadResult;

use super::super::entry::RangeSupport;
use super::super::HostKey;
use super::HostPolicy;

#[test]
fn to_snapshot_roundtrip() {
    let mut policy = HostPolicy::new(2, 16);
    let _ = policy.record_head_result(
        "https://example.com/file",
        &HeadResult {
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
        &HeadResult {
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
