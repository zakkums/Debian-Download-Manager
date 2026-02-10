//! `ddm status` – show status of all jobs.

use anyhow::Result;
use ddm_core::resume_db::ResumeDb;

pub async fn run_status(db: &ResumeDb) -> Result<()> {
    let jobs = db.list_jobs().await?;
    if jobs.is_empty() {
        println!("No jobs in database.");
    } else {
        println!("{:<6} {:<10} {:<10} {}", "ID", "STATE", "SIZE", "URL");
        for j in jobs {
            let size_str = j
                .total_size
                .map(|s| format!("{s}"))
                .unwrap_or_else(|| "-".to_string());
            println!(
                "{:<6} {:<10} {:<10} {}",
                j.id,
                format!("{:?}", j.state).to_lowercase(),
                size_str,
                j.url
            );
        }
    }
    Ok(())
}
