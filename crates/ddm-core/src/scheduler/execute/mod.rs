//! Execute the download phase of a single job: storage, segments, progress, finalize.

mod finish;
mod guard;
mod invoke;
mod progress_worker;
mod run_download;
mod setup;
mod single;

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use crate::config::{DdmConfig, DownloadBackend};
use crate::control::JobAborted;
use crate::host_policy::HostPolicy;
use crate::resume_db::{JobState, ResumeDb};
use crate::segmenter;

pub(super) use self::single::execute_single_download_phase;
use crate::scheduler::budget::GlobalConnectionBudget;
use crate::scheduler::progress::ProgressStats;

use self::invoke::run_download_blocking_async;
use self::setup::setup_storage_and_progress;

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
    shared_policy: Option<Arc<tokio::sync::Mutex<HostPolicy>>>,
    progress_tx: Option<&tokio::sync::mpsc::Sender<ProgressStats>>,
    global_budget: Option<&GlobalConnectionBudget>,
    abort: Option<Arc<std::sync::atomic::AtomicBool>>,
) -> Result<()> {
    if needs_metadata && temp_path.exists() {
        tokio::fs::remove_file(temp_path).await.with_context(|| {
            format!(
                "remove temp file for force-restart: {}",
                temp_path.display()
            )
        })?;
        tracing::debug!(path = %temp_path.display(), "removed existing .part for clean restart");
    }

    let (
        storage_writer,
        actual_concurrent,
        retry_policy,
        curl_opts,
        bytes_this_run,
        download_start,
        progress_handle,
        bitmap_tx,
        in_flight_bytes,
        _budget_guard,
    ): (_, _, _, _, _, Instant, tokio::task::JoinHandle<()>, _, _, _) = setup_storage_and_progress(
        temp_path,
        total_size_u,
        segment_count_u,
        segments,
        bitmap,
        cfg,
        db,
        job_id,
        global_budget,
        progress_tx,
    )?;

    let use_multi = cfg.download_backend == Some(DownloadBackend::Multi);
    let download_result = run_download_blocking_async(
        url,
        headers,
        segments,
        &storage_writer,
        bitmap,
        actual_concurrent,
        &retry_policy,
        bitmap_tx,
        in_flight_bytes,
        abort,
        use_multi,
        curl_opts,
    )
    .await;

    let (bitmap_result, summary) = match download_result {
        Ok((bm, s)) => (bm, s),
        Err(e) => {
            if e.downcast_ref::<JobAborted>().is_some() {
                let _ = progress_handle.await;
                db.set_state(job_id, JobState::Paused).await?;
                tracing::info!("job {} paused by user", job_id);
                return Ok(());
            }
            return Err(e);
        }
    };

    *bitmap = bitmap_result;
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
