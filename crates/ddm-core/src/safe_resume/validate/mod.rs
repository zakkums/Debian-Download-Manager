//! Compares stored job metadata with current HEAD result for safe resume.

mod error;

use crate::fetch_head::HeadResult;
use crate::resume_db::JobDetails;

pub use error::{ValidationError, ValidationErrorKind};

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
mod tests;
