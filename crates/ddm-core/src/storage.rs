//! Disk I/O and file lifecycle.
//!
//! Responsible for:
//! - Preallocating files with `fallocate`.
//! - Buffered, offset-based writes (pwrite-style).
//! - Atomic finalize (rename from `.part` to final name).
//! - fsync policy.

// Placeholder for upcoming implementation.

