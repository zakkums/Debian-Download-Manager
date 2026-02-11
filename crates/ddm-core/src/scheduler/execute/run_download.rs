//! Run the actual segment download in a blocking task (Easy or Multi backend).

use std::sync::Arc;

use crate::downloader::DownloadSummary;
use crate::downloader;
use crate::retry::RetryPolicy;
use crate::segmenter;
use crate::storage;

/// Runs segment download on a blocking thread. Chooses Easy (threads) or Multi
/// based on `use_multi`. Returns updated bitmap and summary.
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
    use_multi: bool,
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
        )
    }
}
