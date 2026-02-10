//! Builder for creating and preallocating temp download files.

use anyhow::{Context, Result};
use std::fs::File;
use std::path::Path;

use super::writer::StorageWriter;
#[cfg(unix)]
use std::os::unix::io::AsRawFd;

/// Builder for a new temp download file. Call `preallocate` then `build` to get
/// a `StorageWriter` that supports concurrent `write_at` from multiple tasks.
pub struct StorageWriterBuilder {
    file: File,
    temp_path: std::path::PathBuf,
}

impl StorageWriterBuilder {
    /// Create a new temp file at `temp_path` (e.g. `destination.part`).
    /// Overwrites if the path already exists.
    pub fn create(temp_path: &Path) -> Result<Self> {
        let file = File::options()
            .write(true)
            .create(true)
            .truncate(true)
            .open(temp_path)
            .with_context(|| format!("failed to create temp file: {}", temp_path.display()))?;
        Ok(StorageWriterBuilder {
            file,
            temp_path: temp_path.to_path_buf(),
        })
    }

    /// Preallocate `size` bytes. On Unix tries `posix_fallocate` for real block
    /// allocation (better throughput, less fragmentation); falls back to `set_len` on failure or non-Unix.
    pub fn preallocate(&mut self, size: u64) -> Result<()> {
        #[cfg(unix)]
        {
            let fd = self.file.as_raw_fd();
            let r = unsafe { libc::posix_fallocate(fd, 0, size as libc::off_t) };
            if r == 0 {
                return Ok(());
            }
            tracing::debug!(errno = r, "posix_fallocate failed, falling back to set_len");
        }
        self.file
            .set_len(size)
            .context("failed to preallocate file")?;
        Ok(())
    }

    /// Finish building and return a writer that can be shared for concurrent writes.
    pub fn build(self) -> StorageWriter {
        StorageWriter::from_file_and_path(self.file, self.temp_path)
    }
}
