//! Single-stream HTTP GET downloader (non-Range fallback).
//!
//! Writes the response body sequentially to storage starting at offset 0.

use anyhow::{Context, Result};
use crate::storage::StorageWriter;
use super::CurlOptions;
use std::collections::HashMap;
use std::str;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Downloads a URL with a single GET (no Range), writing sequentially to `storage`.
/// Returns the number of bytes written.
pub fn download_single(
    url: &str,
    custom_headers: &HashMap<String, String>,
    storage: &StorageWriter,
    expected_len: Option<u64>,
    curl: CurlOptions,
) -> Result<u64> {
    let offset = Arc::new(AtomicU64::new(0));
    let offset_cb = Arc::clone(&offset);
    let storage = storage.clone();

    let mut easy = curl::easy::Easy::new();
    easy.url(url).context("invalid URL")?;
    easy.follow_location(true)?;
    easy.max_redirections(10)?;
    if let Some(speed) = curl.max_recv_speed {
        easy.max_recv_speed(speed).map_err(|e| anyhow::anyhow!("curl: {}", e))?;
    }
    if let Some(sz) = curl.buffer_size {
        easy.buffer_size(sz).map_err(|e| anyhow::anyhow!("curl: {}", e))?;
    }
    easy.connect_timeout(Duration::from_secs(30))?;
    easy.low_speed_limit(1024).map_err(|e| anyhow::anyhow!("curl: {}", e))?;
    easy.low_speed_time(Duration::from_secs(60))?;
    easy.timeout(Duration::from_secs(3600))?;

    let mut list = curl::easy::List::new();
    for (k, v) in custom_headers {
        list.append(&format!("{}: {}", k.trim(), v.trim()))?;
    }
    if !custom_headers.is_empty() {
        easy.http_headers(list)?;
    }

    {
        let mut transfer = easy.transfer();
        // Keep header function to ensure libcurl reads headers even if server uses weird responses;
        // also useful for debugging.
        transfer.header_function(|data| {
            let _ = str::from_utf8(data);
            true
        })?;
        transfer.write_function(move |data| {
            let off = offset_cb.fetch_add(data.len() as u64, Ordering::Relaxed);
            match storage.write_at(off, data) {
                Ok(()) => Ok(data.len()),
                Err(e) => {
                    tracing::warn!("single download write failed: {}", e);
                    Ok(0) // abort transfer
                }
            }
        })?;
        transfer.perform().context("GET request failed")?;
    }

    let code = easy.response_code().context("no response code")?;
    if code < 200 || code >= 300 {
        anyhow::bail!("GET {} returned HTTP {}", url, code);
    }

    let written = offset.load(Ordering::Relaxed);
    if let Some(exp) = expected_len {
        if written != exp {
            anyhow::bail!("partial transfer: wrote {} of {}", written, exp);
        }
    }
    Ok(written)
}

