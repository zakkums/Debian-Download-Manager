//! Benchmark mode: try different segment counts and report throughput + events.
//!
//! Runs controlled downloads (4, 8, 16 segments) over a capped byte range so
//! the benchmark doesn't download the whole file multiple times. Reports
//! throughput (MiB/s), throttle events, error events, and a recommended
//! segment count.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::time::Instant;

use crate::config::DdmConfig;
use crate::downloader::{self, DownloadSummary};
use crate::fetch_head;
use crate::retry::RetryPolicy;
use crate::segmenter;
use crate::storage;

/// Default cap for benchmark download size (20 MiB per run) so 4/8/16 runs stay bounded.
const DEFAULT_BENCH_BYTES: u64 = 20 * 1024 * 1024;

/// Result of one benchmark run (one segment count).
#[derive(Debug, Clone)]
pub struct BenchResult {
    pub segment_count: usize,
    pub bytes_downloaded: u64,
    pub elapsed_secs: f64,
    pub throughput_mib_s: f64,
    pub throttle_events: u32,
    pub error_events: u32,
}

/// Runs benchmark: HEAD, then for each segment count in [4, 8, 16] downloads
/// up to `max_bytes` (capped by content length), measures throughput and events.
/// Uses empty headers unless provided. Runs on the current thread (call from
/// `spawn_blocking` if used from async).
pub fn run_bench(
    url: &str,
    headers: &HashMap<String, String>,
    cfg: &DdmConfig,
    max_bytes: Option<u64>,
) -> Result<Vec<BenchResult>> {
    let head = fetch_head::probe(url, headers).context("HEAD request failed")?;
    if !head.accept_ranges {
        anyhow::bail!("server does not support Range requests (Accept-Ranges: bytes)");
    }
    let total_size = head
        .content_length
        .ok_or_else(|| anyhow::anyhow!("server did not send Content-Length"))?;
    let cap = max_bytes.unwrap_or(DEFAULT_BENCH_BYTES).min(total_size);
    if cap == 0 {
        anyhow::bail!("content length is 0");
    }

    let segment_counts = [4_usize, 8, 16];
    let mut results = Vec::with_capacity(segment_counts.len());
    let retry_policy = RetryPolicy::default();

    for &segment_count in &segment_counts {
        let segment_count = segment_count.min(cap as usize).max(1);
        let segments = segmenter::plan_segments(cap, segment_count);
        if segments.is_empty() {
            continue;
        }

        let temp_dir = tempfile::tempdir().context("create temp dir for bench")?;
        let temp_path = temp_dir.path().join("bench.part");
        let mut builder = storage::StorageWriterBuilder::create(&temp_path)
            .with_context(|| format!("create temp file: {}", temp_path.display()))?;
        builder.preallocate(cap)?;
        let storage_writer = builder.build();
        let mut bitmap = segmenter::SegmentBitmap::new(segments.len());
        let mut summary = DownloadSummary::default();

        let start = Instant::now();
        let download_result = downloader::download_segments(
            url,
            headers,
            &segments,
            &storage_writer,
            &mut bitmap,
            Some(
                segment_count
                    .min(cfg.max_connections_per_host)
                    .min(cfg.max_total_connections),
            ),
            Some(&retry_policy),
            &mut summary,
            None,
            None,
            None,
            downloader::CurlOptions::default(),
        );
        let elapsed = start.elapsed().as_secs_f64();

        let bytes_downloaded = if download_result.is_ok() {
            segments.iter().map(|s| s.end - s.start).sum()
        } else {
            // Partial: count completed segments
            segments
                .iter()
                .enumerate()
                .filter(|(i, _)| bitmap.is_completed(*i))
                .map(|(_, s)| s.end - s.start)
                .sum()
        };

        let throughput_mib_s = if elapsed > 0.0 && bytes_downloaded > 0 {
            (bytes_downloaded as f64 / 1_048_576.0) / elapsed
        } else {
            0.0
        };

        results.push(BenchResult {
            segment_count,
            bytes_downloaded,
            elapsed_secs: elapsed,
            throughput_mib_s,
            throttle_events: summary.throttle_events,
            error_events: summary.error_events,
        });
    }

    Ok(results)
}

/// Picks a recommended segment count: prefer best throughput among runs with no errors;
/// if all have errors, return best throughput overall.
pub fn recommend_segment_count(results: &[BenchResult]) -> Option<usize> {
    if results.is_empty() {
        return None;
    }
    let best_no_errors = results
        .iter()
        .filter(|r| r.error_events == 0)
        .max_by(|a, b| {
            a.throughput_mib_s
                .partial_cmp(&b.throughput_mib_s)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    let best = best_no_errors.or_else(|| {
        results.iter().max_by(|a, b| {
            a.throughput_mib_s
                .partial_cmp(&b.throughput_mib_s)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    })?;
    Some(best.segment_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommend_prefers_no_errors() {
        let results = vec![
            BenchResult {
                segment_count: 4,
                bytes_downloaded: 1000,
                elapsed_secs: 1.0,
                throughput_mib_s: 1.0,
                throttle_events: 0,
                error_events: 0,
            },
            BenchResult {
                segment_count: 16,
                bytes_downloaded: 2000,
                elapsed_secs: 1.0,
                throughput_mib_s: 2.0,
                throttle_events: 0,
                error_events: 1,
            },
        ];
        assert_eq!(recommend_segment_count(&results), Some(4));
    }

    #[test]
    fn recommend_fallback_when_all_have_errors() {
        let results = vec![
            BenchResult {
                segment_count: 8,
                bytes_downloaded: 1000,
                elapsed_secs: 1.0,
                throughput_mib_s: 2.0,
                throttle_events: 0,
                error_events: 1,
            },
            BenchResult {
                segment_count: 4,
                bytes_downloaded: 500,
                elapsed_secs: 1.0,
                throughput_mib_s: 1.0,
                throttle_events: 0,
                error_events: 1,
            },
        ];
        assert_eq!(recommend_segment_count(&results), Some(8));
    }
}
