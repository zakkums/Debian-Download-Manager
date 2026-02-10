//! HTTP HEAD / metadata probing.
//!
//! Uses the curl crate (libcurl) to fetch response headers and confirm
//! `Content-Length`, `Accept-Ranges: bytes`, and capture ETag/Last-Modified
//! for resume safety.

mod parse;

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::str;
use std::time::Duration;

/// Result of a HEAD request: key headers needed for segmented download and resume.
#[derive(Debug, Clone)]
pub struct HeadResult {
    /// Total size in bytes, if `Content-Length` is present.
    pub content_length: Option<u64>,
    /// True if server sent `Accept-Ranges: bytes`.
    pub accept_ranges: bool,
    /// `ETag` value if present (used for resume validation).
    pub etag: Option<String>,
    /// `Last-Modified` value if present (used for resume validation).
    pub last_modified: Option<String>,
    /// `Content-Disposition` value if present (filename hint).
    pub content_disposition: Option<String>,
}

/// Performs a HEAD request and returns parsed metadata.
///
/// Follows redirects. Optional custom headers can be passed (e.g. from a resolver).
/// Runs in the current thread; call from `spawn_blocking` if used from async code.
pub fn probe(url: &str, custom_headers: &HashMap<String, String>) -> Result<HeadResult> {
    let mut headers: Vec<String> = Vec::new();

    let mut easy = curl::easy::Easy::new();
    easy.url(url)
        .context("invalid URL")?;
    easy.nobody(true)?; // HEAD request
    easy.follow_location(true)?;
    easy.connect_timeout(Duration::from_secs(15))?;
    easy.timeout(Duration::from_secs(30))?;

    // Build curl list for custom headers (e.g. "Name: value").
    let mut list = curl::easy::List::new();
    for (k, v) in custom_headers {
        list.append(&format!("{}: {}", k.trim(), v.trim()))?;
    }
    if !custom_headers.is_empty() {
        easy.http_headers(list)?;
    }

    {
        let mut transfer = easy.transfer();
        transfer.header_function(|data| {
            if let Ok(s) = str::from_utf8(data) {
                headers.push(s.trim_end().to_string());
            }
            true
        })?;
        transfer.perform().context("HEAD request failed")?;
    }

    let code = easy.response_code().context("no response code")?;
    if code < 200 || code >= 300 {
        anyhow::bail!("HEAD {} returned HTTP {}", url, code);
    }

    parse::parse_headers(&headers)
}
