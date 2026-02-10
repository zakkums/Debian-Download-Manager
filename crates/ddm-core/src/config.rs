use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Global configuration loaded from `~/.config/ddm/config.toml`.
///
/// This is intentionally minimal for the initial milestone and will be
/// extended with more tuning parameters (per-host policy, retry policy,
/// bandwidth cap, etc.) in later steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DdmConfig {
    /// Maximum total concurrent HTTP connections across all jobs.
    pub max_total_connections: usize,
    /// Maximum concurrent HTTP connections per host.
    pub max_connections_per_host: usize,
    /// Minimum number of segments per job.
    pub min_segments: usize,
    /// Maximum number of segments per job.
    pub max_segments: usize,
}

impl Default for DdmConfig {
    fn default() -> Self {
        Self {
            max_total_connections: 64,
            max_connections_per_host: 16,
            min_segments: 4,
            max_segments: 16,
        }
    }
}

pub fn config_path() -> Result<PathBuf> {
    let xdg_dirs = xdg::BaseDirectories::with_prefix("ddm")?;
    Ok(xdg_dirs.place_config_file("config.toml")?)
}

/// Load configuration from disk, creating a default file if none exists.
pub fn load_or_init() -> Result<DdmConfig> {
    let path = config_path()?;
    if !path.exists() {
        let default_cfg = DdmConfig::default();
        let toml = toml::to_string_pretty(&default_cfg)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, toml)?;
        tracing::info!("created default config at {}", path.display());
        return Ok(default_cfg);
    }

    let data = fs::read_to_string(&path)?;
    let cfg: DdmConfig = toml::from_str(&data)?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let cfg = DdmConfig::default();
        assert_eq!(cfg.max_total_connections, 64);
        assert_eq!(cfg.max_connections_per_host, 16);
        assert_eq!(cfg.min_segments, 4);
        assert_eq!(cfg.max_segments, 16);
    }

    #[test]
    fn config_toml_roundtrip() {
        let cfg = DdmConfig::default();
        let toml = toml::to_string_pretty(&cfg).unwrap();
        let parsed: DdmConfig = toml::from_str(&toml).unwrap();
        assert_eq!(parsed.max_total_connections, cfg.max_total_connections);
        assert_eq!(parsed.max_connections_per_host, cfg.max_connections_per_host);
        assert_eq!(parsed.min_segments, cfg.min_segments);
        assert_eq!(parsed.max_segments, cfg.max_segments);
    }

    #[test]
    fn config_toml_custom_values() {
        let toml = r#"
            max_total_connections = 8
            max_connections_per_host = 4
            min_segments = 2
            max_segments = 32
        "#;
        let cfg: DdmConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.max_total_connections, 8);
        assert_eq!(cfg.max_connections_per_host, 4);
        assert_eq!(cfg.min_segments, 2);
        assert_eq!(cfg.max_segments, 32);
    }
}

