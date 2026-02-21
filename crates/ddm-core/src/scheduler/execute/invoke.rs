//! Run the segment download in a blocking task; returns bitmap and summary or error.

use anyhow::{Context, Result};
use std::sync::Arc;

use crate::downloader::DownloadSummary;
use crate::segmenter;

use super::run_download::run_download_blocking;

/// Runs download in spawn_blocking. Returns Ok((bitmap, summary)) or Err.
/// Caller handles JobAborted (set state to Paused, etc.).
pub(super) async fn run_download_blocking_async(
    url: &str,
    headers: &std::collections::HashMap<String, String>,
    segments: &[segmenter::Segment],
    storage: &crate::storage::StorageWriter,
    bitmap: &segmenter::SegmentBitmap,
    actual_concurrent: usize,
    retry_policy: &crate::retry::RetryPolicy,
    bitmap_tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    in_flight_bytes: Arc<Vec<std::sync::atomic::AtomicU64>>,
    abort: Option<Arc<std::sync::atomic::AtomicBool>>,
    use_multi: bool,
    curl_opts: crate::downloader::CurlOptions,
) -> Result<(segmenter::SegmentBitmap, DownloadSummary)> {
    let url = url.to_string();
    let headers = headers.clone();
    let segments = segments.to_vec();
    let storage = storage.clone();
    let mut bitmap_copy = bitmap.clone();
    let policy = retry_policy.clone();
    let in_flight = in_flight_bytes;
    let curl = curl_opts;

    tokio::task::spawn_blocking(move || {
        let mut summary = DownloadSummary::default();
        run_download_blocking(
            &url,
            &headers,
            &segments,
            &storage,
            &mut bitmap_copy,
            actual_concurrent,
            &policy,
            &mut summary,
            Some(&bitmap_tx),
            Some(in_flight),
            abort,
            use_multi,
            curl,
        )?;
        Ok((bitmap_copy, summary))
    })
    .await
    .context("download task join")?
}
