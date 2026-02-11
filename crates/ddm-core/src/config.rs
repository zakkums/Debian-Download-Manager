use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Retry policy parameters (optional section in config.toml).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum number of attempts per segment (including the first).
    pub max_attempts: u32,
    /// Base delay in seconds for exponential backoff (e.g. 0.25 = 250ms).
    pub base_delay_secs: f64,
    /// Maximum backoff delay in seconds.
    pub max_delay_secs: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            base_delay_secs: 0.25,
            max_delay_secs: 30,
        }
    }
}

/// Download backend: Easy+threads (one Easy per segment in OS threads) or curl multi (single-threaded, multiple Easy2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadBackend {
    #[default]
    Easy,
    Multi,
}

/// Global configuration loaded from `~/.config/ddm/config.toml`.
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
    /// Optional retry policy; if missing, built-in defaults are used.
    #[serde(default)]
    pub retry: Option<RetryConfig>,
    /// Optional bandwidth cap in bytes per second (None = no cap). Not yet enforced.
    #[serde(default)]
    pub max_bytes_per_sec: Option<u64>,
    /// Optional segment read/write buffer size in bytes (None = library default). Reserved.
    #[serde(default)]
    pub segment_buffer_bytes: Option<usize>,
    /// Download backend: "easy" (default) or "multi". Easy = one Easy handle per segment in threads; multi = curl multi.
    #[serde(default)]
    pub download_backend: Option<DownloadBackend>,
}

impl Default for DdmConfig {
    fn default() -> Self {
        Self {
            max_total_connections: 64,
            max_connections_per_host: 16,
            min_segments: 4,
            max_segments: 16,
            retry: None,
            max_bytes_per_sec: None,
            segment_buffer_bytes: None,
            download_backend: None,
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
        assert!(cfg.retry.is_none());
        assert!(cfg.max_bytes_per_sec.is_none());
    }

    #[test]
    fn config_toml_download_backend() {
        let toml = r#"
            max_total_connections = 8
            max_connections_per_host = 4
            min_segments = 2
            max_segments = 16
            download_backend = "multi"
        "#;
        let cfg: DdmConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.download_backend, Some(DownloadBackend::Multi));
        let toml_easy = r#"
            max_total_connections = 8
            max_connections_per_host = 4
            min_segments = 2
            max_segments = 16
            download_backend = "easy"
        "#;
        let cfg_easy: DdmConfig = toml::from_str(toml_easy).unwrap();
        assert_eq!(cfg_easy.download_backend, Some(DownloadBackend::Easy));
    }

    #[test]
    fn config_toml_retry_and_extensions() {
        let toml = r#"
            max_total_connections = 16
            max_connections_per_host = 8
            min_segments = 2
            max_segments = 16
            max_bytes_per_sec = 1_000_000
            segment_buffer_bytes = 65536

            [retry]
            max_attempts = 3
            base_delay_secs = 0.5
            max_delay_secs = 15
        "#;
        let cfg: DdmConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.max_bytes_per_sec, Some(1_000_000));
        assert_eq!(cfg.segment_buffer_bytes, Some(65536));
        let retry = cfg.retry.as_ref().unwrap();
        assert_eq!(retry.max_attempts, 3);
        assert!((retry.base_delay_secs - 0.5).abs() < 1e-9);
        assert_eq!(retry.max_delay_secs, 15);
    }
}

