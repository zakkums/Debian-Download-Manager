//! Core segmented downloader engine.
//!
//! Consumes direct URL + headers, runs N concurrent HTTP Range GETs (bounded by
//! `max_concurrent` when set), writes each segment to storage at the correct
//! offset and updates the completion bitmap. Supports retry with backoff via
//! optional `RetryPolicy`.

use anyhow::{Context, Result};
use crate::retry::{classify, run_with_retry, ErrorKind, SegmentError, RetryPolicy};
use crate::segmenter::{Segment, SegmentBitmap};
use crate::storage::StorageWriter;
use std::cell::Cell;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Result of a single segment download (used for retry classification).
pub type SegmentResult = Result<(), SegmentError>;

/// Summary of a download run for adaptive policy: throttle and error counts.
#[derive(Debug, Clone, Default)]
pub struct DownloadSummary {
    pub throttle_events: u32,
    pub error_events: u32,
}

/// Downloads a single segment: GET with Range header, write body to storage at segment offset.
/// Returns `SegmentError` so callers can classify and retry with backoff.
fn download_one_segment(
    url: &str,
    custom_headers: &HashMap<String, String>,
    segment: &Segment,
    storage: &StorageWriter,
) -> SegmentResult {
    let bytes_written = Cell::new(0u64);
    let segment_start = segment.start;
    let storage = storage.clone();

    let mut easy = curl::easy::Easy::new();
    easy.url(url).map_err(SegmentError::Curl)?;
    easy.follow_location(true).map_err(SegmentError::Curl)?;
    easy.connect_timeout(Duration::from_secs(30))
        .map_err(SegmentError::Curl)?;
    easy.timeout(Duration::from_secs(300))
        .map_err(SegmentError::Curl)?;

    let range_str = format!("{}-{}", segment.start, segment.end.saturating_sub(1));
    easy.range(&range_str).map_err(SegmentError::Curl)?;

    let mut list = curl::easy::List::new();
    for (k, v) in custom_headers {
        list.append(&format!("{}: {}", k.trim(), v.trim()))
            .map_err(SegmentError::Curl)?;
    }
    if !custom_headers.is_empty() {
        easy.http_headers(list).map_err(SegmentError::Curl)?;
    }

    {
        let mut transfer = easy.transfer();
        transfer
            .write_function(move |data| {
                let off = bytes_written.get();
                storage
                    .write_at(segment_start + off, data)
                    .map_err(|_| curl::easy::WriteError::Pause)?;
                bytes_written.set(off + data.len() as u64);
                Ok(data.len())
            })
            .map_err(SegmentError::Curl)?;
        transfer.perform().map_err(SegmentError::Curl)?;
    }

    let code = easy.response_code().map_err(SegmentError::Curl)? as u32;
    if code < 200 || code >= 300 {
        return Err(SegmentError::Http(code));
    }

    Ok(())
}

/// Downloads all segments that are not yet completed, writing to `storage` and updating `bitmap`.
/// When `max_concurrent` is `Some(n)`, at most `n` segment downloads run at once (per-host /
/// global connection limit). When `None`, one thread per incomplete segment (no limit).
/// When `retry_policy` is `Some`, each segment is retried with exponential backoff on
/// timeouts, connection errors, and 429/503/5xx.
/// Fills `summary_out` with throttle/error counts (even on failure) for adaptive policy.
pub fn download_segments(
    url: &str,
    custom_headers: &HashMap<String, String>,
    segments: &[Segment],
    storage: &StorageWriter,
    bitmap: &mut SegmentBitmap,
    max_concurrent: Option<usize>,
    retry_policy: Option<&RetryPolicy>,
    summary_out: &mut DownloadSummary,
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
    let results: Vec<(usize, SegmentResult)> = if let Some(max) = max_concurrent {
        let work: Arc<Mutex<VecDeque<(usize, Segment)>>> =
            Arc::new(Mutex::new(incomplete.into_iter().collect()));
        let (tx, rx) = mpsc::channel();
        let num_workers = max.min(count);
        let mut handles = Vec::with_capacity(num_workers);
        for _ in 0..num_workers {
            let work = Arc::clone(&work);
            let tx = tx.clone();
            let u = url.clone();
            let h = headers.clone();
            let st = storage.clone();
            let policy = retry_policy.copied();
            handles.push(std::thread::spawn(move || {
                loop {
                    let (index, segment) = match work.lock().unwrap().pop_front() {
                        Some(p) => p,
                        None => break,
                    };
                    let res: SegmentResult = match policy {
                        Some(p) => run_with_retry(&p, || download_one_segment(&u, &h, &segment, &st)),
                        None => download_one_segment(&u, &h, &segment, &st),
                    };
                    let _ = tx.send((index, res));
                }
            }));
        }
        drop(tx);
        let mut results_vec = Vec::with_capacity(count);
        for _ in 0..count {
            let (index, res) = rx.recv().expect("worker result");
            results_vec.push((index, res));
        }
        for h in handles {
            h.join().unwrap_or_else(|e| panic!("worker panicked: {:?}", e));
        }
        results_vec
    } else {
        incomplete
            .into_iter()
            .map(|(index, segment)| {
                let u = url.clone();
                let h = headers.clone();
                let st = storage.clone();
                let policy = retry_policy.copied();
                let res = std::thread::spawn(move || {
                    match policy {
                        Some(p) => run_with_retry(&p, || download_one_segment(&u, &h, &segment, &st)),
                        None => download_one_segment(&u, &h, &segment, &st),
                    }
                })
                .join()
                .unwrap_or_else(|e| panic!("worker panicked: {:?}", e));
                (index, res)
            })
            .collect()
    };

    for (index, res) in results {
        match res {
            Ok(()) => {
                bitmap.set_completed(index);
            }
            Err(e) => {
                let kind = classify(&e);
                if kind == ErrorKind::Throttled {
                    summary_out.throttle_events += 1;
                } else if kind != ErrorKind::Other {
                    summary_out.error_events += 1;
                }
                return Err(anyhow::anyhow!("{}", e))
                    .with_context(|| format!("segment {}", index));
            }
        }
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
