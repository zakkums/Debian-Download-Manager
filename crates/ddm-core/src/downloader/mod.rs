//! Core segmented downloader engine.
//!
//! Consumes direct URL + headers, runs N concurrent HTTP Range GETs (bounded by
//! `max_concurrent` when set), writes each segment to storage at the correct
//! offset and updates the completion bitmap. Supports retry with backoff via
//! optional `RetryPolicy`.

mod run;
mod segment;

use anyhow::Result;
use crate::retry::{RetryPolicy, SegmentError};
use crate::segmenter::{Segment, SegmentBitmap};
use crate::storage::StorageWriter;
use std::collections::HashMap;

/// Result of a single segment download (used for retry classification).
pub type SegmentResult = Result<(), SegmentError>;

/// Summary of a download run for adaptive policy: throttle and error counts.
#[derive(Debug, Clone, Default)]
pub struct DownloadSummary {
    pub throttle_events: u32,
    pub error_events: u32,
}

/// Downloads all segments that are not yet completed, writing to `storage` and updating `bitmap`.
/// When `max_concurrent` is `Some(n)`, at most `n` segment downloads run at once. When `None`,
/// one thread per incomplete segment (unbounded). Fills `summary_out` with throttle/error counts.
/// If `progress_tx` is `Some`, the current bitmap is sent after each completed segment
/// (coalesced every N completions) so the caller can persist progress.
pub fn download_segments(
    url: &str,
    custom_headers: &HashMap<String, String>,
    segments: &[Segment],
    storage: &StorageWriter,
    bitmap: &mut SegmentBitmap,
    max_concurrent: Option<usize>,
    retry_policy: Option<&RetryPolicy>,
    summary_out: &mut DownloadSummary,
    progress_tx: Option<&tokio::sync::mpsc::Sender<Vec<u8>>>,
) -> Result<()> {
    let incomplete: Vec<(usize, Segment)> = segments
        .iter()
        .enumerate()
        .filter(|(i, _)| !bitmap.is_completed(*i))
        .map(|(i, s)| (i, *s))
        .collect();

    if incomplete.is_empty() {
        return Ok(());
    }
    *summary_out = DownloadSummary::default();

    let url = url.to_string();
    let headers = custom_headers.clone();
    let storage = storage.clone();
    let segment_count = segments.len();
    let policy = retry_policy.copied();

    if let Some(max) = max_concurrent {
        run::run_concurrent(
            url,
            headers,
            storage,
            incomplete,
            segment_count,
            max,
            policy,
            bitmap,
            summary_out,
            progress_tx,
        )
    } else {
        run::run_unbounded(
            url,
            headers,
            storage,
            incomplete,
            segment_count,
            policy,
            bitmap,
            summary_out,
            progress_tx,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segmenter::plan_segments;

    #[test]
    fn parse_content_range_parses_valid_header() {
        let headers = vec![
            "HTTP/1.1 206 Partial Content".to_string(),
            "Content-Range: bytes 100-199/1000".to_string(),
        ];
        assert_eq!(segment::parse_content_range(&headers), Some((100, 199)));
        let headers_lower = vec!["content-range: bytes 0-99/*".to_string()];
        assert_eq!(segment::parse_content_range(&headers_lower), Some((0, 99)));
    }

    #[test]
    fn download_segments_updates_bitmap() {
        let segments = plan_segments(1000, 4);
        let mut bitmap = SegmentBitmap::new(4);
        assert!(!bitmap.all_completed(4));
        bitmap.set_completed(0);
        bitmap.set_completed(2);
        let incomplete: Vec<_> = segments
            .iter()
            .enumerate()
            .filter(|(i, _)| !bitmap.is_completed(*i))
            .collect();
        assert_eq!(incomplete.len(), 2);
        assert!(bitmap.is_completed(0));
        assert!(!bitmap.is_completed(1));
        assert!(bitmap.is_completed(2));
        assert!(!bitmap.is_completed(3));
    }
}
