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

fn parse_content_range_total(value: &str) -> Option<u64> {
    // Examples:
    // - "bytes 0-0/12345"
    // - "bytes 0-1023/12345"
    // - "bytes */12345"
    let (_, total) = value.split_once('/')?;
    let total = total.trim();
    if total == "*" {
        return None;
    }
    total.parse::<u64>().ok()
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
    easy.max_redirections(10)?;
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
                let line = s.trim_end();
                // Redirect-safe: when curl follows redirects, it emits multiple header blocks.
                // Clear on each HTTP status line so we only keep the final response's headers.
                if line.starts_with("HTTP/") {
                    headers.clear();
                }
                headers.push(line.to_string());
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

/// Performs a lightweight GET probe for metadata by requesting the first byte (`Range: bytes=0-0`).
/// Useful when HEAD is blocked, or when HEAD does not advertise ranges/length but ranged GET does.
///
/// This does not write a file; the response body (if any) is discarded.
pub fn probe_range0(url: &str, custom_headers: &HashMap<String, String>) -> Result<HeadResult> {
    let mut headers: Vec<String> = Vec::new();

    let mut easy = curl::easy::Easy::new();
    easy.url(url).context("invalid URL")?;
    easy.follow_location(true)?;
    easy.max_redirections(10)?;
    easy.connect_timeout(Duration::from_secs(15))?;
    easy.timeout(Duration::from_secs(30))?;
    easy.range("0-0")?;

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
                let line = s.trim_end();
                if line.starts_with("HTTP/") {
                    headers.clear();
                }
                headers.push(line.to_string());
            }
            true
        })?;
        transfer.write_function(|data| Ok(data.len()))?;
        transfer.perform().context("GET range probe failed")?;
    }

    let code = easy.response_code().context("no response code")?;
    if code < 200 || code >= 300 {
        anyhow::bail!("GET range probe {} returned HTTP {}", url, code);
    }

    let mut r = parse::parse_headers(&headers)?;
    if code == 206 {
        // Server honored the Range request: treat as range-capable even if Accept-Ranges is missing.
        r.accept_ranges = true;
        for line in &headers {
            if let Some((name, value)) = line.split_once(':') {
                if name.trim().eq_ignore_ascii_case("content-range") {
                    if let Some(total) = parse_content_range_total(value.trim()) {
                        r.content_length = Some(total);
                    }
                }
            }
        }
    }
    Ok(r)
}

/// Best-effort metadata probe.
///
/// - Tries HEAD first.
/// - If HEAD fails, falls back to `probe_range0`.
/// - If HEAD succeeds but doesn't provide enough info (no ranges or no length),
///   also tries `probe_range0` and merges the results.
pub fn probe_best_effort(url: &str, custom_headers: &HashMap<String, String>) -> Result<HeadResult> {
    let head = probe(url, custom_headers);
    match head {
        Ok(mut r) => {
            if r.accept_ranges && r.content_length.is_some() {
                return Ok(r);
            }
            if let Ok(r2) = probe_range0(url, custom_headers) {
                // Merge: prefer the more capable/more complete result.
                r.accept_ranges |= r2.accept_ranges;
                if r.content_length.is_none() {
                    r.content_length = r2.content_length;
                }
                if r.content_disposition.is_none() {
                    r.content_disposition = r2.content_disposition;
                }
                if r.etag.is_none() {
                    r.etag = r2.etag;
                }
                if r.last_modified.is_none() {
                    r.last_modified = r2.last_modified;
                }
            }
            Ok(r)
        }
        Err(_) => probe_range0(url, custom_headers),
    }
}
