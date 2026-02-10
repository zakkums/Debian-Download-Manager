//! Disk I/O and file lifecycle.
//!
//! Preallocates temp files (fallocate on Linux when available, else set_len),
//! supports concurrent offset writes (pwrite), fsync policy, and atomic
//! finalize (rename from `.part` to final name).

use anyhow::{Context, Result};
use std::fs::File;
use std::path::Path;
use std::sync::Arc;
#[cfg(unix)]
use std::os::unix::fs::FileExt;
#[cfg(unix)]
use std::os::unix::io::AsRawFd;

/// Temporary file suffix used before atomic rename.
pub const TEMP_SUFFIX: &str = ".part";

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
        StorageWriter {
            file: Arc::new(self.file),
            temp_path: self.temp_path,
        }
    }
}

/// Writer for a temp download file. Safe to clone and use from multiple tasks;
/// each `write_at` is independent (pwrite-style).
#[derive(Clone)]
pub struct StorageWriter {
    file: Arc<File>,
    temp_path: std::path::PathBuf,
}

impl StorageWriter {
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
        // Ensure we're dropped (file closed) before rename on some platforms.
        let temp_path = self.temp_path.clone();
        drop(self.file);

        std::fs::rename(&temp_path, final_path)
            .with_context(|| format!("failed to rename {} to {}", temp_path.display(), final_path.display()))?;
        Ok(())
    }
}

/// Path for the temp file: appends `.part` to the final path (e.g. `file.iso` → `file.iso.part`).
pub fn temp_path(final_path: &Path) -> std::path::PathBuf {
    let mut o = final_path.as_os_str().to_owned();
    o.push(".part");
    std::path::PathBuf::from(o)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn temp_path_appends_part() {
        let p = temp_path(Path::new("file.iso"));
        assert_eq!(p.to_string_lossy(), "file.iso.part");
        let p2 = temp_path(Path::new("/tmp/archive.zip"));
        assert_eq!(p2.to_string_lossy(), "/tmp/archive.zip.part");
    }

    #[test]
    fn create_preallocate_write_finalize() {
        let dir = tempfile::tempdir().unwrap();
        let final_path = dir.path().join("output.bin");
        let tp = temp_path(&final_path);

        let mut builder = StorageWriterBuilder::create(&tp).unwrap();
        builder.preallocate(100).unwrap();
        let writer = builder.build();

        writer.write_at(0, b"hello").unwrap();
        writer.write_at(50, b"world").unwrap();
        writer.write_at(95, b"xy").unwrap();
        writer.sync().unwrap();
        writer.finalize(&final_path).unwrap();

        assert!(!tp.exists());
        assert!(final_path.exists());
        let mut f = File::open(&final_path).unwrap();
        let mut buf = vec![0u8; 100];
        f.read_exact(&mut buf).unwrap();
        assert_eq!(&buf[0..5], b"hello");
        assert_eq!(&buf[50..55], b"world");
        assert_eq!(&buf[95..97], b"xy");
    }

    #[test]
    fn write_at_concurrent_style() {
        let dir = tempfile::tempdir().unwrap();
        let tp = dir.path().join("out.part");
        let mut builder = StorageWriterBuilder::create(&tp).unwrap();
        builder.preallocate(20).unwrap();
        let writer = builder.build();
        let w2 = writer.clone();
        writer.write_at(0, b"aaaa").unwrap();
        w2.write_at(10, b"bbbb").unwrap();
        writer.write_at(4, b"cccc").unwrap();
        writer.sync().unwrap();
        let final_p = dir.path().join("out.bin");
        writer.finalize(&final_p).unwrap();
        let mut f = File::open(&final_p).unwrap();
        let mut buf = vec![0u8; 20];
        f.read_exact(&mut buf).unwrap();
        assert_eq!(&buf[0..4], b"aaaa");
        assert_eq!(&buf[4..8], b"cccc");
        assert_eq!(&buf[10..14], b"bbbb");
    }
}
