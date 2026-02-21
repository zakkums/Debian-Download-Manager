//! Run multiple jobs concurrently using the global connection budget.
//!
//! Keeps up to `max_concurrent` jobs running at once; when one finishes,
//! the next queued job is started until the queue is empty.

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

use crate::config::DdmConfig;
use crate::host_policy::HostPolicy;
use crate::resume_db::ResumeDb;

use super::budget::GlobalConnectionBudget;
use super::progress::ProgressStats;
use super::run::run_one_job_shared;

/// Runs queued jobs with up to `max_concurrent` jobs in flight at once.
/// Uses a shared `Arc<Mutex<HostPolicy>>` and `Arc<GlobalConnectionBudget>>`
/// so jobs share limits correctly. Progress from any job is sent to `progress_tx`.
///
/// Replaces `host_policy` with a temporary for the run and restores the
/// updated policy when done (so the caller can save it).
/// If `job_control` is `Some`, running jobs can be paused via the control socket.
pub async fn run_jobs_parallel(
    db: &ResumeDb,
    cfg: &DdmConfig,
    download_dir: PathBuf,
    host_policy: &mut HostPolicy,
    force_restart: bool,
    overwrite: bool,
    progress_tx: Option<tokio::sync::mpsc::Sender<ProgressStats>>,
    global_budget: Arc<GlobalConnectionBudget>,
    max_concurrent: usize,
    job_control: Option<std::sync::Arc<crate::control::JobControl>>,
) -> Result<u32> {
    let max_concurrent = max_concurrent.max(1);
    let shared_policy = Arc::new(tokio::sync::Mutex::new(std::mem::replace(
        host_policy,
        HostPolicy::new(cfg.min_segments, cfg.max_segments),
    )));

    let mut run_count = 0u32;
    let mut join_set = tokio::task::JoinSet::new();

    loop {
        while join_set.len() < max_concurrent {
            let Some(job_id) = db.claim_next_queued_job().await? else {
                break;
            };
            let db = db.clone();
            let cfg = cfg.clone();
            let download_dir = download_dir.clone();
            let policy = Arc::clone(&shared_policy);
            let tx = progress_tx.clone();
            let budget = Arc::clone(&global_budget);
            let overwrite = overwrite;
            let job_control = job_control.clone();
            join_set.spawn(async move {
                run_one_job_shared(
                    &db,
                    job_id,
                    force_restart,
                    overwrite,
                    &cfg,
                    &download_dir,
                    policy,
                    tx,
                    Some(budget),
                    job_control,
                )
                .await
            });
        }

        if join_set.is_empty() {
            break;
        }

        let Some(res) = join_set.join_next().await else {
            break;
        };
        run_count += 1;
        res.map_err(|e| anyhow::anyhow!("job task join: {}", e))??;
    }

    // Restore updated policy; if a clone is still held (e.g. by a task), clone out instead of failing.
    *host_policy = match Arc::try_unwrap(shared_policy) {
        Ok(m) => m.into_inner(),
        Err(arc) => arc.lock().await.clone(),
    };

    Ok(run_count)
}
