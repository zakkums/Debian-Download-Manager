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
use crate::storage;
use crate::url_model;
use crate::host_policy::HostPolicy;

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
        move || fetch_head::probe(&url, &headers)
    })
    .await
    .context("probe task join")?
    .context("HEAD request failed")?;

    {
        let mut policy = host_policy.lock().await;
        policy
            .record_head_result(&url, &head)
            .context("update host policy from HEAD")?;
    }

    if !head.accept_ranges {
        anyhow::bail!("server does not support Range requests (Accept-Ranges: bytes)");
    }

    let total_size = head
        .content_length
        .ok_or_else(|| anyhow::anyhow!("server did not send Content-Length"))?;

    let validation = safe_resume::validate_for_resume(&job, &head);
    if let Err(ref e) = validation {
        if !force_restart {
            return Err(anyhow::anyhow!("{}", e));
        }
        tracing::info!("force-restart: discarding progress and re-downloading (remote changed)");
    }

    let segment_count = {
        let policy = host_policy.lock().await;
        choose::choose_segment_count(total_size, cfg, &url, &policy)
    };

    let candidate_name =
        url_model::derive_filename(&url, head.content_disposition.as_deref());
    let effective_dir_str = job
        .settings
        .download_dir
        .as_deref()
        .or_else(|| download_dir.to_str());
    let final_name = if job.total_size.is_none() || force_restart || validation.is_err() {
        let existing = db
            .list_final_filenames_in_dir(effective_dir_str, Some(job_id))
            .await?;
        url_model::unique_filename_among(&candidate_name, &existing)
    } else {
        job.final_filename
            .as_deref()
            .unwrap_or(&candidate_name)
            .to_string()
    };
    let temp_name = storage::temp_path(Path::new(&final_name));
    let temp_name_str = temp_name.to_string_lossy().to_string();

    let needs_metadata = job.total_size.is_none()
        || force_restart
        || validation.is_err();

    if needs_metadata {
        let _segments = segmenter::plan_segments(total_size, segment_count);
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

    let effective_dir = job
        .settings
        .download_dir
        .as_deref()
        .map(std::path::Path::new)
        .unwrap_or(download_dir);
    let temp_path = effective_dir.join(
        job.temp_filename
            .as_deref()
            .unwrap_or(&temp_name_str),
    );
    let final_path = effective_dir.join(
        job.final_filename
            .as_deref()
            .unwrap_or(&final_name),
    );

    if final_path.exists() && !overwrite {
        anyhow::bail!(
            "final file already exists: {} (use --overwrite to replace)",
            final_path.display()
        );
    }

    db.set_state(job_id, JobState::Running).await?;

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
    )
    .await;

    if run_result.is_err() {
        let _ = db.set_state(job_id, JobState::Error).await;
    }
    run_result
}
