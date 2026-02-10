use anyhow::Result;
use clap::{Parser, Subcommand};
use ddm_core::config;
use ddm_core::host_policy::HostPolicy;
use ddm_core::resume_db::{JobSettings, JobState, ResumeDb};
use ddm_core::scheduler;

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
}

impl CliCommand {
    pub async fn run_from_args() -> Result<()> {
        let cli = Cli::parse();

        // Load global config early; later stages will pass this down to
        // scheduler/downloader modules.
        let cfg = config::load_or_init()?;
        tracing::debug!("loaded config: {:?}", cfg);

        // Open (or create) the persistent job database.
        let db = ResumeDb::open_default().await?;

        match cli.command {
            CliCommand::Add { url } => {
                let settings = JobSettings::default();
                let id = db.add_job(&url, &settings).await?;
                println!("Added job {id} for URL: {url}");
            }
            CliCommand::Run { force_restart } => {
                let download_dir = std::env::current_dir()?;
                let recovered = db.recover_running_jobs().await?;
                if recovered > 0 {
                    tracing::info!("recovered {} job(s) from previous run", recovered);
                }
                let mut run_count = 0u32;
                let mut host_policy = HostPolicy::new(cfg.min_segments, cfg.max_segments);
                while scheduler::run_next_job(
                    &db,
                    force_restart,
                    &cfg,
                    &download_dir,
                    &mut host_policy,
                )
                .await?
                {
                    run_count += 1;
                }
                if run_count == 0 {
                    println!("No queued jobs.");
                } else {
                    tracing::info!("run completed {} job(s)", run_count);
                }
            }
            CliCommand::Status => {
                let jobs = db.list_jobs().await?;
                if jobs.is_empty() {
                    println!("No jobs in database.");
                } else {
                    println!("{:<6} {:<10} {:<10} {}", "ID", "STATE", "SIZE", "URL");
                    for j in jobs {
                        let size_str = j
                            .total_size
                            .map(|s| format!("{s}"))
                            .unwrap_or_else(|| "-".to_string());
                        println!(
                            "{:<6} {:<10} {:<10} {}",
                            j.id,
                            format!("{:?}", j.state).to_lowercase(),
                            size_str,
                            j.url
                        );
                    }
                }
            }
            CliCommand::Pause { id } => {
                db.set_state(id, JobState::Paused).await?;
                println!("Paused job {id}");
            }
            CliCommand::Resume { id } => {
                db.set_state(id, JobState::Queued).await?;
                println!("Resumed job {id}");
            }
            CliCommand::Remove { id } => {
                db.remove_job(id).await?;
                println!("Removed job {id}");
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

#[cfg(test)]
mod tests;

