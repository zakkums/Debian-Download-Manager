//! Persistent resume/job database (SQLite via sqlx).
//!
//! Stores jobs, filenames, sizes, segment completion bitmaps, and
//! ETag/Last-Modified metadata for safe resume.

pub mod db;
pub mod jobs;
pub mod types;

#[cfg(test)]
mod tests;

pub use db::ResumeDb;
pub use types::*;
