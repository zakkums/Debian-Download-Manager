//! HTTP HEAD / metadata probing.
//!
//! Uses the curl crate (libcurl) to fetch response headers and confirm
//! `Content-Length`, `Accept-Ranges: bytes`, and capture ETag/Last-Modified
//! for resume safety.

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

    parse_headers(&headers)
}

/// Parse collected header lines into HeadResult.
fn parse_headers(lines: &[String]) -> Result<HeadResult> {
    let mut content_length = None;
    let mut accept_ranges = false;
    let mut etag = None;
    let mut last_modified = None;
    let mut content_disposition = None;

    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim();
            let value = value.trim();
            if name.eq_ignore_ascii_case("content-length") {
                if let Ok(n) = value.parse::<u64>() {
                    content_length = Some(n);
                }
            }
            if name.eq_ignore_ascii_case("accept-ranges") {
                accept_ranges = value.eq_ignore_ascii_case("bytes");
            }
            if name.eq_ignore_ascii_case("etag") {
                etag = Some(value.trim_matches('"').to_string());
            }
            if name.eq_ignore_ascii_case("last-modified") {
                last_modified = Some(value.to_string());
            }
            if name.eq_ignore_ascii_case("content-disposition") {
                content_disposition = Some(value.to_string());
            }
        }
    }

    Ok(HeadResult {
        content_length,
        accept_ranges,
        etag,
        last_modified,
        content_disposition,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_headers_content_length_and_ranges() {
        let lines = [
            "HTTP/1.1 200 OK".to_string(),
            "Content-Length: 12345".to_string(),
            "Accept-Ranges: bytes".to_string(),
        ];
        let r = parse_headers(&lines).unwrap();
        assert_eq!(r.content_length, Some(12345));
        assert!(r.accept_ranges);
        assert!(r.etag.is_none());
    }

    #[test]
    fn parse_headers_etag_and_last_modified() {
        let lines = [
            "ETag: \"abc-123\"".to_string(),
            "Last-Modified: Wed, 21 Oct 2015 07:28:00 GMT".to_string(),
        ];
        let r = parse_headers(&lines).unwrap();
        assert_eq!(r.etag.as_deref(), Some("abc-123"));
        assert_eq!(
            r.last_modified.as_deref(),
            Some("Wed, 21 Oct 2015 07:28:00 GMT")
        );
    }

    #[test]
    fn parse_headers_no_ranges() {
        let lines = [
            "Content-Length: 999".to_string(),
            "Accept-Ranges: none".to_string(),
        ];
        let r = parse_headers(&lines).unwrap();
        assert_eq!(r.content_length, Some(999));
        assert!(!r.accept_ranges);
    }

    #[test]
    fn parse_headers_content_disposition() {
        let lines = [
            "Content-Disposition: attachment; filename=\"report.pdf\"".to_string(),
        ];
        let r = parse_headers(&lines).unwrap();
        assert!(r.content_disposition.is_some());
        assert!(r.content_disposition.as_deref().unwrap().contains("report.pdf"));
    }
}
