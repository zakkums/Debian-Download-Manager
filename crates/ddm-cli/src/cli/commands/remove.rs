//! `ddm remove <id>` – remove a job; optionally delete its files with --delete-files.

use anyhow::Result;
use ddm_core::resume_db::ResumeDb;
use std::path::Path;

/// Removes the job from the DB. If `delete_files` is true, deletes the job's
/// .part and final file(s) from the job's stored download_dir (or `download_dir`
/// / current directory if the job has none).
pub async fn run_remove(
    db: &ResumeDb,
    id: i64,
    delete_files: bool,
    download_dir: Option<&Path>,
) -> Result<()> {
    if delete_files {
        let job = db.get_job(id).await?;
        let dir = job
            .as_ref()
            .and_then(|j| j.settings.download_dir.as_deref())
            .map(Path::new)
            .or(download_dir)
            .unwrap_or_else(|| Path::new("."));
        if let Some(ref j) = job {
            for name in [&j.temp_filename, &j.final_filename].into_iter().flatten() {
                let path = dir.join(name);
                match tokio::fs::remove_file(&path).await {
                    Ok(()) => tracing::debug!(path = %path.display(), "deleted file"),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => tracing::warn!(path = %path.display(), "could not delete file: {}", e),
                }
            }
        }
    }

    db.remove_job(id).await?;
    println!("Removed job {id}");
    Ok(())
}
