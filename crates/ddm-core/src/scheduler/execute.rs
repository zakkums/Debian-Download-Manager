//! Execute the download phase of a single job: storage, segments, progress, finalize.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use crate::config::DdmConfig;
use crate::downloader::DownloadSummary;
use crate::downloader;
use crate::resume_db::{JobMetadata, JobState, ResumeDb};
use crate::retry::RetryPolicy;
use crate::segmenter;
use crate::storage;
use crate::host_policy::HostPolicy;

use super::budget::GlobalConnectionBudget;
use super::progress::ProgressStats;

/// Releases reserved connections when dropped.
struct BudgetGuard<'a> {
    budget: &'a GlobalConnectionBudget,
    reserved: usize,
}

impl Drop for BudgetGuard<'_> {
    fn drop(&mut self) {
        self.budget.release(self.reserved);
    }
}

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
    host_policy: &mut HostPolicy,
    progress_tx: Option<&tokio::sync::mpsc::Sender<ProgressStats>>,
    global_budget: Option<&GlobalConnectionBudget>,
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
    let _budget_guard = global_budget.map(|b| BudgetGuard {
        budget: b,
        reserved: actual_concurrent,
    });
    let retry_policy = RetryPolicy::default();
    let bytes_this_run: u64 = segments
        .iter()
        .enumerate()
        .filter(|(i, _)| !bitmap.is_completed(*i))
        .map(|(_, s)| s.end - s.start)
        .sum();
    let download_start = Instant::now();

    let (bitmap_tx, mut progress_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(8);
    let db_clone = db.clone();
    let segments_vec = segments.to_vec();
    let stats_tx = progress_tx.cloned();
    let download_start = download_start;
    let progress_handle = tokio::spawn(async move {
        while let Some(blob) = progress_rx.recv().await {
            if db_clone.update_bitmap(job_id, &blob).await.is_err() {
                tracing::warn!(job_id, "durable progress update failed");
            }
            if let Some(ref tx) = stats_tx {
                let bitmap = segmenter::SegmentBitmap::from_bytes(&blob, segment_count_u);
                let bytes_done: u64 = segments_vec
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| bitmap.is_completed(*i))
                    .map(|(_, s)| s.end - s.start)
                    .sum();
                let elapsed_secs = download_start.elapsed().as_secs_f64();
                let segments_done = (0..segment_count_u).filter(|i| bitmap.is_completed(*i)).count();
                let stats = ProgressStats {
                    bytes_done,
                    total_bytes: total_size_u,
                    elapsed_secs,
                    segments_done,
                    segment_count: segment_count_u,
                };
                let _ = tx.try_send(stats);
            }
        }
    });

    let mut bitmap_copy = bitmap.clone();
    let (bitmap_result, summary) = {
        let url = url.to_string();
        let headers = headers.clone();
        let segments = segments.to_vec();
        let storage = storage_writer.clone();
        let max_concurrent = actual_concurrent.max(1);
        let policy = retry_policy;
        let tx = bitmap_tx.clone();
        tokio::task::spawn_blocking(move || -> Result<(segmenter::SegmentBitmap, DownloadSummary)> {
            let mut summary = DownloadSummary::default();
            downloader::download_segments(
                &url,
                &headers,
                &segments,
                &storage,
                &mut bitmap_copy,
                Some(max_concurrent),
                Some(&policy),
                &mut summary,
                Some(&tx),
            )?;
            Ok((bitmap_copy, summary))
        })
        .await
        .context("download task join")??
    };

    *bitmap = bitmap_result;

    drop(bitmap_tx);
    progress_handle.await.context("progress writer join")?;
    let download_elapsed = download_start.elapsed();
    host_policy
        .record_job_outcome(
            url,
            segment_count_u,
            bytes_this_run,
            download_elapsed,
            summary.throttle_events,
            summary.error_events,
        )
        .context("record job outcome for adaptive policy")?;

    storage_writer.sync()?;

    let meta = JobMetadata {
        final_filename: job.final_filename.clone(),
        temp_filename: job.temp_filename.clone(),
        total_size: job.total_size,
        etag: job.etag.clone(),
        last_modified: job.last_modified.clone(),
        segment_count: job.segment_count,
        completed_bitmap: bitmap.to_bytes(segment_count_u),
    };
    db.update_metadata(job_id, &meta).await?;

    if bitmap.all_completed(segment_count_u) {
        storage_writer.finalize(final_path)?;
        db.set_state(job_id, JobState::Completed).await?;
        tracing::info!("job {} completed: {}", job_id, final_path.display());
    }

    Ok(())
}
