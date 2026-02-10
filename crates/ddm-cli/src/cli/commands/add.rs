//! `ddm add <url>` – add a new download job.

use anyhow::Result;
use ddm_core::resume_db::{JobSettings, ResumeDb};

pub async fn run_add(db: &ResumeDb, url: &str) -> Result<()> {
    let settings = JobSettings::default();
    let id = db.add_job(url, &settings).await?;
    println!("Added job {id} for URL: {url}");
    Ok(())
}
