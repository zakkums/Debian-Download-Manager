//! Resolve HAR file to direct URL and optional headers.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

use crate::resolver::ResolvedJobSpec;

use super::parse::{HarEntry, HarHeader, HarLog};

/// Resolves a HAR file to a direct URL (and optional headers).
///
/// Picks the entry whose response looks like a real download: status 200 or 206,
/// Content-Length present, and optionally Accept-Ranges. This avoids choosing
/// an unrelated redirect when the HAR has mixed entries. If no such entry exists,
/// falls back to the previous redirect-chain behavior.
///
/// If `include_cookies` is true, the `Cookie` header from the chosen request
/// is included (for cookie-based CDN auth).
pub fn resolve_har(path: &Path, include_cookies: bool) -> Result<ResolvedJobSpec> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("read HAR file: {}", path.display()))?;
    let har: HarLog = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse HAR JSON: {}", path.display()))?;

    let entries = har.log.entries;
    if entries.is_empty() {
        anyhow::bail!("HAR file has no entries");
    }

    let best_index = select_download_entry(&entries).unwrap_or_else(|| {
        let mut final_url = entries[0].request.url.clone();
        for entry in &entries {
            let status = entry.response.status;
            if (301..=302).contains(&status) || status == 307 || status == 308 {
                if let Some(url) = entry
                    .response
                    .redirect_url
                    .clone()
                    .or_else(|| get_header(&entry.response.headers, "Location").map(String::from))
                {
                    final_url = url.trim().to_string();
                }
            }
        }
        entries
            .iter()
            .enumerate()
            .rev()
            .find(|(_, e)| e.request.url == final_url)
            .map(|(i, _)| i)
            .unwrap_or(0)
    });

    let entry = &entries[best_index];
    let final_url = entry.request.url.clone();
    let mut headers = HashMap::new();
    if include_cookies {
        if let Some(cookie) = get_header(&entry.request.headers, "Cookie") {
            if !cookie.is_empty() {
                headers.insert("Cookie".to_string(), cookie.to_string());
            }
        }
    }

    Ok(ResolvedJobSpec {
        url: final_url,
        headers,
    })
}

/// True if response looks like a real download: 200/206 + Content-Length.
fn response_looks_like_download(entry: &HarEntry) -> bool {
    let status = entry.response.status;
    if status != 200 && status != 206 {
        return false;
    }
    get_header(&entry.response.headers, "Content-Length").is_some()
}

/// Prefer 206 over 200; then Accept-Ranges; then later index (redirect chain end).
fn download_entry_score(entry: &HarEntry, index: usize) -> (bool, bool, usize) {
    let has_accept_ranges = get_header(&entry.response.headers, "Accept-Ranges")
        .map(|v| v.eq_ignore_ascii_case("bytes"))
        .unwrap_or(false);
    (
        entry.response.status == 206,
        has_accept_ranges,
        index,
    )
}

/// Best entry that looks like a download, or None if none match.
fn select_download_entry(entries: &[HarEntry]) -> Option<usize> {
    entries
        .iter()
        .enumerate()
        .filter(|(_, e)| response_looks_like_download(e))
        .max_by_key(|(i, e)| download_entry_score(e, *i))
        .map(|(i, _)| i)
}

fn get_header<'a>(headers: &'a [HarHeader], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}
