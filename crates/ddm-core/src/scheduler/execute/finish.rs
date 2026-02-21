//! Post-download phase: record outcome, sync storage, update metadata, finalize.

use anyhow::Context;
use std::sync::Arc;
use std::time::Duration;

use crate::downloader::DownloadSummary;
use crate::resume_db::{JobMetadata, JobState, ResumeDb};
use crate::segmenter;
use crate::storage;
use crate::host_policy::HostPolicy;

/// After download completes (or is aborted with pause): record host policy outcome,
/// sync storage, update DB metadata, and finalize file + set state if all segments done.
pub(super) async fn finish_after_download(
    db: &ResumeDb,
    job_id: i64,
    job: &crate::resume_db::JobDetails,
    url: &str,
    segment_count_u: usize,
    bytes_this_run: u64,
    download_elapsed: Duration,
    summary: &DownloadSummary,
    bitmap: &segmenter::SegmentBitmap,
    storage_writer: &storage::StorageWriter,
    final_path: &std::path::Path,
    host_policy: Option<&mut HostPolicy>,
    shared_policy: Option<&Arc<tokio::sync::Mutex<HostPolicy>>>,
) -> anyhow::Result<()> {
    if let Some(p) = host_policy {
        p.record_job_outcome(
            url,
            segment_count_u,
            bytes_this_run,
            download_elapsed,
            summary.throttle_events,
            summary.error_events,
        )
        .context("record job outcome for adaptive policy")?;
    } else if let Some(arc) = shared_policy {
        arc.lock()
            .await
            .record_job_outcome(
                url,
                segment_count_u,
                bytes_this_run,
                download_elapsed,
                summary.throttle_events,
                summary.error_events,
            )
            .context("record job outcome for adaptive policy")?;
    }

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
        storage_writer.clone().finalize(final_path)?;
        db.set_state(job_id, JobState::Completed).await?;
        tracing::info!("job {} completed: {}", job_id, final_path.display());
    }

    Ok(())
}
