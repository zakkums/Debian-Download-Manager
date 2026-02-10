use std::time::Duration;

/// High-level classification of an error for retry purposes.
///
/// This intentionally stays generic; callers can map HTTP status codes,
/// curl errors, or IO failures into these kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Operation timed out (connect/read).
    Timeout,
    /// Server asked us to slow down (e.g. 429, 503).
    Throttled,
    /// Network-level failure (connection reset, DNS, etc.).
    Connection,
    /// HTTP status that is retryable but not strictly throttling (5xx).
    Http5xx(u16),
    /// Any other error (typically not retried).
    Other,
}

/// Decision returned by the retry policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    /// Do not retry this error.
    NoRetry,
    /// Retry after the given delay.
    RetryAfter(Duration),
}

/// Simple exponential backoff policy with caps.
///
/// For now this is hard-coded; later it can be made configurable via
/// `DdmConfig` once we extend the config schema.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    /// Maximum number of attempts (including the first).
    pub max_attempts: u32,
    /// Base delay for backoff.
    pub base_delay: Duration,
    /// Upper bound on backoff delay.
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            base_delay: Duration::from_millis(250),
            max_delay: Duration::from_secs(30),
        }
    }
}

impl RetryPolicy {
    /// Compute the next backoff delay for a given attempt and error kind.
    ///
    /// `attempt` is 1-based (1 = first attempt). Returns `RetryDecision::NoRetry`
    /// when we should stop retrying.
    pub fn decide(&self, attempt: u32, kind: ErrorKind) -> RetryDecision {
        if attempt >= self.max_attempts {
            return RetryDecision::NoRetry;
        }

        match kind {
            ErrorKind::Other => RetryDecision::NoRetry,
            ErrorKind::Timeout
            | ErrorKind::Connection
            | ErrorKind::Throttled
            | ErrorKind::Http5xx(_) => {
                // Simple exponential backoff: base * 2^(attempt-1), capped.
                let exp = 1u32.saturating_mul(1 << attempt.saturating_sub(1).min(8));
                let raw = self.base_delay.saturating_mul(exp);
                let delay = raw.min(self.max_delay);
                RetryDecision::RetryAfter(delay)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_retry_for_other() {
        let p = RetryPolicy::default();
        assert_eq!(p.decide(1, ErrorKind::Other), RetryDecision::NoRetry);
    }

    #[test]
    fn exponential_backoff_grows_and_is_capped() {
        let mut p = RetryPolicy::default();
        // Allow many attempts so we can observe capping behaviour.
        p.max_attempts = 20;
        let d1 = match p.decide(1, ErrorKind::Timeout) {
            RetryDecision::RetryAfter(d) => d,
            _ => panic!("expected retry"),
        };
        let d2 = match p.decide(2, ErrorKind::Timeout) {
            RetryDecision::RetryAfter(d) => d,
            _ => panic!("expected retry"),
        };
        assert!(d2 >= d1);

        // Very high attempt should cap at max_delay
        let d_last = match p.decide(10, ErrorKind::Timeout) {
            RetryDecision::RetryAfter(d) => d,
            _ => panic!("expected retry"),
        };
        assert!(d_last <= p.max_delay);
    }

    #[test]
    fn respects_max_attempts() {
        let mut p = RetryPolicy::default();
        p.max_attempts = 3;
        assert!(matches!(
            p.decide(1, ErrorKind::Throttled),
            RetryDecision::RetryAfter(_)
        ));
        assert!(matches!(
            p.decide(2, ErrorKind::Throttled),
            RetryDecision::RetryAfter(_)
        ));
        assert_eq!(
            p.decide(3, ErrorKind::Throttled),
            RetryDecision::NoRetry
        );
    }
}

