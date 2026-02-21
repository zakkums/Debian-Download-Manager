//! Execute a non-segmented single-stream download (fallback for non-Range servers).

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

use crate::downloader;
use crate::downloader::CurlOptions;
use crate::resume_db::{JobState, ResumeDb};
use crate::storage;

/// Runs a single-stream GET download: (re)create temp file, stream bytes, sync, finalize, set Completed.
/// Returns bytes written.
pub(crate) async fn execute_single_download_phase(
    db: &ResumeDb,
    job_id: i64,
    url: &str,
    headers: &HashMap<String, String>,
    temp_path: &Path,
    final_path: &Path,
    expected_len: Option<u64>,
    curl: CurlOptions,
) -> Result<u64> {
    if temp_path.exists() {
        tokio::fs::remove_file(temp_path)
            .await
            .with_context(|| format!("remove existing temp file: {}", temp_path.display()))?;
    }

    let mut builder = storage::StorageWriterBuilder::create(temp_path)
        .with_context(|| format!("create temp file: {}", temp_path.display()))?;
    if let Some(n) = expected_len {
        builder.preallocate(n)?;
    }
    let storage_writer = builder.build();

    let bytes_written = tokio::task::spawn_blocking({
        let url = url.to_string();
        let headers = headers.clone();
        let storage = storage_writer.clone();
        move || -> Result<u64> {
            downloader::download_single(&url, &headers, &storage, expected_len, curl)
        }
    })
    .await
    .context("download task join")??;

    storage_writer.sync()?;
    storage_writer.finalize(final_path)?;
    db.set_state(job_id, JobState::Completed).await?;
    tracing::info!(
        "job {} completed (single): {}",
        job_id,
        final_path.display()
    );

    Ok(bytes_written)
}
