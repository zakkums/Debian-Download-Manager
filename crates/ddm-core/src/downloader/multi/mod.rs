//! Curl multi backend: single-threaded event loop, multiple Easy2 handles.
//!
//! Drives segment downloads via one `curl::multi` handle for connection reuse.

mod handler;
mod refill;
mod result;
mod run;

use anyhow::Result;
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use crate::retry::RetryPolicy;
use crate::segmenter::{Segment, SegmentBitmap};
use crate::storage::StorageWriter;

use super::DownloadSummary;
use super::CurlOptions;

/// Runs segment downloads via the curl multi backend (Easy2 + Multi handle).
/// When retry_policy is Some, retryable segment failures are retried with backoff.
pub fn download_segments_multi(
    url: &str,
    custom_headers: &HashMap<String, String>,
    segments: &[Segment],
    storage: &StorageWriter,
    bitmap: &mut SegmentBitmap,
    max_concurrent: Option<usize>,
    retry_policy: Option<&RetryPolicy>,
    summary_out: &mut DownloadSummary,
    progress_tx: Option<&tokio::sync::mpsc::Sender<Vec<u8>>>,
    in_flight_bytes: Option<Arc<Vec<AtomicU64>>>,
    curl: CurlOptions,
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

    let max = max_concurrent.unwrap_or_else(|| incomplete.len()).max(1);
    run::run_multi(
        url,
        custom_headers,
        storage,
        incomplete,
        segments.len(),
        max,
        bitmap,
        summary_out,
        progress_tx,
        in_flight_bytes,
        retry_policy.copied(),
        curl,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segmenter::plan_segments;

    #[test]
    fn download_segments_multi_empty_incomplete_returns_ok() {
        let segments = plan_segments(1000, 4);
        let mut bitmap = crate::segmenter::SegmentBitmap::new(4);
        bitmap.set_completed(0);
        bitmap.set_completed(1);
        bitmap.set_completed(2);
        bitmap.set_completed(3);
        let mut summary = DownloadSummary::default();
        let headers = std::collections::HashMap::new();
        let dir = tempfile::tempdir().unwrap();
        let tp = crate::storage::temp_path(&dir.path().join("out.bin"));
        let mut builder = crate::storage::StorageWriterBuilder::create(&tp).unwrap();
        builder.preallocate(1000).unwrap();
        let storage = builder.build();
        let result = download_segments_multi(
            "http://example.com/file",
            &headers,
            &segments,
            &storage,
            &mut bitmap,
            Some(2),
            None,
            &mut summary,
            None,
            None,
            CurlOptions::default(),
        );
        assert!(result.is_ok(), "multi returns Ok when no segments to download");
    }
}
