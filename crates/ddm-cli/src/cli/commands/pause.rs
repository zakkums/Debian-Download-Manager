//! `ddm pause <id>` – pause a job.

use anyhow::Result;
use ddm_core::resume_db::{JobState, ResumeDb};

pub async fn run_pause(db: &ResumeDb, id: i64) -> Result<()> {
    db.set_state(id, JobState::Paused).await?;
    println!("Paused job {id}");
    Ok(())
}
