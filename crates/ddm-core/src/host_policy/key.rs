use anyhow::{Context, Result};

/// Key used to index per-host policy entries.
///
/// We intentionally normalise URLs down to `(scheme, host, port)` so that
/// different paths on the same origin share policy (range support, throttling,
/// and recommended segment limits).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct HostKey {
    pub scheme: String,
    pub host: String,
    pub port: u16,
}

impl HostKey {
    /// String form for persistence: "scheme:host:port".
    pub fn to_string_key(&self) -> String {
        format!("{}:{}:{}", self.scheme, self.host, self.port)
    }

    /// Parse from persisted string key.
    pub fn from_string_key(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.splitn(3, ':').collect();
        if parts.len() != 3 {
            return None;
        }
        let port: u16 = parts[2].parse().ok()?;
        Some(Self {
            scheme: parts[0].to_string(),
            host: parts[1].to_string(),
            port,
        })
    }

    /// Construct a host key from a URL string.
    pub fn from_url(url: &str) -> Result<Self> {
        let parsed =
            url::Url::parse(url).with_context(|| format!("invalid URL for host policy: {url}"))?;

        let scheme = parsed.scheme().to_string();
        let host = parsed
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("URL missing host for host policy: {url}"))?
            .to_string();
        let port = parsed
            .port_or_known_default()
            .ok_or_else(|| anyhow::anyhow!("URL missing port and unknown default: {url}"))?;

        Ok(Self {
            scheme,
            host,
            port,
        })
    }
}

