//! Run one job or the next queued job; supports sequential and shared (parallel) policy.

mod common;
mod fallback;
mod shared;
mod single;

use anyhow::Result;
use std::path::Path;

use crate::config::DdmConfig;
use crate::resume_db::{JobState, ResumeDb};
use crate::host_policy::HostPolicy;

use super::budget::GlobalConnectionBudget;
use super::progress::ProgressStats;

pub use shared::run_one_job_shared;
pub use single::run_one_job;

/// Returns the id of the next queued job (smallest id first), or None if none queued.
pub async fn next_queued_job_id(db: &ResumeDb) -> Result<Option<i64>> {
    let jobs = db.list_jobs().await?;
    let next = jobs
        .into_iter()
        .filter(|j| j.state == JobState::Queued)
        .min_by_key(|j| j.id)
        .map(|j| j.id);
    Ok(next)
}

/// Runs the next queued job (smallest id first, FIFO). Returns true if a job was run, false if none queued.
/// If `progress_tx` is `Some`, progress stats are sent during the download.
/// If `job_control` is `Some`, the job can be paused via the control socket.
pub async fn run_next_job(
    db: &ResumeDb,
    force_restart: bool,
    overwrite: bool,
    cfg: &DdmConfig,
    download_dir: &Path,
    host_policy: &mut HostPolicy,
    progress_tx: Option<&tokio::sync::mpsc::Sender<ProgressStats>>,
    global_budget: Option<&GlobalConnectionBudget>,
    job_control: Option<std::sync::Arc<crate::control::JobControl>>,
) -> Result<bool> {
    let Some(job_id) = next_queued_job_id(db).await? else {
        return Ok(false);
    };
    run_one_job(
        db,
        job_id,
        force_restart,
        overwrite,
        cfg,
        download_dir,
        host_policy,
        progress_tx,
        global_budget,
        job_control,
    )
    .await?;
    Ok(true)
}
