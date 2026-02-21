//! Optional checksum verification (e.g., SHA-256) after completion.
//!
//! This module computes checksums on demand, not inline with the main
//! download path to avoid impacting throughput.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;

const BUF_SIZE: usize = 64 * 1024;

/// Compute SHA-256 of a file and return the digest as lowercase hex.
/// Reads in chunks to keep memory use bounded; suitable for large files.
pub fn sha256_path(path: &Path) -> Result<String> {
    let mut f = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; BUF_SIZE];
    loop {
        let n = f
            .read(&mut buf)
            .with_context(|| format!("read {}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    Ok(hex::encode(digest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn sha256_path_empty_file() {
        let f = tempfile::NamedTempFile::new().unwrap();
        let path = f.path();
        let digest = sha256_path(path).unwrap();
        assert_eq!(
            digest,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_path_known_content() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"hello\n").unwrap();
        f.flush().unwrap();
        let path = f.path();
        let digest = sha256_path(path).unwrap();
        assert_eq!(
            digest,
            "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
        );
    }
}
