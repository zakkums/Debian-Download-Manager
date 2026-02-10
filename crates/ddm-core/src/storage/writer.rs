//! Concurrent offset writer for temp download files.

use anyhow::{Context, Result};
use std::fs::File;
use std::path::Path;
use std::sync::Arc;
#[cfg(unix)]
use std::os::unix::fs::FileExt;

/// Writer for a temp download file. Safe to clone and use from multiple tasks;
/// each `write_at` is independent (pwrite-style).
#[derive(Clone)]
pub struct StorageWriter {
    file: Arc<File>,
    temp_path: std::path::PathBuf,
}

impl StorageWriter {
    /// Create from an open file and path (used by StorageWriterBuilder).
    pub(crate) fn from_file_and_path(file: File, temp_path: std::path::PathBuf) -> Self {
        Self {
            file: Arc::new(file),
            temp_path,
        }
    }

    /// Open an existing temp file for resume (read+write, no truncation).
    /// Use this when resuming a job; the file must already exist and have been preallocated.
    pub fn open_existing(temp_path: &Path) -> Result<Self> {
        let file = File::options()
            .read(true)
            .write(true)
            .open(temp_path)
            .with_context(|| format!("failed to open existing temp file: {}", temp_path.display()))?;
        Ok(StorageWriter {
            file: Arc::new(file),
            temp_path: temp_path.to_path_buf(),
        })
    }

    /// Write `data` at `offset`. Does not change the file's logical cursor; safe for concurrent use.
    #[cfg(unix)]
    pub fn write_at(&self, offset: u64, data: &[u8]) -> Result<()> {
        let n = self
            .file
            .write_at(data, offset)
            .context("storage write_at failed")?;
        if n != data.len() {
            anyhow::bail!("short write: {} of {}", n, data.len());
        }
        Ok(())
    }

    /// Stub for non-Unix (e.g. Windows): use seek + write. Not safe for concurrent use.
    #[cfg(not(unix))]
    pub fn write_at(&self, offset: u64, data: &[u8]) -> Result<()> {
        use std::io::{Seek, SeekFrom, Write};
        let mut f = (*self.file).try_clone()?;
        f.seek(SeekFrom::Start(offset))?;
        f.write_all(data)?;
        Ok(())
    }

    /// Sync file data to disk. Call before `finalize` for durability.
    pub fn sync(&self) -> Result<()> {
        self.file.sync_all().context("storage sync failed")?;
        Ok(())
    }

    /// Path to the current temp file.
    pub fn temp_path(&self) -> &Path {
        &self.temp_path
    }

    /// Atomically rename the temp file to the final path. Consumes the writer and closes the file.
    /// Call `sync` before this if you need durability. Fails if `final_path` is on a different filesystem.
    pub fn finalize(self, final_path: &Path) -> Result<()> {
        let temp_path = self.temp_path.clone();
        drop(self.file);

        std::fs::rename(&temp_path, final_path)
            .with_context(|| format!("failed to rename {} to {}", temp_path.display(), final_path.display()))?;
        Ok(())
    }
}
