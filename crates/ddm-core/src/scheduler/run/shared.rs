//! Run a single job with shared host policy (parallel scheduler).

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::config::DdmConfig;
use crate::fetch_head;
use crate::resume_db::{JobMetadata, JobState, ResumeDb};
use crate::safe_resume;
use crate::segmenter;
use crate::host_policy::HostPolicy;
use crate::control::JobControl;

use super::super::budget::GlobalConnectionBudget;
use super::super::choose;
use super::super::execute;
use super::super::progress::ProgressStats;

/// Like `run_one_job` but uses a shared `Arc<Mutex<HostPolicy>>` and optional
/// `Arc<GlobalConnectionBudget>>` so multiple jobs can run concurrently.
/// Used by the parallel scheduler.
pub async fn run_one_job_shared(
    db: &ResumeDb,
    job_id: i64,
    force_restart: bool,
    overwrite: bool,
    cfg: &DdmConfig,
    download_dir: &Path,
    host_policy: Arc<tokio::sync::Mutex<HostPolicy>>,
    progress_tx: Option<tokio::sync::mpsc::Sender<ProgressStats>>,
    global_budget: Option<Arc<GlobalConnectionBudget>>,
    job_control: Option<Arc<JobControl>>,
) -> Result<()> {
    let mut job = db
        .get_job(job_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("job {} not found", job_id))?;

    let url = job.url.clone();
    let headers: HashMap<String, String> = job
        .settings
        .custom_headers
        .clone()
        .unwrap_or_default();

    let head = tokio::task::spawn_blocking({
        let url = url.clone();
        let headers = headers.clone();
        move || fetch_head::probe_best_effort(&url, &headers)
    })
    .await
    .context("probe task join")?
    .context("probe failed")?;

    {
        let mut policy = host_policy.lock().await;
        policy
            .record_head_result(&url, &head)
            .context("update host policy from HEAD")?;
    }

    let validation = safe_resume::validate_for_resume(&job, &head);
    if let Err(ref e) = validation {
        if !force_restart {
            return Err(anyhow::anyhow!("{}", e));
        }
        tracing::info!("force-restart: discarding progress and re-downloading (remote changed)");
    }

    let (final_name, temp_name_str, needs_metadata) = super::common::resolve_filenames(
        db, job_id, &job, &head, force_restart, validation.is_err(), download_dir,
    )
    .await?;

    let segmentable = head.accept_ranges && head.content_length.is_some();
    if !segmentable {
        return super::fallback::run_single_stream(
            db,
            job_id,
            &mut job,
            &url,
            &headers,
            &head,
            overwrite,
            cfg,
            download_dir,
            &final_name,
            &temp_name_str,
            needs_metadata,
        )
        .await;
    }

    let total_size = head
        .content_length
        .ok_or_else(|| anyhow::anyhow!("server did not send Content-Length"))?;
    let segment_count = {
        let policy = host_policy.lock().await;
        choose::choose_segment_count(total_size, cfg, &url, &policy)
    };

    if needs_metadata {
        let bitmap = segmenter::SegmentBitmap::new(segment_count);
        let meta = JobMetadata {
            final_filename: Some(final_name.clone()),
            temp_filename: Some(temp_name_str.clone()),
            total_size: Some(total_size as i64),
            etag: head.etag.clone(),
            last_modified: head.last_modified.clone(),
            segment_count: segment_count as i64,
            completed_bitmap: bitmap.to_bytes(segment_count),
        };
        db.update_metadata(job_id, &meta).await?;
        job = db.get_job(job_id).await?.expect("job exists after update");
    }

    let total_size_u = job.total_size.unwrap() as u64;
    let segment_count_u = job.segment_count as usize;
    let segments = segmenter::plan_segments(total_size_u, segment_count_u);
    let mut bitmap =
        segmenter::SegmentBitmap::from_bytes(&job.completed_bitmap, segment_count_u);

    let (temp_path, final_path) = super::common::paths_and_overwrite_check(
        &job, &final_name, &temp_name_str, download_dir, overwrite,
    )?;

    db.set_state(job_id, JobState::Running).await?;

    let abort = job_control.as_ref().map(|c| c.register(job_id));
    let run_result = execute::execute_download_phase(
        db,
        job_id,
        &job,
        &url,
        &headers,
        needs_metadata,
        &temp_path,
        &final_path,
        total_size_u,
        segment_count_u,
        &segments,
        &mut bitmap,
        cfg,
        None,
        Some(Arc::clone(&host_policy)),
        progress_tx.as_ref(),
        global_budget.as_deref(),
        abort,
    )
    .await;
    if let Some(ref c) = job_control {
        c.unregister(job_id);
    }

    if let Err(ref e) = &run_result {
        if e.downcast_ref::<crate::control::JobAborted>().is_none() {
            let _ = db.set_state(job_id, JobState::Error).await;
        }
    }
    run_result
}
