//! Persist HostPolicy to disk (JSON under XDG state dir) so tuning survives across runs.

use anyhow::{Context, Result};
use std::path::Path;

use super::state::{HostPolicy, PersistedHostPolicy};

impl HostPolicy {
    /// Default path for host policy file: `~/.local/state/ddm/host_policy.json`.
    pub fn default_path() -> Result<std::path::PathBuf> {
        let xdg_dirs = xdg::BaseDirectories::with_prefix("ddm")?;
        Ok(xdg_dirs.get_state_home().join("ddm").join("host_policy.json"))
    }

    /// Save current policy to the given path (creates parent dir if needed).
    pub fn save_to_path(&self, path: &Path) -> Result<()> {
        let snapshot = self.to_snapshot();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("create dir: {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(&snapshot).context("serialize host policy")?;
        std::fs::write(path, json).with_context(|| format!("write host policy: {}", path.display()))?;
        Ok(())
    }

    /// Load policy from the given path. If the file is missing or invalid, returns None
    /// (caller can fall back to HostPolicy::new). Bounds are taken from arguments so
    /// config always wins.
    pub fn load_from_path(
        path: &Path,
        min_segments: usize,
        max_segments: usize,
    ) -> Result<Option<HostPolicy>> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("read host policy: {}", path.display())),
        };
        let snapshot: PersistedHostPolicy =
            serde_json::from_slice(&bytes).with_context(|| format!("parse host policy: {}", path.display()))?;
        Ok(Some(HostPolicy::from_snapshot(snapshot, min_segments, max_segments)))
    }
}
