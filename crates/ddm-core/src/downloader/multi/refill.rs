//! Refill helpers for the multi event loop: when to wait for next retry and
//! how to keep the active set full.

use anyhow::Result;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::segmenter::Segment;
use crate::storage::StorageWriter;

use super::handler::SegmentHandler;
use super::super::CurlOptions;

/// Active entry in the multi event loop: handle + segment index + metadata.
pub(super) type ActiveItem =
    (curl::multi::Easy2Handle<SegmentHandler>, usize, Segment, u32);

/// Returns wait time in ms until the next retry is ready, capped at 100.
pub(super) fn next_retry_wait_ms(retry_after: &[(Instant, usize, Segment, u32)]) -> u64 {
    let now = Instant::now();
    retry_after
        .iter()
        .filter_map(|(t, ..)| t.checked_duration_since(now))
        .min()
        .map(|d| d.as_millis().min(100) as u64)
        .unwrap_or(100)
}

/// Add a new Easy handle for the given segment to the multi handle, configuring
/// range, headers, timeouts and optional bandwidth/buffer settings.
pub(super) fn add_easy_to_multi(
    multi: &curl::multi::Multi,
    url: &str,
    headers: &HashMap<String, String>,
    storage: &StorageWriter,
    in_flight_bytes: Option<&Arc<Vec<AtomicU64>>>,
    index: usize,
    segment: Segment,
    curl: CurlOptions,
) -> Result<curl::multi::Easy2Handle<SegmentHandler>> {
    let handler = SegmentHandler::new(
        index,
        segment,
        storage.clone(),
        in_flight_bytes.map(Arc::clone),
    );
    let mut easy = curl::easy::Easy2::new(handler);
    easy.url(url).map_err(|e| anyhow::anyhow!("curl url: {}", e))?;
    easy.follow_location(true)
        .map_err(|e| anyhow::anyhow!("curl: {}", e))?;
    easy.max_redirections(10)
        .map_err(|e| anyhow::anyhow!("curl: {}", e))?;
    if let Some(speed) = curl.max_recv_speed {
        easy.max_recv_speed(speed)
            .map_err(|e| anyhow::anyhow!("curl: {}", e))?;
    }
    if let Some(sz) = curl.buffer_size {
        easy.buffer_size(sz)
            .map_err(|e| anyhow::anyhow!("curl: {}", e))?;
    }
    easy.connect_timeout(Duration::from_secs(30))
        .map_err(|e| anyhow::anyhow!("curl: {}", e))?;
    easy.low_speed_limit(1024)
        .map_err(|e| anyhow::anyhow!("curl: {}", e))?;
    easy.low_speed_time(Duration::from_secs(60))
        .map_err(|e| anyhow::anyhow!("curl: {}", e))?;
    easy.timeout(Duration::from_secs(3600))
        .map_err(|e| anyhow::anyhow!("curl: {}", e))?;
    let end = segment.end.saturating_sub(1);
    easy.range(&format!("{}-{}", segment.start, end))
        .map_err(|e| anyhow::anyhow!("curl: {}", e))?;
    if !headers.is_empty() {
        let mut list = curl::easy::List::new();
        for (k, v) in headers {
            list.append(&format!("{}: {}", k.trim(), v.trim()))
                .map_err(|e| anyhow::anyhow!("curl: {}", e))?;
        }
        easy.http_headers(list)
            .map_err(|e| anyhow::anyhow!("curl: {}", e))?;
    }
    multi
        .add2(easy)
        .map_err(|e| anyhow::anyhow!("curl multi add: {}", e))
}

/// Refill the active set with pending or ready-to-retry segments until
/// `max_concurrent` is reached or there is nothing left to schedule.
pub(super) fn refill_active(
    multi: &curl::multi::Multi,
    url: &str,
    headers: &HashMap<String, String>,
    storage: &StorageWriter,
    in_flight_bytes: Option<&Arc<Vec<AtomicU64>>>,
    max_concurrent: usize,
    active: &mut Vec<ActiveItem>,
    pending: &mut VecDeque<(usize, Segment)>,
    retry_after: &mut Vec<(Instant, usize, Segment, u32)>,
    curl: CurlOptions,
) -> Result<()> {
    let now = Instant::now();
    while active.len() < max_concurrent {
        if let Some((index, segment)) = pending.pop_front() {
            let h = add_easy_to_multi(
                multi,
                url,
                headers,
                storage,
                in_flight_bytes,
                index,
                segment,
                curl,
            )?;
            active.push((h, index, segment, 1));
        } else if let Some(pos) = retry_after.iter().position(|(t, ..)| now >= *t) {
            let (_, index, segment, attempt) = retry_after.remove(pos);
            let h = add_easy_to_multi(
                multi,
                url,
                headers,
                storage,
                in_flight_bytes,
                index,
                segment,
                curl,
            )?;
            active.push((h, index, segment, attempt));
        } else {
            break;
        }
    }
    Ok(())
}

