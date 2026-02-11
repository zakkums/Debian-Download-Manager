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
    },

    /// Run the scheduler/worker loop to process queued jobs.
    Run {
        /// If the remote file changed (ETag/Last-Modified/size), discard progress and re-download.
        #[arg(long)]
        force_restart: bool,
        /// Run up to N jobs concurrently (default 1). Use >1 for parallel downloads sharing the global connection budget.
        #[arg(long, default_value = "1", value_name = "N")]
        jobs: usize,
    },

    /// Show status of all jobs.
    Status,

    /// Pause a running or queued job by its ID.
    Pause {
        /// Job identifier.
        id: i64,
    },

    /// Resume a paused job by its ID.
    Resume {
        /// Job identifier.
        id: i64,
    },

    /// Remove a job (and optionally its data) by ID.
    Remove {
        /// Job identifier.
        id: i64,
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
            CliCommand::Add { url } => run_add(&db, &url).await?,
            CliCommand::Run { force_restart, jobs } => {
                let download_dir = std::env::current_dir()?;
                run_scheduler(&db, &cfg, &download_dir, force_restart, jobs).await?;
            }
            CliCommand::Status => run_status(&db).await?,
            CliCommand::Pause { id } => run_pause(&db, id).await?,
            CliCommand::Resume { id } => run_resume(&db, id).await?,
            CliCommand::Remove { id } => run_remove(&db, id).await?,
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
