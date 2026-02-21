//! Error types for safe-resume validation.

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
