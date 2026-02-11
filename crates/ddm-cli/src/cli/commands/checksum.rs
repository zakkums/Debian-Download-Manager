//! Checksum command: compute SHA-256 of a file.

use anyhow::Result;
use ddm_core::checksum;
use std::path::Path;

/// Compute and print SHA-256 of the given file.
pub async fn run_checksum(path: &Path) -> Result<()> {
    let digest = checksum::sha256_path(path)?;
    println!("{}  {}", digest, path.display());
    Ok(())
}
