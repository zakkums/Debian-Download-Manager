//! Run the actual segment download in a blocking task (Easy or Multi backend).

use std::sync::Arc;

use crate::downloader;
use crate::downloader::CurlOptions;
use crate::downloader::DownloadSummary;
use crate::retry::RetryPolicy;
use crate::segmenter;
use crate::storage;

/// Runs segment download on a blocking thread. Chooses Easy (threads) or Multi
/// based on `use_multi`. If `abort` is set and becomes true, returns JobAborted.
pub(super) fn run_download_blocking(
    url: &str,
    headers: &std::collections::HashMap<String, String>,
    segments: &[segmenter::Segment],
    storage: &storage::StorageWriter,
    bitmap: &mut segmenter::SegmentBitmap,
    max_concurrent: usize,
    policy: &RetryPolicy,
    summary: &mut DownloadSummary,
    bitmap_tx: Option<&tokio::sync::mpsc::Sender<Vec<u8>>>,
    in_flight: Option<Arc<Vec<std::sync::atomic::AtomicU64>>>,
    abort: Option<Arc<std::sync::atomic::AtomicBool>>,
    use_multi: bool,
    curl: CurlOptions,
) -> anyhow::Result<()> {
    let max_concurrent = max_concurrent.max(1);
    if use_multi {
        downloader::multi::download_segments_multi(
            url,
            headers,
            segments,
            storage,
            bitmap,
            Some(max_concurrent),
            Some(policy),
            summary,
            bitmap_tx,
            in_flight,
            abort,
            curl,
        )
    } else {
        downloader::download_segments(
            url,
            headers,
            segments,
            storage,
            bitmap,
            Some(max_concurrent),
            Some(policy),
            summary,
            bitmap_tx,
            in_flight,
            abort,
            curl,
        )
    }
}
