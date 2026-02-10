//! `ddm remove <id>` – remove a job.

use anyhow::Result;
use ddm_core::resume_db::ResumeDb;

pub async fn run_remove(db: &ResumeDb, id: i64) -> Result<()> {
    db.remove_job(id).await?;
    println!("Removed job {id}");
    Ok(())
}
