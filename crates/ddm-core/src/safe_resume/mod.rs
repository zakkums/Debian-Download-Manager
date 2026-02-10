//! Safe resume: re-validate ETag/Last-Modified/size before resuming.
//!
//! On start, the scheduler probes the URL and compares the result with stored
//! job metadata. If anything changed, the caller must require an explicit user
//! override (e.g. `--force-restart`) before discarding progress and re-downloading.

mod validate;

pub use validate::{validate_for_resume, ValidationError, ValidationErrorKind};
