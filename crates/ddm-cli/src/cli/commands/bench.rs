//! `ddm bench <url>` – benchmark segment counts.

use anyhow::{Context, Result};
use ddm_core::bench::{self, BenchResult};
use ddm_core::config;
use std::collections::HashMap;

fn print_bench_results(results: &[BenchResult]) {
    println!(
        "  {:>6}  {:>10}  {:>8}  {:>8}  {:>8}  {:>8}",
        "Segs", "Bytes", "Time(s)", "MiB/s", "Throttle", "Errors"
    );
    println!(
        "  {}  {}  {}  {}  {}  {}",
        "------", "----------", "--------", "--------", "--------", "------"
    );
    for r in results {
        println!(
            "  {:>6}  {:>10}  {:>8.2}  {:>8.2}  {:>8}  {:>8}",
            r.segment_count,
            r.bytes_downloaded,
            r.elapsed_secs,
            r.throughput_mib_s,
            r.throttle_events,
            r.error_events
        );
    }
}

pub async fn run_bench(url: &str) -> Result<()> {
    let cfg = config::load_or_init()?;
    let headers = HashMap::new();
    let results = tokio::task::spawn_blocking({
        let url = url.to_string();
        let cfg = cfg.clone();
        move || bench::run_bench(&url, &headers, &cfg, None)
    })
    .await
    .context("bench task join")??;
    print_bench_results(&results);
    if let Some(rec) = bench::recommend_segment_count(&results) {
        println!("Recommended segment count: {}", rec);
    }
    Ok(())
}
