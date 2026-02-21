//! Concurrent and sequential execution of segment downloads.

use anyhow::Result;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use crate::retry::{classify, run_with_retry, ErrorKind, RetryPolicy};
use crate::segmenter::{Segment, SegmentBitmap};
use crate::storage::StorageWriter;

use crate::control::JobAborted;
use super::segment;
use super::CurlOptions;
use super::DownloadSummary;
use super::SegmentResult;

mod unbounded;
pub(super) use unbounded::run_unbounded;

pub(super) const COALESCE_PROGRESS_EVERY: usize = 2;

/// Run incomplete segments with a bounded worker pool. Process results as they
/// arrive; on ErrorKind::Other drain the queue and reduce expected count to avoid deadlock.
pub(super) fn run_concurrent(
    url: String,
    headers: HashMap<String, String>,
    storage: StorageWriter,
    incomplete: Vec<(usize, Segment)>,
    segment_count: usize,
    max_concurrent: usize,
    retry_policy: Option<RetryPolicy>,
    bitmap: &mut SegmentBitmap,
    summary_out: &mut DownloadSummary,
    progress_tx: Option<&tokio::sync::mpsc::Sender<Vec<u8>>>,
    in_flight_bytes: Option<Arc<Vec<AtomicU64>>>,
    abort: Option<Arc<AtomicBool>>,
    curl: CurlOptions,
) -> Result<()> {
    let count = incomplete.len();
    let work: Arc<Mutex<VecDeque<(usize, Segment)>>> =
        Arc::new(Mutex::new(incomplete.into_iter().collect()));
    let abort_requested = Arc::new(AtomicBool::new(false));
    let user_abort = abort.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let (tx, rx) = mpsc::channel();
    let num_workers = max_concurrent.min(count);
    let mut handles = Vec::with_capacity(num_workers);
    for _ in 0..num_workers {
        let work = Arc::clone(&work);
        let tx = tx.clone();
        let abort = Arc::clone(&abort_requested);
        let user_abort = Arc::clone(&user_abort);
        let u = url.clone();
        let h = headers.clone();
        let st = storage.clone();
        let policy = retry_policy;
        let curl_opts = curl;
        let in_flight = in_flight_bytes.as_ref().map(Arc::clone);
        handles.push(std::thread::spawn(move || {
            loop {
                if abort.load(Ordering::Relaxed) || user_abort.load(Ordering::Relaxed) {
                    break;
                }
                let (index, segment) = match work.lock().unwrap().pop_front() {
                    Some(p) => p,
                    None => break,
                };
                let in_flight_seg = in_flight.as_ref().map(|v| (Arc::clone(v), index));
                let res: SegmentResult = match policy.as_ref() {
                    Some(p) => run_with_retry(p, || {
                        segment::download_one_segment(&u, &h, &segment, &st, in_flight_seg.clone(), curl_opts)
                    }),
                    None => segment::download_one_segment(&u, &h, &segment, &st, in_flight_seg, curl_opts),
                };
                let _ = tx.send((index, res));
            }
        }));
    }
    drop(tx);

    let mut first_error: Option<anyhow::Error> = None;
    let mut completed_since_send = 0usize;
    let mut to_receive = count;
    while to_receive > 0 {
        let (index, res) = match rx.recv() {
            Ok(pair) => pair,
            Err(_) => {
                first_error = Some(anyhow::anyhow!(
                    "worker result channel closed (worker may have panicked)"
                ));
                break;
            }
        };
        to_receive -= 1;
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
                    let drained = {
                        let mut q = work.lock().unwrap();
                        let mut n = 0;
                        while q.pop_front().is_some() {
                            n += 1;
                        }
                        n
                    };
                    to_receive = to_receive.saturating_sub(drained);
                }
                if first_error.is_none() {
                    first_error = Some(anyhow::anyhow!("{}", e).context(format!("segment {}", index)));
                }
            }
        }
        if user_abort.load(Ordering::Relaxed) {
            if first_error.is_none() {
                first_error = Some(anyhow::anyhow!(JobAborted));
            }
            break;
        }
    }
    if completed_since_send > 0 {
        if let Some(progress_tx) = progress_tx {
            let _ = progress_tx.try_send(bitmap.to_bytes(segment_count));
        }
    }
    for h in handles {
        if let Err(e) = h.join() {
            if first_error.is_none() {
                first_error = Some(anyhow::anyhow!("worker panicked: {:?}", e));
            }
        }
    }
    if let Some(e) = first_error {
        return Err(e);
    }
    Ok(())
}