//! Resolver interface for turning higher-level inputs into direct URLs.
//!
//! The core downloader only depends on this trait and does not know about
//! HAR or any other specific resolver formats.

use std::collections::HashMap;

/// Minimal request specification needed by the core downloader.
#[derive(Debug, Clone)]
pub struct ResolvedJobSpec {
    pub url: String,
    /// Minimal headers required to perform the GET.
    pub headers: HashMap<String, String>,
}

/// Trait implemented by optional resolver plugins (e.g. HAR resolver).
pub trait Resolver {
    fn resolve(&self) -> anyhow::Result<ResolvedJobSpec>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_job_spec_holds_url_and_headers() {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer xyz".to_string());
        let spec = ResolvedJobSpec {
            url: "https://cdn.example.com/file.zip".to_string(),
            headers: headers.clone(),
        };
        assert_eq!(spec.url, "https://cdn.example.com/file.zip");
        assert_eq!(spec.headers.get("Authorization").unwrap(), "Bearer xyz");

        let spec2 = spec.clone();
        assert_eq!(spec2.url, spec.url);
        assert_eq!(spec2.headers.len(), 1);
    }

    #[test]
    fn resolved_job_spec_empty_headers() {
        let spec = ResolvedJobSpec {
            url: "https://example.com/direct".to_string(),
            headers: HashMap::new(),
        };
        assert!(spec.headers.is_empty());
    }
}

