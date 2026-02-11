//! Single-segment HTTP Range GET and write to storage.
//!
//! Enforces Range behavior: we always send a Range header, so we require HTTP 206
//! Partial Content to avoid servers that ignore Range and return 200 with the
//! full body (which would corrupt the temp file when written at segment offset).
//! Validation is done in the write callback before writing any byte (pre-write).

use crate::retry::SegmentError;
use crate::segmenter::Segment;
use crate::storage::StorageWriter;
use super::CurlOptions;
use std::collections::HashMap;
use std::str;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Result of a single segment download (used for retry classification).
pub(super) type SegmentResult = Result<(), SegmentError>;

/// Optional in-flight counter: (per-segment bytes vec, segment index). Updated in write callback.
pub(super) type InFlightRef = Option<(Arc<Vec<AtomicU64>>, usize)>;

/// Downloads a single segment: GET with Range header, write body to storage at segment offset.
/// Validates 206 and Content-Range before writing any body; aborts on first write if not honored.
/// If `in_flight` is Some, the segment's byte count is written so progress can sum in-flight bytes.
pub(super) fn download_one_segment(
    url: &str,
    custom_headers: &HashMap<String, String>,
    segment: &Segment,
    storage: &StorageWriter,
    in_flight: InFlightRef,
    curl: CurlOptions,
) -> SegmentResult {
    let bytes_written = Arc::new(AtomicU64::new(0));
    let bytes_written_in_cb = Arc::clone(&bytes_written);
    let storage_error: Arc<Mutex<Option<std::io::Error>>> = Arc::new(Mutex::new(None));
    let storage_error_cb = Arc::clone(&storage_error);
    let response_headers: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let response_headers_header = Arc::clone(&response_headers);
    let response_headers_write = Arc::clone(&response_headers);
    let range_check: Arc<Mutex<Option<Result<(), u32>>>> = Arc::new(Mutex::new(None));
    let range_check_cb = Arc::clone(&range_check);
    let segment_start = segment.start;
    let segment_end_inclusive = segment.end.saturating_sub(1);
    let storage = storage.clone();

    let mut easy = curl::easy::Easy::new();
    easy.url(url).map_err(SegmentError::Curl)?;
    easy.follow_location(true).map_err(SegmentError::Curl)?;
    if let Some(speed) = curl.max_recv_speed {
        easy.max_recv_speed(speed).map_err(SegmentError::Curl)?;
    }
    if let Some(sz) = curl.buffer_size {
        easy.buffer_size(sz).map_err(SegmentError::Curl)?;
    }
    easy.connect_timeout(Duration::from_secs(30))
        .map_err(SegmentError::Curl)?;
    easy.low_speed_limit(1024).map_err(SegmentError::Curl)?;
    easy.low_speed_time(Duration::from_secs(60))
        .map_err(SegmentError::Curl)?;
    easy.timeout(Duration::from_secs(3600))
        .map_err(SegmentError::Curl)?;

    let range_str = format!("{}-{}", segment.start, segment_end_inclusive);
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
            .header_function(move |data| {
                if let Ok(s) = str::from_utf8(data) {
                    let line = s.trim_end();
                    if line.starts_with("HTTP/") {
                        let mut vec = response_headers_header.lock().unwrap();
                        vec.clear();
                        vec.push(line.to_string());
                    } else {
                        let _ = response_headers_header.lock().unwrap().push(line.to_string());
                    }
                }
                true
            })
            .map_err(SegmentError::Curl)?;
        transfer
            .write_function(move |data| {
                let mut check = range_check_cb.lock().unwrap();
                if check.is_none() {
                    let headers = response_headers_write.lock().unwrap().clone();
                    let status = parse_http_status(&headers);
                    let content_ok = parse_content_range(&headers)
                        .map(|(s, e)| s == segment_start && e == segment_end_inclusive)
                        .unwrap_or(false);
                    let ok = status == Some(206) && content_ok;
                    *check = Some(if ok { Ok(()) } else { Err(status.unwrap_or(0)) });
                }
                if let Some(Err(_)) = *check {
                    return Ok(0);
                }
                let off = bytes_written_in_cb.fetch_add(data.len() as u64, Ordering::Relaxed);
                if let Some((ref v, idx)) = in_flight {
                    v.get(idx).map(|a| a.store(bytes_written_in_cb.load(Ordering::Relaxed), Ordering::Relaxed));
                }
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
                if let Some(Err(code)) = range_check.lock().unwrap().take() {
                    return Err(SegmentError::InvalidRangeResponse(code));
                }
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
    if code != 206 {
        return Err(SegmentError::InvalidRangeResponse(code));
    }
    if let Some((start, end)) = parse_content_range(&response_headers.lock().unwrap()) {
        if start != segment.start || end != segment_end_inclusive {
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

/// Parse HTTP status code from the first header line (e.g. "HTTP/1.1 206 ...").
pub(crate) fn parse_http_status(headers: &[String]) -> Option<u32> {
    let first = headers.first()?.trim();
    let part = first.split_whitespace().nth(1)?;
    part.parse().ok()
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
