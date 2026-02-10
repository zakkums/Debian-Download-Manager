//! Persistent resume/job database (SQLite via sqlx).
//!
//! Stores jobs, filenames, sizes, segment completion bitmaps, and
//! ETag/Last-Modified metadata for safe resume.

pub mod types;
pub mod db;

pub use types::*;
pub use db::*;

