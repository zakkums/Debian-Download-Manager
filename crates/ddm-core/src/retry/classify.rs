//! Classify HTTP status and curl errors into retry policy error kinds.

use crate::retry::policy::{ErrorKind, RetryDecision, RetryPolicy};
use std::fmt;

/// Runs a closure until it succeeds or the retry policy says to stop.
/// On retryable failure, sleeps for the backoff duration then tries again.
pub fn run_with_retry<F>(policy: &RetryPolicy, mut f: F) -> Result<(), SegmentError>
where
    F: FnMut() -> Result<(), SegmentError>,
{
    let mut attempt = 1u32;
    loop {
        match f() {
            Ok(()) => return Ok(()),
            Err(e) => {
                let kind = classify(&e);
                match policy.decide(attempt, kind) {
                    RetryDecision::NoRetry => return Err(e),
                    RetryDecision::RetryAfter(d) => {
                        std::thread::sleep(d);
                        attempt += 1;
                    }
                }
            }
        }
    }
}

/// Error returned by a single segment download (curl failure or HTTP error).
/// Used so we can classify and decide retries before converting to anyhow.
#[derive(Debug)]
pub enum SegmentError {
    /// Curl reported an error (timeout, connection, etc.).
    Curl(curl::Error),
    /// HTTP response had a non-2xx status.
    Http(u32),
}

impl fmt::Display for SegmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SegmentError::Curl(e) => write!(f, "{}", e),
            SegmentError::Http(code) => write!(f, "HTTP {}", code),
        }
    }
}

impl std::error::Error for SegmentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SegmentError::Curl(e) => Some(e),
            SegmentError::Http(_) => None,
        }
    }
}

/// Classify an HTTP status code for retry decisions.
pub fn classify_http_status(code: u32) -> ErrorKind {
    match code {
        429 | 503 => ErrorKind::Throttled,
        500..=599 => ErrorKind::Http5xx(code as u16),
        _ => ErrorKind::Other,
    }
}

/// Classify a curl error for retry decisions.
pub fn classify_curl_error(e: &curl::Error) -> ErrorKind {
    if e.is_operation_timedout() {
        return ErrorKind::Timeout;
    }
    if e.is_couldnt_connect()
        || e.is_couldnt_resolve_host()
        || e.is_couldnt_resolve_proxy()
        || e.is_read_error()
        || e.is_recv_error()
        || e.is_send_error()
        || e.is_got_nothing()
    {
        return ErrorKind::Connection;
    }
    ErrorKind::Other
}

/// Classify a segment error (curl or HTTP) into an ErrorKind.
pub fn classify(e: &SegmentError) -> ErrorKind {
    match e {
        SegmentError::Curl(ce) => classify_curl_error(ce),
        SegmentError::Http(code) => classify_http_status(*code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_429_and_503_throttled() {
        assert_eq!(classify_http_status(429), ErrorKind::Throttled);
        assert_eq!(classify_http_status(503), ErrorKind::Throttled);
    }

    #[test]
    fn http_5xx_retryable() {
        assert!(matches!(classify_http_status(500), ErrorKind::Http5xx(500)));
        assert!(matches!(classify_http_status(502), ErrorKind::Http5xx(502)));
    }

    #[test]
    fn http_4xx_other() {
        assert_eq!(classify_http_status(404), ErrorKind::Other);
        assert_eq!(classify_http_status(403), ErrorKind::Other);
    }
}
