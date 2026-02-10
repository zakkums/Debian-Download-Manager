//! Progress reporting for downloads (bytes done, ETA, rate).
//!
//! Used by the scheduler to report progress to the CLI; consumers can compute
//! rate = bytes_done / elapsed_secs and ETA = (total_bytes - bytes_done) / rate.

/// Snapshot of download progress for one job (CLI-friendly).
#[derive(Debug, Clone)]
pub struct ProgressStats {
    /// Bytes written so far (completed segments).
    pub bytes_done: u64,
    /// Total file size in bytes.
    pub total_bytes: u64,
    /// Elapsed time since download start (seconds).
    pub elapsed_secs: f64,
    /// Number of segments completed.
    pub segments_done: usize,
    /// Total number of segments.
    pub segment_count: usize,
}

impl ProgressStats {
    /// Total download rate in bytes per second (0 if elapsed is 0).
    pub fn bytes_per_sec(&self) -> f64 {
        if self.elapsed_secs <= 0.0 {
            return 0.0;
        }
        self.bytes_done as f64 / self.elapsed_secs
    }

    /// Estimated seconds remaining (None if rate is 0 or already done).
    pub fn eta_secs(&self) -> Option<f64> {
        let remaining = self.total_bytes.saturating_sub(self.bytes_done);
        if remaining == 0 {
            return Some(0.0);
        }
        let rate = self.bytes_per_sec();
        if rate <= 0.0 {
            return None;
        }
        Some(remaining as f64 / rate)
    }

    /// Fraction complete in [0.0, 1.0].
    pub fn fraction(&self) -> f64 {
        if self.total_bytes == 0 {
            return 1.0;
        }
        (self.bytes_done as f64 / self.total_bytes as f64).min(1.0)
    }
}
