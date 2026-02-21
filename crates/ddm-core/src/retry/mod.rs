//! Retry and backoff policy.
//!
//! This module encapsulates error classification (timeouts, throttling,
//! connection failures) and exponential backoff decisions so that higher
//! layers (scheduler, downloader) can share a consistent policy.

mod classify;
mod error;
mod policy;
mod run;

pub use classify::{classify, classify_curl_error, classify_http_status};
pub use error::SegmentError;
pub use policy::{ErrorKind, RetryDecision, RetryPolicy};
pub use run::run_with_retry;
