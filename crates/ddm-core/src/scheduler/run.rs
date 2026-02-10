//! Run one job: probe, validate, then download only missing segments.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

use crate::config::DdmConfig;
use crate::downloader;
use crate::fetch_head;
use crate::retry::RetryPolicy;
use crate::resume_db::{JobMetadata, JobState, ResumeDb};
use crate::safe_resume;
use crate::segmenter;
use crate::storage;
use crate::url_model;

/// Runs a single job: re-validates with HEAD, then downloads only incomplete segments.
///
/// If `force_restart` is true and the remote has changed, metadata and bitmap
/// are reset from the new HEAD and the full file is re-downloaded.
pub async fn run_one_job(
    db: &ResumeDb,
    job_id: i64,
    force_restart: bool,
    cfg: &DdmConfig,
    download_dir: &Path,
) -> Result<()> {
    let mut job = db
        .get_job(job_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("job {} not found", job_id))?;

    let url = job.url.clone();
    let headers: HashMap<String, String> = HashMap::new();

    let head = tokio::task::spawn_blocking({
        let url = url.clone();
        let headers = headers.clone();
        move || fetch_head::probe(&url, &headers)
    })
    .await
    .context("probe task join")?
    .context("HEAD request failed")?;

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

    let segment_count = choose_segment_count(total_size, cfg);
    let final_name =
        url_model::derive_filename(&url, head.content_disposition.as_deref());
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

    let temp_path = download_dir.join(
        job.temp_filename
            .as_deref()
            .unwrap_or(&temp_name_str),
    );
    let final_path = download_dir.join(
        job.final_filename
            .as_deref()
            .unwrap_or(&final_name),
    );

    db.set_state(job_id, JobState::Running).await?;

    let storage_writer = if temp_path.exists() {
        storage::StorageWriter::open_existing(&temp_path)
            .with_context(|| format!("open existing temp file: {}", temp_path.display()))?
    } else {
        let mut builder = storage::StorageWriterBuilder::create(&temp_path)
            .with_context(|| format!("create temp file: {}", temp_path.display()))?;
        builder.preallocate(total_size_u)?;
        builder.build()
    };

    let max_concurrent = (cfg.max_connections_per_host)
        .min(cfg.max_total_connections)
        .min(segment_count_u);
    let retry_policy = RetryPolicy::default();
    let bitmap = {
        let url = url.clone();
        let headers = headers.clone();
        let segments = segments.clone();
        let storage = storage_writer.clone();
        let max_concurrent = max_concurrent;
        let policy = retry_policy;
        tokio::task::spawn_blocking(move || -> Result<segmenter::SegmentBitmap> {
            downloader::download_segments(
                &url,
                &headers,
                &segments,
                &storage,
                &mut bitmap,
                Some(max_concurrent),
                Some(&policy),
            )?;
            Ok(bitmap)
        })
        .await
        .context("download task join")??
    };

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
        storage_writer.finalize(&final_path)?;
        db.set_state(job_id, JobState::Completed).await?;
        tracing::info!("job {} completed: {}", job_id, final_path.display());
    }

    Ok(())
}

fn choose_segment_count(total_size: u64, cfg: &DdmConfig) -> usize {
    let n = cfg.min_segments.max(1).min(cfg.max_segments);
    if total_size == 0 {
        return n;
    }
    n.min(total_size as usize)
}

/// Runs the next queued job (smallest id first, FIFO). Returns true if a job was run, false if none queued.
pub async fn run_next_job(
    db: &ResumeDb,
    force_restart: bool,
    cfg: &DdmConfig,
    download_dir: &Path,
) -> Result<bool> {
    let jobs = db.list_jobs().await?;
    let next = jobs
        .into_iter()
        .filter(|j| j.state == JobState::Queued)
        .min_by_key(|j| j.id)
        .map(|j| j.id);
    let Some(job_id) = next else {
        return Ok(false);
    };
    run_one_job(db, job_id, force_restart, cfg, download_dir).await?;
    Ok(true)
}
