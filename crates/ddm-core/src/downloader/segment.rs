//! Single-segment HTTP Range GET and write to storage.

use crate::retry::SegmentError;
use crate::segmenter::Segment;
use crate::storage::StorageWriter;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Result of a single segment download (used for retry classification).
pub(super) type SegmentResult = Result<(), SegmentError>;

/// Downloads a single segment: GET with Range header, write body to storage at segment offset.
/// Returns `SegmentError` so callers can classify and retry with backoff.
pub(super) fn download_one_segment(
    url: &str,
    custom_headers: &HashMap<String, String>,
    segment: &Segment,
    storage: &StorageWriter,
) -> SegmentResult {
    let bytes_written = Arc::new(AtomicU64::new(0));
    let bytes_written_in_cb = Arc::clone(&bytes_written);
    let storage_error: Arc<Mutex<Option<std::io::Error>>> = Arc::new(Mutex::new(None));
    let storage_error_cb = Arc::clone(&storage_error);
    let segment_start = segment.start;
    let storage = storage.clone();

    let mut easy = curl::easy::Easy::new();
    easy.url(url).map_err(SegmentError::Curl)?;
    easy.follow_location(true).map_err(SegmentError::Curl)?;
    easy.connect_timeout(Duration::from_secs(30))
        .map_err(SegmentError::Curl)?;
    // Prefer low-speed timeout: abort if throughput drops below 1 KiB/s for 60s.
    // Keeps large segments on slow links from being killed by a hard wall-clock timeout.
    easy.low_speed_limit(1024)
        .map_err(SegmentError::Curl)?;
    easy.low_speed_time(Duration::from_secs(60))
        .map_err(SegmentError::Curl)?;
    // Safety net: hard timeout after 1 hour so a completely stuck transfer eventually fails.
    easy.timeout(Duration::from_secs(3600))
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
                let off = bytes_written_in_cb.fetch_add(data.len() as u64, Ordering::Relaxed);
                match storage.write_at(segment_start + off, data) {
                    Ok(()) => Ok(data.len()),
                    Err(e) => {
                        let io_err = e
                            .downcast::<std::io::Error>()
                            .unwrap_or_else(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));
                        let _ = storage_error_cb.lock().unwrap().replace(io_err);
                        Ok(0)
                    }
                }
            })
            .map_err(SegmentError::Curl)?;
        let perform_result = transfer.perform();
        if let Err(e) = perform_result {
            if e.is_write_error() {
                if let Some(io_err) = storage_error.lock().unwrap().take() {
                    return Err(SegmentError::Storage(io_err));
                }
            }
            return Err(SegmentError::Curl(e));
        }
    }

    let code = easy.response_code().map_err(SegmentError::Curl)? as u32;
    if code < 200 || code >= 300 {
        return Err(SegmentError::Http(code));
    }

    let received = bytes_written.load(Ordering::Relaxed);
    let expected = segment.len();
    if received != expected {
        return Err(SegmentError::PartialTransfer { expected, received });
    }

    Ok(())
}
