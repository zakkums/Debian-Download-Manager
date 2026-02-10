//! `ddm resume <id>` – resume a paused job.

use anyhow::Result;
use ddm_core::resume_db::{JobState, ResumeDb};

pub async fn run_resume(db: &ResumeDb, id: i64) -> Result<()> {
    db.set_state(id, JobState::Queued).await?;
    println!("Resumed job {id}");
    Ok(())
}
