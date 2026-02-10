//! Resolve HAR file to direct URL and optional headers.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

use crate::resolver::ResolvedJobSpec;

use super::parse::{HarHeader, HarLog};

/// Resolves a HAR file to a direct URL (and optional headers).
///
/// Follows the redirect chain: for each entry, if the response is 301/302/307/308,
/// uses `redirectURL` or the `Location` header as the next URL. The final URL is
/// the last redirect target or the first request URL if no redirects.
///
/// If `include_cookies` is true, the `Cookie` header from the request that
/// corresponds to the final URL is included in the returned headers (for
/// cookie-based CDN auth). Otherwise no cookies are extracted or stored.
pub fn resolve_har(path: &Path, include_cookies: bool) -> Result<ResolvedJobSpec> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("read HAR file: {}", path.display()))?;
    let har: HarLog = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse HAR JSON: {}", path.display()))?;

    let entries = har.log.entries;
    if entries.is_empty() {
        anyhow::bail!("HAR file has no entries");
    }

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

    let final_entry_index = entries
        .iter()
        .enumerate()
        .rev()
        .find(|(_, e)| e.request.url == final_url)
        .map(|(i, _)| i)
        .unwrap_or(0);

    let mut headers = HashMap::new();
    if include_cookies {
        let entry = &entries[final_entry_index];
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

fn get_header<'a>(headers: &'a [HarHeader], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}
