//! Classify HTTP status and curl errors into retry policy error kinds.

use super::error::SegmentError;
use super::policy::ErrorKind;

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
    if e.is_write_error() {
        return ErrorKind::Connection;
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

/// Classify a segment error (curl, HTTP, or storage) into an ErrorKind.
pub fn classify(e: &SegmentError) -> ErrorKind {
    match e {
        SegmentError::Curl(ce) => classify_curl_error(ce),
        SegmentError::Http(code) => classify_http_status(*code),
        SegmentError::PartialTransfer { .. } => ErrorKind::Connection,
        SegmentError::Storage(_) => ErrorKind::Other,
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

    #[test]
    fn partial_transfer_classified_as_connection() {
        let e = SegmentError::PartialTransfer {
            expected: 100,
            received: 50,
        };
        assert_eq!(classify(&e), ErrorKind::Connection);
    }

    #[test]
    fn storage_classified_as_other() {
        let e = SegmentError::Storage(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "read-only filesystem",
        ));
        assert_eq!(classify(&e), ErrorKind::Other);
    }
}
