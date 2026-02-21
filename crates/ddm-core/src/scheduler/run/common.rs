//! Shared helpers for single and parallel job run (filename resolution, paths).

use anyhow::Result;
use std::path::Path;

use crate::resume_db::ResumeDb;
use crate::storage;
use crate::url_model;

/// Resolve final and temp filenames and whether metadata must be (re)fetched.
/// Uses job's download_dir or `download_dir`; checks DB for existing names to avoid collisions.
pub async fn resolve_filenames(
    db: &ResumeDb,
    job_id: i64,
    job: &crate::resume_db::JobDetails,
    head: &crate::fetch_head::HeadResult,
    force_restart: bool,
    validation_failed: bool,
    download_dir: &Path,
) -> Result<(String, String, bool)> {
    let candidate_name =
        url_model::derive_filename(&job.url, head.content_disposition.as_deref());
    let effective_dir_str = job
        .settings
        .download_dir
        .as_deref()
        .or_else(|| download_dir.to_str());
    let final_name = if job.total_size.is_none() || force_restart || validation_failed {
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
    let needs_metadata = job.total_size.is_none() || force_restart || validation_failed;
    Ok((final_name, temp_name_str, needs_metadata))
}

/// Build temp and final paths from job and names; error if final exists and overwrite is false.
pub fn paths_and_overwrite_check(
    job: &crate::resume_db::JobDetails,
    final_name: &str,
    temp_name_str: &str,
    download_dir: &Path,
    overwrite: bool,
) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
    let effective_dir = job
        .settings
        .download_dir
        .as_deref()
        .map(std::path::Path::new)
        .unwrap_or(download_dir);
    let temp_path = effective_dir.join(
        job.temp_filename
            .as_deref()
            .unwrap_or(temp_name_str),
    );
    let final_path = effective_dir.join(
        job.final_filename
            .as_deref()
            .unwrap_or(final_name),
    );
    if final_path.exists() && !overwrite {
        anyhow::bail!(
            "final file already exists: {} (use --overwrite to replace)",
            final_path.display()
        );
    }
    Ok((temp_path, final_path))
}
