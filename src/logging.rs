use anyhow::Result;
use std::fs;
use std::path::PathBuf;
use tracing_subscriber::fmt::writer::BoxMakeWriter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::EnvFilter;

/// Initialize structured logging to `~/.local/state/ddm/ddm.log`.
///
/// Uses the XDG base directory spec via the `xdg` crate to locate the state directory.
pub fn init_logging() -> Result<()> {
    let xdg_dirs = xdg::BaseDirectories::with_prefix("ddm")?;
    let log_dir = xdg_dirs.get_state_home();

    fs::create_dir_all(&log_dir)?;
    let log_file_path: PathBuf = log_dir.join("ddm.log");

    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file_path)?;

    // Simple writer that always clones the same file handle.
    struct FileMakeWriter(std::fs::File);

    impl<'a> MakeWriter<'a> for FileMakeWriter {
        type Writer = std::fs::File;

        fn make_writer(&'a self) -> Self::Writer {
            self.0.try_clone().expect("failed to clone log file handle")
        }
    }

    let writer: BoxMakeWriter = BoxMakeWriter::new(FileMakeWriter(file));

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,ddm=debug"));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(writer)
        .with_ansi(false)
        .init();

    tracing::info!("ddm logging initialized at {}", log_file_path.display());

    Ok(())
}

