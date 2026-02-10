use anyhow::Result;
use clap::{Parser, Subcommand};
use ddm_core::config;

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
    Run,

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
}

impl CliCommand {
    pub fn run_from_args() -> Result<()> {
        let cli = Cli::parse();

        // Load global config early; later stages will pass this down to
        // scheduler/downloader modules.
        let cfg = config::load_or_init()?;
        tracing::debug!("loaded config: {:?}", cfg);

        match cli.command {
            CliCommand::Add { url } => {
                // TODO: insert job into SQLite job database.
                tracing::info!("add job for url={}", url);
            }
            CliCommand::Run => {
                // TODO: main scheduler/worker loop.
                tracing::info!("run scheduler (stub)");
            }
            CliCommand::Status => {
                // TODO: query job DB and display status/progress.
                tracing::info!("status (stub)");
            }
            CliCommand::Pause { id } => {
                // TODO: update job state in DB.
                tracing::info!("pause job id={}", id);
            }
            CliCommand::Resume { id } => {
                // TODO: update job state in DB.
                tracing::info!("resume job id={}", id);
            }
            CliCommand::Remove { id } => {
                // TODO: remove job and optionally files.
                tracing::info!("remove job id={}", id);
            }
            CliCommand::ImportHar { path, allow_cookies } => {
                // TODO: invoke HAR resolver plugin, create job, and kick off download.
                tracing::info!(
                    "import-har path={} allow_cookies={}",
                    path,
                    allow_cookies
                );
            }
            CliCommand::Bench { url } => {
                // TODO: run benchmark mode for the given URL.
                tracing::info!("bench url={}", url);
            }
        }

        Ok(())
    }
}

