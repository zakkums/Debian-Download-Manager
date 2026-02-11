//! CLI for the DDM download manager.

mod commands;

use anyhow::Result;
use clap::{Parser, Subcommand};
use ddm_core::config;
use ddm_core::resume_db::ResumeDb;
use std::path::Path;

use commands::{
    run_add, run_bench, run_checksum, run_import_har, run_pause, run_remove, run_resume,
    run_scheduler, run_status,
};

/// Top-level CLI for the DDM download manager.
#[derive(Debug, Parser)]
#[command(name = "ddm")]
#[command(about = "DDM: high-throughput segmented download manager", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: CliCommand,
}

#[derive(Debug, Subcommand)]
pub enum CliCommand {
    /// Add a new download job.
    Add {
        /// Direct HTTP/HTTPS URL to download.
        url: String,
        /// Directory where the file will be saved (default: current directory). Stored with the job so resume works from any working directory.
        #[arg(long, value_name = "DIR")]
        download_dir: Option<std::path::PathBuf>,
    },

    /// Run the scheduler/worker loop to process queued jobs.
    Run {
        /// If the remote file changed (ETag/Last-Modified/size), discard progress and re-download.
        #[arg(long)]
        force_restart: bool,
        /// Run up to N jobs concurrently (default 1). Use >1 for parallel downloads sharing the global connection budget.
        #[arg(long, default_value = "1", value_name = "N")]
        jobs: usize,
        /// Overwrite existing final file if it already exists on disk. Without this, run fails when the target file is present.
        #[arg(long)]
        overwrite: bool,
    },

    /// Show status of all jobs.
    Status,

    /// Pause a job by ID. Only affects scheduling: the job will not be picked on the next run. Does not stop an already running download.
    Pause {
        /// Job identifier.
        id: i64,
    },

    /// Resume a paused job by its ID.
    Resume {
        /// Job identifier.
        id: i64,
    },

    /// Remove a job by ID. With --delete-files, also deletes the job's .part and final file(s) from the current directory or --download-dir.
    Remove {
        /// Job identifier.
        id: i64,
        /// Also delete the job's downloaded .part and final file(s) from the given directory.
        #[arg(long)]
        delete_files: bool,
        /// Directory where the job's files live (used only with --delete-files; default: current directory).
        #[arg(long, value_name = "DIR")]
        download_dir: Option<std::path::PathBuf>,
    },

    /// Import a HAR file and create download jobs from it.
    ImportHar {
        /// Path to the HAR file.
        path: String,

        /// Allow persisting cookies extracted from the HAR (if needed).
        #[arg(long)]
        allow_cookies: bool,
    },

    /// Benchmark different segment counts for a given URL.
    Bench {
        /// Direct HTTP/HTTPS URL to benchmark.
        url: String,
    },

    /// Compute SHA-256 of a file (e.g. after download).
    Checksum {
        /// Path to the file.
        path: String,
    },
}

impl CliCommand {
    pub async fn run_from_args() -> Result<()> {
        let cli = Cli::parse();
        let cfg = config::load_or_init()?;
        tracing::debug!("loaded config: {:?}", cfg);
        let db = ResumeDb::open_default().await?;

        match cli.command {
            CliCommand::Add { url, download_dir } => {
                let dir = download_dir.or_else(|| std::env::current_dir().ok());
                run_add(&db, &url, dir.as_deref()).await?
            }
            CliCommand::Run { force_restart, jobs, overwrite } => {
                let download_dir = std::env::current_dir()?;
                run_scheduler(&db, &cfg, &download_dir, force_restart, jobs, overwrite).await?;
            }
            CliCommand::Status => run_status(&db).await?,
            CliCommand::Pause { id } => run_pause(&db, id).await?,
            CliCommand::Resume { id } => run_resume(&db, id).await?,
            CliCommand::Remove { id, delete_files, download_dir } => {
                let dir = if delete_files {
                    download_dir.or_else(|| std::env::current_dir().ok())
                } else {
                    None
                };
                run_remove(&db, id, delete_files, dir.as_deref()).await?
            }
            CliCommand::ImportHar { path, allow_cookies } => {
                run_import_har(&db, Path::new(&path), allow_cookies).await?;
            }
            CliCommand::Bench { url } => run_bench(&url).await?,
            CliCommand::Checksum { path } => run_checksum(Path::new(&path)).await?,
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests;
