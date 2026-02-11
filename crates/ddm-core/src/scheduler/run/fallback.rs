//! Fallback execution paths for servers without usable Range/length metadata.

use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

use crate::config::DdmConfig;
use crate::fetch_head::HeadResult;
use crate::downloader::CurlOptions;
use crate::resume_db::{JobDetails, JobMetadata, JobState, ResumeDb};
use crate::scheduler::execute;

/// Runs a single-stream GET download for a job (non-Range fallback) and completes the job.
/// Intended to be called from the scheduler run paths after a metadata probe.
pub(super) async fn run_single_stream(
    db: &ResumeDb,
    job_id: i64,
    job: &mut JobDetails,
    url: &str,
    headers: &HashMap<String, String>,
    head: &HeadResult,
    overwrite: bool,
    cfg: &DdmConfig,
    default_download_dir: &Path,
    final_name: &str,
    temp_name_str: &str,
    needs_metadata: bool,
) -> Result<()> {
    if needs_metadata {
        let meta = JobMetadata {
            final_filename: Some(final_name.to_string()),
            temp_filename: Some(temp_name_str.to_string()),
            total_size: head.content_length.map(|n| n as i64),
            etag: head.etag.clone(),
            last_modified: head.last_modified.clone(),
            segment_count: 0,
            completed_bitmap: Vec::new(),
        };
        db.update_metadata(job_id, &meta).await?;
        *job = db.get_job(job_id).await?.expect("job exists after update");
    }

    let effective_dir = job
        .settings
        .download_dir
        .as_deref()
        .map(Path::new)
        .unwrap_or(default_download_dir);
    let temp_path = effective_dir.join(job.temp_filename.as_deref().unwrap_or(temp_name_str));
    let final_path = effective_dir.join(job.final_filename.as_deref().unwrap_or(final_name));

    if final_path.exists() && !overwrite {
        anyhow::bail!(
            "final file already exists: {} (use --overwrite to replace)",
            final_path.display()
        );
    }

    db.set_state(job_id, JobState::Running).await?;
    let curl = CurlOptions::per_handle(cfg.max_bytes_per_sec, 1, cfg.segment_buffer_bytes);
    let bytes_written = execute::execute_single_download_phase(
        db,
        job_id,
        url,
        headers,
        &temp_path,
        &final_path,
        head.content_length,
        curl,
    )
    .await?;

    if job.total_size.is_none() {
        let meta = JobMetadata {
            final_filename: Some(final_name.to_string()),
            temp_filename: Some(temp_name_str.to_string()),
            total_size: Some(bytes_written as i64),
            etag: head.etag.clone(),
            last_modified: head.last_modified.clone(),
            segment_count: 0,
            completed_bitmap: Vec::new(),
        };
        db.update_metadata(job_id, &meta).await?;
    }

    Ok(())
}

