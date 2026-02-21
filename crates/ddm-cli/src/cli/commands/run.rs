//! `ddm run` – run the scheduler to process queued jobs.

use anyhow::Result;
use ddm_core::config::DdmConfig;
use ddm_core::control::JobControl;
use ddm_core::host_policy::HostPolicy;
use ddm_core::resume_db::ResumeDb;
use ddm_core::scheduler::{self, GlobalConnectionBudget, ProgressStats};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use crate::cli::control_socket;

pub async fn run_scheduler(
    db: &ResumeDb,
    cfg: &DdmConfig,
    download_dir: &Path,
    force_restart: bool,
    jobs: usize,
    overwrite: bool,
) -> Result<()> {
    let recovered = db.recover_running_jobs().await?;
    if recovered > 0 {
        tracing::info!("recovered {} job(s) from previous run", recovered);
    }
    let global_budget = Arc::new(GlobalConnectionBudget::new(cfg.max_total_connections));
    let mut host_policy = match HostPolicy::default_path()
        .and_then(|p| HostPolicy::load_from_path(&p, cfg.min_segments, cfg.max_segments))
    {
        Ok(Some(policy)) => {
            tracing::debug!("loaded host policy from state file");
            policy
        }
        _ => HostPolicy::new(cfg.min_segments, cfg.max_segments),
    };

    let job_control = Arc::new(JobControl::new());
    if let Ok(socket_path) = ddm_core::control::default_control_socket_path() {
        if control_socket::spawn_control_listener(Arc::clone(&job_control), &socket_path).is_ok() {
            tracing::debug!(path = %socket_path.display(), "control socket listening");
        }
    }

    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel::<ProgressStats>(16);
    const PROGRESS_INTERVAL_MS: u64 = 500;
    let progress_handle = tokio::spawn(async move {
        let mut last_print = Instant::now();
        while let Some(stats) = progress_rx.recv().await {
            let now = Instant::now();
            if now.duration_since(last_print).as_millis() as u64 >= PROGRESS_INTERVAL_MS
                || stats.bytes_done >= stats.total_bytes
            {
                let done_mib = stats.bytes_done as f64 / 1_048_576.0;
                let total_mib = stats.total_bytes as f64 / 1_048_576.0;
                let pct = stats.fraction() * 100.0;
                let rate = if stats.elapsed_secs > 0.0 {
                    stats.effective_bytes() as f64 / stats.elapsed_secs
                } else {
                    0.0
                };
                let rate_mib = rate / 1_048_576.0;
                let eta = stats
                    .eta_secs()
                    .map(|s| format!("{:.0}s", s))
                    .unwrap_or_else(|| "?".to_string());
                println!(
                    "\r  {:.1} / {:.1} MiB ({:.1}%)  {:.2} MiB/s  ETA {}  ",
                    done_mib, total_mib, pct, rate_mib, eta
                );
                last_print = now;
            }
        }
        println!();
    });

    let run_count = if jobs > 1 {
        scheduler::run_jobs_parallel(
            db,
            cfg,
            download_dir.to_path_buf(),
            &mut host_policy,
            force_restart,
            overwrite,
            Some(progress_tx),
            Arc::clone(&global_budget),
            jobs,
            Some(Arc::clone(&job_control)),
        )
        .await?
    } else {
        let mut run_count = 0u32;
        let budget_ref = global_budget.as_ref();
        while scheduler::run_next_job(
            db,
            force_restart,
            overwrite,
            cfg,
            download_dir,
            &mut host_policy,
            Some(&progress_tx),
            Some(budget_ref),
            Some(Arc::clone(&job_control)),
        )
        .await?
        {
            run_count += 1;
        }
        drop(progress_tx);
        run_count
    };

    let _ = progress_handle.await;

    if let Ok(path) = HostPolicy::default_path() {
        if host_policy.save_to_path(&path).is_err() {
            tracing::warn!("could not save host policy to {}", path.display());
        }
    }

    if run_count == 0 {
        println!("No queued jobs.");
    } else {
        tracing::info!("run completed {} job(s)", run_count);
    }
    Ok(())
}
