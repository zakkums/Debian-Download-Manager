//! Execute the download phase of a single job: storage, segments, progress, finalize.

mod finish;
mod guard;
mod progress_worker;
mod run_download;
mod single;

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::{DdmConfig, DownloadBackend};
use crate::control::JobAborted;
use crate::downloader::DownloadSummary;
use crate::resume_db::{JobState, ResumeDb};
use crate::retry::RetryPolicy;
use crate::segmenter;
use crate::storage;
use crate::host_policy::HostPolicy;

use self::guard::BudgetGuard;
use self::progress_worker::run_progress_persistence_loop;
use self::run_download::run_download_blocking;
pub(super) use self::single::execute_single_download_phase;
use crate::scheduler::budget::GlobalConnectionBudget;
use crate::scheduler::progress::ProgressStats;

/// Runs the download phase: open/create storage, download incomplete segments,
/// persist progress, update metadata, and finalize if complete.
/// If `progress_tx` is `Some`, progress stats (bytes done, elapsed) are sent
/// when the bitmap is updated so the caller can show ETA/rate.
pub(super) async fn execute_download_phase(
    db: &ResumeDb,
    job_id: i64,
    job: &crate::resume_db::JobDetails,
    url: &str,
    headers: &HashMap<String, String>,
    needs_metadata: bool,
    temp_path: &Path,
    final_path: &Path,
    total_size_u: u64,
    segment_count_u: usize,
    segments: &[segmenter::Segment],
    bitmap: &mut segmenter::SegmentBitmap,
    cfg: &DdmConfig,
    host_policy: Option<&mut HostPolicy>,
    shared_policy: Option<std::sync::Arc<tokio::sync::Mutex<HostPolicy>>>,
    progress_tx: Option<&tokio::sync::mpsc::Sender<ProgressStats>>,
    global_budget: Option<&GlobalConnectionBudget>,
    abort: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> Result<()> {
    if needs_metadata && temp_path.exists() {
        tokio::fs::remove_file(temp_path)
            .await
            .with_context(|| format!("remove temp file for force-restart: {}", temp_path.display()))?;
        tracing::debug!(path = %temp_path.display(), "removed existing .part for clean restart");
    }

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
    let _budget_guard: Option<BudgetGuard<'_>> = global_budget.map(|b| BudgetGuard {
        budget: b,
        reserved: actual_concurrent,
    });
    let retry_policy = cfg.retry.as_ref().map(|r| RetryPolicy {
        max_attempts: r.max_attempts,
        base_delay: Duration::from_secs_f64(r.base_delay_secs),
        max_delay: Duration::from_secs(r.max_delay_secs),
    }).unwrap_or_else(RetryPolicy::default);

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
        (0..segment_count_u).map(|_| std::sync::atomic::AtomicU64::new(0)).collect(),
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

    let mut bitmap_copy = bitmap.clone();
    let use_multi = cfg.download_backend == Some(DownloadBackend::Multi);
    let download_result = {
        let url = url.to_string();
        let headers = headers.clone();
        let segments = segments.to_vec();
        let storage = storage_writer.clone();
        let max_concurrent = actual_concurrent;
        let policy = retry_policy;
        let tx = bitmap_tx.clone();
        let in_flight = Arc::clone(&in_flight_bytes);
        let abort_clone = abort.clone();
        let curl = curl_opts;
        tokio::task::spawn_blocking(move || -> Result<(segmenter::SegmentBitmap, DownloadSummary)> {
            let mut summary = DownloadSummary::default();
            run_download_blocking(
                &url,
                &headers,
                &segments,
                &storage,
                &mut bitmap_copy,
                max_concurrent,
                &policy,
                &mut summary,
                Some(&tx),
                Some(in_flight),
                abort_clone,
                use_multi,
                curl,
            )?;
            Ok((bitmap_copy, summary))
        })
        .await
        .context("download task join")?
    };

    let (bitmap_result, summary) = match download_result {
        Ok((bm, s)) => (bm, s),
        Err(e) => {
            if e.downcast_ref::<JobAborted>().is_some() {
                drop(bitmap_tx);
                let _ = progress_handle.await;
                db.set_state(job_id, JobState::Paused).await?;
                tracing::info!("job {} paused by user", job_id);
                return Ok(());
            }
            return Err(e);
        }
    };

    *bitmap = bitmap_result;

    drop(bitmap_tx);
    progress_handle.await.context("progress writer join")?;
    let download_elapsed = download_start.elapsed();
    finish::finish_after_download(
        db,
        job_id,
        job,
        url,
        segment_count_u,
        bytes_this_run,
        download_elapsed,
        &summary,
        bitmap,
        &storage_writer,
        final_path,
        host_policy,
        shared_policy.as_ref(),
    )
    .await?;

    Ok(())
}
