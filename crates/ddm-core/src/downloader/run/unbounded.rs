use anyhow::Result;
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use crate::downloader::{CurlOptions, DownloadSummary, SegmentResult};
use crate::downloader::segment;
use crate::retry::{classify, run_with_retry, ErrorKind, RetryPolicy};
use crate::segmenter::{Segment, SegmentBitmap};
use crate::storage::StorageWriter;

/// Run incomplete segments with one thread per segment (unbounded parallelism).
pub fn run_unbounded(
    url: String,
    headers: HashMap<String, String>,
    storage: StorageWriter,
    incomplete: Vec<(usize, Segment)>,
    segment_count: usize,
    retry_policy: Option<RetryPolicy>,
    bitmap: &mut SegmentBitmap,
    summary_out: &mut DownloadSummary,
    progress_tx: Option<&tokio::sync::mpsc::Sender<Vec<u8>>>,
    in_flight_bytes: Option<Arc<Vec<AtomicU64>>>,
    curl: CurlOptions,
) -> Result<()> {
    let results: Vec<(usize, SegmentResult)> = incomplete
        .into_iter()
        .map(|(index, segment)| {
            let u = url.clone();
            let h = headers.clone();
            let st = storage.clone();
            let policy = retry_policy.clone();
            let curl_opts = curl;
            let in_flight = in_flight_bytes.as_ref().map(|v| (Arc::clone(v), index));
            let res = std::thread::spawn(move || {
                match policy.as_ref() {
                    Some(p) => run_with_retry(p, || {
                        segment::download_one_segment(&u, &h, &segment, &st, in_flight.clone(), curl_opts)
                    }),
                    None => segment::download_one_segment(&u, &h, &segment, &st, in_flight, curl_opts),
                }
            })
            .join()
            .unwrap_or_else(|e| panic!("worker panicked: {:?}", e));
            (index, res)
        })
        .collect();

    let mut first_error: Option<anyhow::Error> = None;
    let mut completed_since_send = 0usize;
    for (index, res) in results {
        match res {
            Ok(()) => {
                bitmap.set_completed(index);
                completed_since_send += 1;
                if let Some(tx) = progress_tx {
                    if completed_since_send >= super::COALESCE_PROGRESS_EVERY {
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

