//! Retry loop: run a closure until success or policy says stop.

use super::classify;
use super::error::SegmentError;
use super::policy::{RetryDecision, RetryPolicy};

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
                let kind = classify::classify(&e);
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
