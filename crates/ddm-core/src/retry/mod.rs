//! Retry and backoff policy.
//!
//! This module encapsulates error classification (timeouts, throttling,
//! connection failures) and exponential backoff decisions so that higher
//! layers (scheduler, downloader) can share a consistent policy.

mod classify;
mod policy;

pub use classify::{
    classify, classify_curl_error, classify_http_status, run_with_retry, SegmentError,
};
pub use policy::{ErrorKind, RetryDecision, RetryPolicy};

