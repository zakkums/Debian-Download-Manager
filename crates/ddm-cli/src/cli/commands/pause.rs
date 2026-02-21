//! `ddm pause <id>` – pause a job. If `ddm run` is active, signals it to stop the download.

use anyhow::Result;
use ddm_core::resume_db::{JobState, ResumeDb};

use crate::cli::control_socket;

pub async fn run_pause(db: &ResumeDb, id: i64) -> Result<()> {
    if let Ok(path) = ddm_core::control::default_control_socket_path() {
        let _ = control_socket::send_pause(&path, id).await;
    }
    db.set_state(id, JobState::Paused).await?;
    println!("Paused job {id}");
    Ok(())
}
