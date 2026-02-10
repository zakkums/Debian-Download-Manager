use anyhow::Result;
use clap::{Parser, Subcommand};
use ddm_core::config;
use ddm_core::resume_db::{JobSettings, JobState, ResumeDb};

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
            CliCommand::Run => {
                // TODO: main scheduler/worker loop.
                tracing::info!("run scheduler (stub)");
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
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> CliCommand {
        let cli = Cli::try_parse_from(args).unwrap();
        cli.command
    }

    #[test]
    fn cli_parse_add() {
        match parse(&["ddm", "add", "https://example.com/file.iso"]) {
            CliCommand::Add { url } => assert_eq!(url, "https://example.com/file.iso"),
            _ => panic!("expected Add"),
        }
    }

    #[test]
    fn cli_parse_run() {
        match parse(&["ddm", "run"]) {
            CliCommand::Run => {}
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn cli_parse_status() {
        match parse(&["ddm", "status"]) {
            CliCommand::Status => {}
            _ => panic!("expected Status"),
        }
    }

    #[test]
    fn cli_parse_pause() {
        match parse(&["ddm", "pause", "42"]) {
            CliCommand::Pause { id } => assert_eq!(id, 42),
            _ => panic!("expected Pause"),
        }
    }

    #[test]
    fn cli_parse_resume() {
        match parse(&["ddm", "resume", "1"]) {
            CliCommand::Resume { id } => assert_eq!(id, 1),
            _ => panic!("expected Resume"),
        }
    }

    #[test]
    fn cli_parse_remove() {
        match parse(&["ddm", "remove", "99"]) {
            CliCommand::Remove { id } => assert_eq!(id, 99),
            _ => panic!("expected Remove"),
        }
    }

    #[test]
    fn cli_parse_import_har_without_cookies() {
        match parse(&["ddm", "import-har", "/path/to/file.har"]) {
            CliCommand::ImportHar { path, allow_cookies } => {
                assert_eq!(path, "/path/to/file.har");
                assert!(!allow_cookies);
            }
            _ => panic!("expected ImportHar"),
        }
    }

    #[test]
    fn cli_parse_import_har_allow_cookies() {
        match parse(&["ddm", "import-har", "x.har", "--allow-cookies"]) {
            CliCommand::ImportHar { path, allow_cookies } => {
                assert_eq!(path, "x.har");
                assert!(allow_cookies);
            }
            _ => panic!("expected ImportHar"),
        }
    }

    #[test]
    fn cli_parse_bench() {
        match parse(&["ddm", "bench", "https://example.com/large.bin"]) {
            CliCommand::Bench { url } => assert_eq!(url, "https://example.com/large.bin"),
            _ => panic!("expected Bench"),
        }
    }
}

