//! Curl multi event loop: perform, wait, messages; process completed handles.
//! Supports per-segment retry with backoff when RetryPolicy is provided.

use anyhow::Result;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::control::JobAborted;
use crate::retry::{classify, ErrorKind, RetryDecision, RetryPolicy};
use crate::segmenter::{Segment, SegmentBitmap};
use crate::storage::StorageWriter;

use super::handler::SegmentHandler;
use super::refill;
use super::result;
use super::super::DownloadSummary;
use super::super::CurlOptions;

const COALESCE_PROGRESS_EVERY: usize = 2;

/// Run incomplete segments using curl multi: add up to max_concurrent Easy2 handles,
/// perform/wait/messages loop, process completions and add more until done or error.
/// When retry_policy is Some, retryable failures are re-queued with backoff.
pub(super) fn run_multi(
    url: &str,
    headers: &HashMap<String, String>,
    storage: &StorageWriter,
    incomplete: Vec<(usize, Segment)>,
    segment_count: usize,
    max_concurrent: usize,
    bitmap: &mut SegmentBitmap,
    summary_out: &mut DownloadSummary,
    progress_tx: Option<&tokio::sync::mpsc::Sender<Vec<u8>>>,
    in_flight_bytes: Option<Arc<Vec<AtomicU64>>>,
    abort: Option<Arc<AtomicBool>>,
    retry_policy: Option<RetryPolicy>,
    curl: CurlOptions,
) -> Result<()> {
    if incomplete.is_empty() {
        return Ok(());
    }

    let multi = curl::multi::Multi::new();
    let mut pending: VecDeque<(usize, Segment)> = incomplete.into_iter().collect();
    let mut retry_after: Vec<(Instant, usize, Segment, u32)> = Vec::new();
    let mut active: Vec<(curl::multi::Easy2Handle<SegmentHandler>, usize, Segment, u32)> = Vec::new();
    let mut first_error: Option<anyhow::Error> = None;
    let mut completed_since_send = 0usize;

    let to_add = max_concurrent.min(pending.len());
    for _ in 0..to_add {
        if let Some((index, segment)) = pending.pop_front() {
            let h = refill::add_easy_to_multi(
                &multi,
                url,
                headers,
                storage,
                in_flight_bytes.as_ref(),
                index,
                segment,
                curl,
            )?;
            active.push((h, index, segment, 1));
        }
    }

    while !active.is_empty() {
        if abort.as_ref().map(|a| a.load(Ordering::Relaxed)).unwrap_or(false) {
            if first_error.is_none() {
                first_error = Some(anyhow::anyhow!(JobAborted));
            }
            break;
        }
        let running = multi.perform().map_err(|e| anyhow::anyhow!("curl multi perform: {}", e))?;
        let mut completed_indices: Vec<usize> = Vec::new();
        multi.messages(|msg| {
            for (i, (ref handle, ..)) in active.iter().enumerate() {
                if msg.result_for2(handle).is_some() {
                    completed_indices.push(i);
                    break;
                }
            }
        });
        completed_indices.sort_by(|a, b| b.cmp(a));
        for &i in &completed_indices {
            let (handle, seg_index, segment, attempt) = active.remove(i);
            let mut easy = multi.remove2(handle).map_err(|e| anyhow::anyhow!("curl multi remove: {}", e))?;
            let code = easy.response_code().unwrap_or(0);
            let handler = easy.get_mut();
            let res = result::segment_result_from_easy(code, &segment, handler);
            match res {
                Ok(()) => {
                    bitmap.set_completed(seg_index);
                    completed_since_send += 1;
                    if let Some(ref tx) = progress_tx {
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
                    let will_retry = retry_policy.as_ref().and_then(|policy| {
                        match policy.decide(attempt, kind) {
                            RetryDecision::RetryAfter(d) => Some((Instant::now() + d, seg_index, segment, attempt + 1)),
                            RetryDecision::NoRetry => None,
                        }
                    });
                    if let Some(entry) = will_retry {
                        retry_after.push(entry);
                    } else {
                        if first_error.is_none() {
                            first_error = Some(anyhow::anyhow!("{}", e).context(format!("segment {}", seg_index)));
                        }
                    }
                }
            }
        }
        refill::refill_active(
            &multi,
            url,
            headers,
            storage,
            in_flight_bytes.as_ref(),
            max_concurrent,
            &mut active,
            &mut pending,
            &mut retry_after,
            curl,
        )?;
        if first_error.is_some() {
            break;
        }
        if running > 0 {
            let wait_ms = refill::next_retry_wait_ms(&retry_after).min(100);
            multi.wait(&mut [], Duration::from_millis(wait_ms))
                .map_err(|e| anyhow::anyhow!("curl multi wait: {}", e))?;
        }
    }

    if completed_since_send > 0 {
        if let Some(ref tx) = progress_tx {
            let _ = tx.try_send(bitmap.to_bytes(segment_count));
        }
    }
    if let Some(e) = first_error {
        return Err(e);
    }
    Ok(())
}
