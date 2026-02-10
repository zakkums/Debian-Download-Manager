//! `ddm import-har <path>` – create job from HAR file.

use anyhow::Result;
use ddm_core::har;
use ddm_core::resume_db::{JobSettings, ResumeDb};
use std::path::Path;

pub async fn run_import_har(db: &ResumeDb, path: &Path, allow_cookies: bool) -> Result<()> {
    let spec = har::resolve_har(path, allow_cookies)?;
    let settings = JobSettings {
        note: None,
        custom_headers: if spec.headers.is_empty() {
            None
        } else {
            Some(spec.headers)
        },
    };
    let id = db.add_job(&spec.url, &settings).await?;
    println!("Added job {id} for URL: {}", spec.url);
    if allow_cookies
        && !settings
            .custom_headers
            .as_ref()
            .map_or(true, |h| h.is_empty())
    {
        println!("  (cookies included; stored with job)");
    }
    Ok(())
}
