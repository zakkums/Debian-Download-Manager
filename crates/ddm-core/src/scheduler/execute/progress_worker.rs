//! Background task that persists bitmap updates and sends progress stats.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::resume_db::ResumeDb;
use crate::segmenter;

use crate::scheduler::progress::ProgressStats;

/// Runs the progress persistence loop: receive bitmap blobs, persist to DB,
/// and optionally send ProgressStats to the CLI. Spawn this with tokio::spawn.
pub(super) async fn run_progress_persistence_loop(
    mut progress_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    db: ResumeDb,
    job_id: i64,
    segment_count_u: usize,
    segments: Vec<segmenter::Segment>,
    total_size_u: u64,
    stats_tx: Option<tokio::sync::mpsc::Sender<ProgressStats>>,
    in_flight: Arc<Vec<AtomicU64>>,
    download_start: Instant,
) {
    while let Some(blob) = progress_rx.recv().await {
        if db.update_bitmap(job_id, &blob).await.is_err() {
            tracing::warn!(job_id, "durable progress update failed");
        }
        if let Some(ref tx) = stats_tx {
            let bitmap = segmenter::SegmentBitmap::from_bytes(&blob, segment_count_u);
            let bytes_done: u64 = segments
                .iter()
                .enumerate()
                .filter(|(i, _)| bitmap.is_completed(*i))
                .map(|(_, s)| s.end - s.start)
                .sum();
            let bytes_in_flight: u64 = in_flight
                .iter()
                .enumerate()
                .filter(|(i, _)| !bitmap.is_completed(*i))
                .map(|(_, a)| a.load(Ordering::Relaxed))
                .sum();
            let elapsed_secs = download_start.elapsed().as_secs_f64();
            let segments_done = (0..segment_count_u)
                .filter(|i| bitmap.is_completed(*i))
                .count();
            let stats = ProgressStats {
                bytes_done,
                bytes_in_flight,
                total_bytes: total_size_u,
                elapsed_secs,
                segments_done,
                segment_count: segment_count_u,
            };
            let _ = tx.try_send(stats);
        }
    }
}
