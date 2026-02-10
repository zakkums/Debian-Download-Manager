//! Segment download error type for retry classification.

use std::fmt;

/// Error returned by a single segment download (curl failure, HTTP error, or storage failure).
/// Used so we can classify and decide retries before converting to anyhow.
#[derive(Debug)]
pub enum SegmentError {
    /// Curl reported an error (timeout, connection, etc.).
    Curl(curl::Error),
    /// HTTP response had a non-2xx status.
    Http(u32),
    /// We sent a Range request but the server did not respond with 206 Partial Content
    /// (e.g. 200 with full body). Prevents writing full response into segment window and corrupting the file.
    InvalidRangeResponse(u32),
    /// Transfer completed but fewer bytes were written than the segment length
    /// (e.g. server closed early). Enables retry instead of silent corruption.
    PartialTransfer { expected: u64, received: u64 },
    /// Disk/storage write failed (e.g. disk full, permission denied). Not retried.
    Storage(std::io::Error),
}

impl fmt::Display for SegmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SegmentError::Curl(e) => write!(f, "{}", e),
            SegmentError::Http(code) => write!(f, "HTTP {}", code),
            SegmentError::InvalidRangeResponse(code) => {
                write!(f, "range request got HTTP {} instead of 206 Partial Content", code)
            }
            SegmentError::PartialTransfer { expected, received } => {
                write!(f, "partial transfer: expected {} bytes, got {}", expected, received)
            }
            SegmentError::Storage(e) => write!(f, "storage: {}", e),
        }
    }
}

impl std::error::Error for SegmentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SegmentError::Curl(e) => Some(e),
            SegmentError::Storage(e) => Some(e),
            SegmentError::Http(_)
            | SegmentError::InvalidRangeResponse(_)
            | SegmentError::PartialTransfer { .. } => None,
        }
    }
}
