//! `ddm add <url>` – add a new download job.

use anyhow::Result;
use ddm_core::resume_db::{JobSettings, ResumeDb};
use std::path::Path;

/// Adds a job for the given URL. If `download_dir` is None, the job will use
/// the current directory at run time (legacy behavior).
pub async fn run_add(db: &ResumeDb, url: &str, download_dir: Option<&Path>) -> Result<()> {
    let mut settings = JobSettings::default();
    if let Some(dir) = download_dir {
        settings.download_dir = Some(dir.to_string_lossy().to_string());
    }
    let id = db.add_job(url, &settings).await?;
    println!("Added job {id} for URL: {url}");
    Ok(())
}
