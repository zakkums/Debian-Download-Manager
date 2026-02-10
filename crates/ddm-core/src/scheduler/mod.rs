//! Job and segment scheduler.
//!
//! Coordinates jobs, per-host concurrency, and the download pipeline:
//! fetch_head → safe_resume validation → segmenter → downloader → storage.

mod choose;
mod execute;
mod run;

pub use run::{run_one_job, run_next_job};
