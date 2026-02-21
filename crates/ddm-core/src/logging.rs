//! Logging init: file under XDG state dir, or graceful fallback to stderr.

use anyhow::Result;
use std::fs;
use std::io;
use std::path::PathBuf;
use tracing_subscriber::fmt::writer::BoxMakeWriter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::EnvFilter;

/// Writer that is either a file or stderr (used when file clone fails).
enum FileOrStderr {
    File(std::fs::File),
    Stderr,
}

impl io::Write for FileOrStderr {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            FileOrStderr::File(f) => f.write(buf),
            FileOrStderr::Stderr => io::stderr().lock().write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            FileOrStderr::File(f) => f.flush(),
            FileOrStderr::Stderr => io::stderr().lock().flush(),
        }
    }
}

/// Initialize structured logging to `~/.local/state/ddm/ddm.log`.
/// On failure (e.g. log dir unwritable), returns Err so the caller can fall back to stderr.
pub fn init_logging() -> Result<()> {
    let xdg_dirs = xdg::BaseDirectories::with_prefix("ddm")?;
    let log_dir = xdg_dirs.get_state_home().join("ddm");

    fs::create_dir_all(&log_dir)?;
    let log_file_path: PathBuf = log_dir.join("ddm.log");

    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file_path)?;

    struct FileMakeWriter(std::fs::File);

    impl<'a> MakeWriter<'a> for FileMakeWriter {
        type Writer = FileOrStderr;

        fn make_writer(&'a self) -> Self::Writer {
            self.0
                .try_clone()
                .map(FileOrStderr::File)
                .unwrap_or(FileOrStderr::Stderr)
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

/// Initialize logging to stderr only (no file). Use when init_logging() fails so the CLI doesn't crash.
pub fn init_logging_stderr() {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,ddm=debug"));
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();
}

