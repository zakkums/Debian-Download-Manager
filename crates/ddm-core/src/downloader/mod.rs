//! Core segmented downloader engine.
//!
//! Consumes direct URL + headers, runs N concurrent HTTP Range GETs (bounded by
//! `max_concurrent` when set), writes each segment to storage at the correct
//! offset and updates the completion bitmap. Supports retry with backoff via
//! optional `RetryPolicy`.

mod segment;

use anyhow::Result;
use crate::retry::{classify, run_with_retry, ErrorKind, RetryPolicy, SegmentError};
use crate::segmenter::{Segment, SegmentBitmap};
use crate::storage::StorageWriter;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

/// Result of a single segment download (used for retry classification).
pub type SegmentResult = Result<(), SegmentError>;

/// Summary of a download run for adaptive policy: throttle and error counts.
#[derive(Debug, Clone, Default)]
pub struct DownloadSummary {
    pub throttle_events: u32,
    pub error_events: u32,
}

/// Downloads all segments that are not yet completed, writing to `storage` and updating `bitmap`.
/// When `max_concurrent` is `Some(n)`, at most `n` segment downloads run at once (per-host /
/// global connection limit). When `None`, one thread per incomplete segment (no limit).
/// When `retry_policy` is `Some`, each segment is retried with exponential backoff on
/// timeouts, connection errors, and 429/503/5xx.
/// Fills `summary_out` with throttle/error counts (even on failure) for adaptive policy.
/// If `progress_tx` is `Some`, the current bitmap is sent after each completed segment
/// (coalesced every COALESCE_PROGRESS_EVERY completions) so the caller can persist progress.
const COALESCE_PROGRESS_EVERY: usize = 2;

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

    let count = incomplete.len();
    let segment_count = segments.len();

    if let Some(max) = max_concurrent {
        let work: Arc<Mutex<VecDeque<(usize, Segment)>>> =
            Arc::new(Mutex::new(incomplete.into_iter().collect()));
        let abort_requested = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let num_workers = max.min(count);
        let mut handles = Vec::with_capacity(num_workers);
        for _ in 0..num_workers {
            let work = Arc::clone(&work);
            let tx = tx.clone();
            let abort = Arc::clone(&abort_requested);
            let u = url.clone();
            let h = headers.clone();
            let st = storage.clone();
            let policy = retry_policy.copied();
            handles.push(std::thread::spawn(move || {
                loop {
                    if abort.load(Ordering::Relaxed) {
                        break;
                    }
                    let (index, segment) = match work.lock().unwrap().pop_front() {
                        Some(p) => p,
                        None => break,
                    };
                    let res: SegmentResult = match policy {
                        Some(p) => run_with_retry(&p, || segment::download_one_segment(&u, &h, &segment, &st)),
                        None => segment::download_one_segment(&u, &h, &segment, &st),
                    };
                    let _ = tx.send((index, res));
                }
            }));
        }
        drop(tx);

        // Process results as they arrive: mark bitmap on each Ok, persist (coalesced), record first error but drain all.
        let mut first_error: Option<anyhow::Error> = None;
        let mut completed_since_send = 0usize;
        for _ in 0..count {
            let (index, res) = rx.recv().expect("worker result");
            match res {
                Ok(()) => {
                    bitmap.set_completed(index);
                    completed_since_send += 1;
                    if let Some(progress_tx) = progress_tx {
                        if completed_since_send >= COALESCE_PROGRESS_EVERY {
                            let _ = progress_tx.try_send(bitmap.to_bytes(segment_count));
                            completed_since_send = 0;
                        }
                    }
                }
                Err(e) => {
                    let kind = classify(&e);
                    if kind == ErrorKind::Throttled {
                        summary_out.throttle_events += 1;
                    } else if kind != ErrorKind::Other {
                        summary_out.error_events += 1;
                    }
                    if kind == ErrorKind::Other {
                        abort_requested.store(true, Ordering::Relaxed);
                    }
                    if first_error.is_none() {
                        first_error = Some(anyhow::anyhow!("{}", e).context(format!("segment {}", index)));
                    }
                }
            }
        }
        // Send any remaining coalesced progress.
        if completed_since_send > 0 {
            if let Some(progress_tx) = progress_tx {
                let _ = progress_tx.try_send(bitmap.to_bytes(segment_count));
            }
        }
        for h in handles {
            h.join().unwrap_or_else(|e| panic!("worker panicked: {:?}", e));
        }
        if let Some(e) = first_error {
            return Err(e);
        }
        return Ok(());
    }

    // Sequential path (no worker pool): run segments in order, still process each result and persist progress.
    let mut first_error: Option<anyhow::Error> = None;
    let mut completed_since_send = 0usize;
    let results_seq: Vec<(usize, SegmentResult)> = {
        incomplete
            .into_iter()
            .map(|(index, segment)| {
                let u = url.clone();
                let h = headers.clone();
                let st = storage.clone();
                let policy = retry_policy.copied();
                let res = std::thread::spawn(move || {
                    match policy {
                        Some(p) => run_with_retry(&p, || segment::download_one_segment(&u, &h, &segment, &st)),
                        None => segment::download_one_segment(&u, &h, &segment, &st),
                    }
                })
                .join()
                .unwrap_or_else(|e| panic!("worker panicked: {:?}", e));
                (index, res)
            })
            .collect()
    };

    for (index, res) in results_seq {
        match res {
            Ok(()) => {
                bitmap.set_completed(index);
                completed_since_send += 1;
                if let Some(tx) = progress_tx {
                    if completed_since_send >= COALESCE_PROGRESS_EVERY {
                        let _ = tx.try_send(bitmap.to_bytes(segment_count));
                        completed_since_send = 0;
                    }
                }
            }
            Err(e) => {
                let kind = classify(&e);
                if kind == ErrorKind::Throttled {
                    summary_out.throttle_events += 1;
                } else if kind != ErrorKind::Other {
                    summary_out.error_events += 1;
                }
                if first_error.is_none() {
                    first_error = Some(anyhow::anyhow!("{}", e).context(format!("segment {}", index)));
                }
            }
        }
    }
    if completed_since_send > 0 {
        if let Some(tx) = progress_tx {
            let _ = tx.try_send(bitmap.to_bytes(segment_count));
        }
    }
    if let Some(e) = first_error {
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segmenter::plan_segments;

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
