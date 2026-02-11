//! Job read operations: list and get.

use anyhow::Result;
use sqlx::Row;

use super::super::db::ResumeDb;
use super::super::types::{JobDetails, JobId, JobState, JobSettings, JobSummary};

impl ResumeDb {
    /// List all jobs in the database, newest first.
    pub async fn list_jobs(&self) -> Result<Vec<JobSummary>> {
        let rows = sqlx::query(
            r#"
            SELECT id, url, state, final_filename, total_size
            FROM jobs
            ORDER BY created_at DESC, id DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let id: i64 = row.get("id");
            let url: String = row.get("url");
            let state_str: String = row.get("state");
            let final_filename: Option<String> = row.get("final_filename");
            let total_size: Option<i64> = row.get("total_size");

            out.push(JobSummary {
                id,
                url,
                state: JobState::from_str(&state_str),
                final_filename,
                total_size,
            });
        }

        Ok(out)
    }

    /// Returns final_filename of all jobs that use the given download_dir (for collision detection).
    /// If download_dir is None, only jobs with no stored download_dir are considered (legacy).
    /// exclude_job_id, if set, is omitted from the list (e.g. current job when updating metadata).
    pub async fn list_final_filenames_in_dir(
        &self,
        download_dir: Option<&str>,
        exclude_job_id: Option<JobId>,
    ) -> Result<Vec<String>> {
        let rows = sqlx::query(
            r#"SELECT id, final_filename, settings_json FROM jobs"#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::new();
        for row in rows {
            let id: i64 = row.get("id");
            if exclude_job_id == Some(id) {
                continue;
            }
            let final_filename: Option<String> = row.get("final_filename");
            let settings_json: Option<String> = row.get("settings_json");
            let job_dir = settings_json
                .as_deref()
                .filter(|s| !s.is_empty())
                .and_then(|s| serde_json::from_str::<JobSettings>(s).ok())
                .and_then(|s| s.download_dir);
            let same_dir = match (download_dir, job_dir.as_deref()) {
                (None, None) => true,
                (Some(a), Some(b)) => a == b,
                _ => false,
            };
            if same_dir {
                if let Some(name) = final_filename {
                    out.push(name);
                }
            }
        }
        Ok(out)
    }

    /// Fetch a single job row with full metadata for the scheduler.
    pub async fn get_job(&self, id: JobId) -> Result<Option<JobDetails>> {
        let row = sqlx::query(
            r#"
            SELECT
                id, url, final_filename, temp_filename, total_size,
                etag, last_modified, segment_count, completed_bitmap,
                state, created_at, updated_at, settings_json
            FROM jobs
            WHERE id = ?1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let id: i64 = row.get("id");
        let url: String = row.get("url");
        let final_filename: Option<String> = row.get("final_filename");
        let temp_filename: Option<String> = row.get("temp_filename");
        let total_size: Option<i64> = row.get("total_size");
        let etag: Option<String> = row.get("etag");
        let last_modified: Option<String> = row.get("last_modified");
        let segment_count: i64 = row.get("segment_count");
        let completed_bitmap: Vec<u8> = row.get("completed_bitmap");
        let state_str: String = row.get("state");
        let created_at: i64 = row.get("created_at");
        let updated_at: i64 = row.get("updated_at");
        let settings_json: Option<String> = row.get("settings_json");

        let settings = settings_json
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| serde_json::from_str::<JobSettings>(s))
            .transpose()?
            .unwrap_or_default();

        Ok(Some(JobDetails {
            id,
            url,
            final_filename,
            temp_filename,
            total_size,
            etag,
            last_modified,
            segment_count,
            completed_bitmap,
            state: JobState::from_str(&state_str),
            created_at,
            updated_at,
            settings,
        }))
    }
}
