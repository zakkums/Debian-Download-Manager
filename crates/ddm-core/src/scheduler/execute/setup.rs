//! Setup for execute_download_phase: open storage, reserve budget, start progress loop.

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use crate::config::DdmConfig;
use crate::resume_db::ResumeDb;
use crate::retry::RetryPolicy;
use crate::segmenter;
use crate::storage;

use super::guard::BudgetGuard;
use super::progress_worker::run_progress_persistence_loop;
use crate::scheduler::budget::GlobalConnectionBudget;
use crate::scheduler::progress::ProgressStats;

/// Opens or creates temp storage, reserves connection budget, builds retry policy and
/// curl opts, starts progress persistence loop. Returns all handles and values needed
/// to run the download and then finish.
pub(super) fn setup_storage_and_progress<'a>(
    temp_path: &Path,
    total_size_u: u64,
    segment_count_u: usize,
    segments: &[segmenter::Segment],
    bitmap: &segmenter::SegmentBitmap,
    cfg: &DdmConfig,
    db: &ResumeDb,
    job_id: i64,
    global_budget: Option<&'a GlobalConnectionBudget>,
    progress_tx: Option<&tokio::sync::mpsc::Sender<ProgressStats>>,
) -> Result<(
    storage::StorageWriter,
    usize,
    RetryPolicy,
    crate::downloader::CurlOptions,
    u64,
    Instant,
    tokio::task::JoinHandle<()>,
    tokio::sync::mpsc::Sender<Vec<u8>>,
    Arc<Vec<std::sync::atomic::AtomicU64>>,
    Option<BudgetGuard<'a>>,
)> {
    let storage_writer = if temp_path.exists() {
        storage::StorageWriter::open_existing(temp_path)
            .with_context(|| format!("open existing temp file: {}", temp_path.display()))?
    } else {
        let mut builder = storage::StorageWriterBuilder::create(temp_path)
            .with_context(|| format!("create temp file: {}", temp_path.display()))?;
        builder.preallocate(total_size_u)?;
        builder.build()
    };

    let max_concurrent = (cfg.max_connections_per_host)
        .min(cfg.max_total_connections)
        .min(segment_count_u);
    let actual_concurrent = match global_budget {
        Some(b) => b.reserve(max_concurrent),
        None => max_concurrent,
    };
    let budget_guard = global_budget.map(|b| BudgetGuard {
        budget: b,
        reserved: actual_concurrent,
    });
    let retry_policy = cfg
        .retry
        .as_ref()
        .map(|r| RetryPolicy {
            max_attempts: r.max_attempts,
            base_delay: std::time::Duration::from_secs_f64(r.base_delay_secs),
            max_delay: std::time::Duration::from_secs(r.max_delay_secs),
        })
        .unwrap_or_else(RetryPolicy::default);

    let curl_opts = crate::downloader::CurlOptions::per_handle(
        cfg.max_bytes_per_sec,
        actual_concurrent,
        cfg.segment_buffer_bytes,
    );
    let bytes_this_run: u64 = segments
        .iter()
        .enumerate()
        .filter(|(i, _)| !bitmap.is_completed(*i))
        .map(|(_, s)| s.end - s.start)
        .sum();
    let download_start = Instant::now();

    let in_flight_bytes: Arc<Vec<std::sync::atomic::AtomicU64>> = Arc::new(
        (0..segment_count_u)
            .map(|_| std::sync::atomic::AtomicU64::new(0))
            .collect(),
    );
    let (bitmap_tx, progress_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(8);
    let progress_handle = tokio::spawn(run_progress_persistence_loop(
        progress_rx,
        db.clone(),
        job_id,
        segment_count_u,
        segments.to_vec(),
        total_size_u,
        progress_tx.cloned(),
        Arc::clone(&in_flight_bytes),
        download_start,
    ));

    Ok((
        storage_writer,
        actual_concurrent,
        retry_policy,
        curl_opts,
        bytes_this_run,
        download_start,
        progress_handle,
        bitmap_tx,
        in_flight_bytes,
        budget_guard,
    ))
}
