//! Compares stored job metadata with current HEAD result for safe resume.

use crate::fetch_head::HeadResult;
use crate::resume_db::JobDetails;
use std::fmt;

/// Result of validating that the remote resource is unchanged and safe to resume.
#[derive(Debug)]
pub struct ValidationError {
    pub kind: ValidationErrorKind,
}

#[derive(Debug)]
pub enum ValidationErrorKind {
    /// Remote ETag, Last-Modified, or size changed; user must confirm restart.
    RemoteChanged {
        etag_changed: bool,
        last_modified_changed: bool,
        size_changed: bool,
    },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ValidationErrorKind::RemoteChanged {
                etag_changed,
                last_modified_changed,
                size_changed,
            } => {
                write!(f, "remote resource changed")?;
                let mut first = true;
                if *etag_changed {
                    write!(f, " (ETag)")?;
                    first = false;
                }
                if *last_modified_changed {
                    if first {
                        write!(f, " (Last-Modified)")?;
                    } else {
                        write!(f, ", Last-Modified")?;
                    }
                    first = false;
                }
                if *size_changed {
                    if first {
                        write!(f, " (size)")?;
                    } else {
                        write!(f, ", size")?;
                    }
                }
                write!(
                    f,
                    "; use --force-restart to discard progress and re-download"
                )?;
                Ok(())
            }
        }
    }
}

impl std::error::Error for ValidationError {}

/// Returns Ok(()) if the job can be safely resumed against the current HEAD result.
///
/// If the job has no stored metadata (never probed), returns Ok(()) so the caller
/// can proceed with initial probe and segment planning. Otherwise compares ETag,
/// Last-Modified, and size; returns Err(ValidationError) if any differ.
pub fn validate_for_resume(job: &JobDetails, head: &HeadResult) -> Result<(), ValidationError> {
    let has_stored = job.total_size.is_some() || job.etag.is_some() || job.last_modified.is_some();

    if !has_stored {
        return Ok(());
    }

    let etag_changed = match (&job.etag, &head.etag) {
        (None, None) => false,
        (Some(a), Some(b)) => a != b,
        _ => true,
    };

    let last_modified_changed = match (&job.last_modified, &head.last_modified) {
        (None, None) => false,
        (Some(a), Some(b)) => a != b,
        _ => true,
    };

    let head_size = head.content_length.map(|u| u as i64);
    let size_changed = match (job.total_size, head_size) {
        (None, None) => false,
        (Some(a), Some(b)) => a != b,
        _ => true,
    };

    if etag_changed || last_modified_changed || size_changed {
        return Err(ValidationError {
            kind: ValidationErrorKind::RemoteChanged {
                etag_changed,
                last_modified_changed,
                size_changed,
            },
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resume_db::{JobSettings, JobState};

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
}
