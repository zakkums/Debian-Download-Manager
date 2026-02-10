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

