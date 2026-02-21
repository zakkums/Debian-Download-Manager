//! Job control for pause/cancel: shared abort tokens and optional IPC.
//!
//! When the scheduler runs with a `JobControl`, each running job is registered
//! with an abort token. A control client (e.g. `ddm pause 1` via socket) can
//! request abort for a job; the download loop checks the token and stops.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

/// Error returned when a download is stopped by user (pause/cancel).
#[derive(Debug)]
pub struct JobAborted;

impl std::fmt::Display for JobAborted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "job aborted by user")
    }
}

impl std::error::Error for JobAborted {}

/// Shared registry of job id -> abort token. Used by the scheduler to pass
/// an abort token into each job and by the control socket to signal pause/cancel.
#[derive(Default)]
pub struct JobControl {
    jobs: RwLock<HashMap<i64, Arc<AtomicBool>>>,
}

impl JobControl {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a running job; returns the abort token to pass to the download phase.
    /// Call when starting a job; the token is set to true when pause/cancel is requested.
    pub fn register(&self, job_id: i64) -> Arc<AtomicBool> {
        let token = Arc::new(AtomicBool::new(false));
        self.jobs.write().unwrap().insert(job_id, Arc::clone(&token));
        token
    }

    /// Unregister a job (call when the job finishes, success or failure).
    pub fn unregister(&self, job_id: i64) {
        self.jobs.write().unwrap().remove(&job_id);
    }

    /// Request abort for a job (e.g. from control socket). The download loop will
    /// see the token set and stop; progress is persisted and state set to Paused.
    pub fn request_abort(&self, job_id: i64) {
        if let Some(token) = self.jobs.read().unwrap().get(&job_id) {
            token.store(true, Ordering::Relaxed);
        }
    }
}

/// Default path for the control socket (same XDG state dir as the DB).
pub fn default_control_socket_path() -> std::io::Result<PathBuf> {
    let dir = xdg::BaseDirectories::with_prefix("ddm")?.get_state_home();
    Ok(dir.join("control.sock"))
}
