//! Single-segment HTTP Range GET and write to storage.
//!
//! Enforces Range behavior: we always send a Range header, so we require HTTP 206
//! Partial Content to avoid servers that ignore Range and return 200 with the
//! full body (which would corrupt the temp file when written at segment offset).

use crate::retry::SegmentError;
use crate::segmenter::Segment;
use crate::storage::StorageWriter;
use std::collections::HashMap;
use std::str;
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

    let mut response_headers: Vec<String> = Vec::new();
    {
        let mut transfer = easy.transfer();
        transfer
            .header_function(|data| {
                if let Ok(s) = str::from_utf8(data) {
                    response_headers.push(s.trim_end().to_string());
                }
                true
            })
            .map_err(SegmentError::Curl)?;
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

    // We sent a Range header; require 206 Partial Content so we don't accept 200 full-body (corruption).
    if code != 206 {
        return Err(SegmentError::InvalidRangeResponse(code));
    }

    // Optionally validate Content-Range matches our segment (extra safety).
    if let Some((start, end)) = parse_content_range(&response_headers) {
        if start != segment.start || end != segment.end.saturating_sub(1) {
            return Err(SegmentError::InvalidRangeResponse(code));
        }
    }

    let received = bytes_written.load(Ordering::Relaxed);
    let expected = segment.len();
    if received != expected {
        return Err(SegmentError::PartialTransfer { expected, received });
    }

    Ok(())
}

/// Parse Content-Range from response headers. Returns (start, end_inclusive) if present and valid.
/// Format: "Content-Range: bytes start-end/total" or "bytes start-end/*".
pub(crate) fn parse_content_range(headers: &[String]) -> Option<(u64, u64)> {
    const PREFIX: &str = "Content-Range:";
    for line in headers {
        let line = line.trim();
        if line.len() >= PREFIX.len() && line[..PREFIX.len()].eq_ignore_ascii_case(PREFIX) {
            let rest = line[PREFIX.len()..].trim();
            let rest = rest.strip_prefix("bytes")?.trim();
            let (range, _) = rest.split_once('/')?;
            let range = range.trim();
            let (start_str, end_str) = range.split_once('-')?;
            let start: u64 = start_str.trim().parse().ok()?;
            let end_inclusive: u64 = end_str.trim().parse().ok()?;
            return Some((start, end_inclusive));
        }
    }
    None
}
