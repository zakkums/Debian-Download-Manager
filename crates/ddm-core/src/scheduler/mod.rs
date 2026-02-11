//! Job and segment scheduler.
//!
//! Coordinates jobs, per-host concurrency, and the download pipeline:
//! fetch_head → safe_resume validation → segmenter → downloader → storage.
//! Supports a global connection budget so multiple jobs share max_total_connections.

mod budget;
mod choose;
mod execute;
mod parallel;
mod progress;
mod run;

pub use budget::GlobalConnectionBudget;
pub use parallel::run_jobs_parallel;
pub use progress::ProgressStats;
pub use run::{run_one_job, run_next_job};
