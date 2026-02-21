//! Tests for safe-resume validation.

use crate::fetch_head::HeadResult;
use crate::resume_db::{JobDetails, JobSettings, JobState};

use super::{validate_for_resume, ValidationErrorKind};

fn job_details(
    total_size: Option<i64>,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> JobDetails {
    JobDetails {
        id: 1,
        url: "https://example.com/file.bin".to_string(),
        final_filename: Some("file.bin".to_string()),
        temp_filename: Some("file.bin.part".to_string()),
        total_size,
        etag: etag.map(String::from),
        last_modified: last_modified.map(String::from),
        segment_count: 4,
        completed_bitmap: vec![],
        state: JobState::Paused,
        created_at: 0,
        updated_at: 0,
        settings: JobSettings::default(),
    }
}

fn head_result(
    content_length: Option<u64>,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> HeadResult {
    HeadResult {
        content_length,
        accept_ranges: true,
        etag: etag.map(String::from),
        last_modified: last_modified.map(String::from),
        content_disposition: None,
    }
}

#[test]
fn no_stored_metadata_ok() {
    let job = job_details(None, None, None);
    let head = head_result(
        Some(1000),
        Some("e1"),
        Some("Wed, 21 Oct 2015 07:28:00 GMT"),
    );
    assert!(validate_for_resume(&job, &head).is_ok());
}

#[test]
fn same_etag_and_size_ok() {
    let job = job_details(
        Some(1000),
        Some("e1"),
        Some("Wed, 21 Oct 2015 07:28:00 GMT"),
    );
    let head = head_result(
        Some(1000),
        Some("e1"),
        Some("Wed, 21 Oct 2015 07:28:00 GMT"),
    );
    assert!(validate_for_resume(&job, &head).is_ok());
}

#[test]
fn etag_changed_err() {
    let job = job_details(
        Some(1000),
        Some("e1"),
        Some("Wed, 21 Oct 2015 07:28:00 GMT"),
    );
    let head = head_result(
        Some(1000),
        Some("e2"),
        Some("Wed, 21 Oct 2015 07:28:00 GMT"),
    );
    let r = validate_for_resume(&job, &head);
    assert!(r.is_err());
    let e = r.unwrap_err();
    assert!(matches!(
        e.kind,
        ValidationErrorKind::RemoteChanged {
            etag_changed: true,
            ..
        }
    ));
}

#[test]
fn size_changed_err() {
    let job = job_details(Some(1000), Some("e1"), None);
    let head = head_result(Some(2000), Some("e1"), None);
    let r = validate_for_resume(&job, &head);
    assert!(r.is_err());
    let e = r.unwrap_err();
    assert!(matches!(
        e.kind,
        ValidationErrorKind::RemoteChanged {
            size_changed: true,
            ..
        }
    ));
}

#[test]
fn last_modified_changed_err() {
    let job = job_details(Some(1000), None, Some("Wed, 21 Oct 2015 07:28:00 GMT"));
    let head = head_result(Some(1000), None, Some("Thu, 22 Oct 2015 08:00:00 GMT"));
    let r = validate_for_resume(&job, &head);
    assert!(r.is_err());
    let e = r.unwrap_err();
    assert!(matches!(
        e.kind,
        ValidationErrorKind::RemoteChanged {
            last_modified_changed: true,
            ..
        }
    ));
}
