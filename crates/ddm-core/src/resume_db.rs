//! Persistent resume/job database (SQLite via sqlx).
//!
//! Stores jobs, filenames, sizes, segment completion bitmaps, and
//! ETag/Last-Modified metadata for safe resume.

use anyhow::Result;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Pool, Row, Sqlite};
use std::time::{SystemTime, UNIX_EPOCH};

/// Job identifier.
pub type JobId = i64;

/// High-level job state stored as a string in the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Running,
    Paused,
    Completed,
    Error,
}

impl JobState {
    fn as_str(self) -> &'static str {
        match self {
            JobState::Queued => "queued",
            JobState::Running => "running",
            JobState::Paused => "paused",
            JobState::Completed => "completed",
            JobState::Error => "error",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "queued" => JobState::Queued,
            "running" => JobState::Running,
            "paused" => JobState::Paused,
            "completed" => JobState::Completed,
            "error" => JobState::Error,
            _ => JobState::Error,
        }
    }
}

/// Minimal per-job settings container, stored as JSON in the DB.
///
/// This keeps the schema flexible while still allowing structured config
/// per job (segment limits, bandwidth caps, etc.) as we extend the core.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct JobSettings {
    /// Reserved for future per-job tuning (e.g., segment bounds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Summary view used by the CLI `status` command.
#[derive(Debug, Clone)]
pub struct JobSummary {
    pub id: JobId,
    pub url: String,
    pub state: JobState,
    pub final_filename: Option<String>,
    pub total_size: Option<i64>,
}

/// Handle to the SQLite-backed job database.
///
/// The database file is stored under the XDG state directory:
/// `~/.local/state/ddm/jobs.db` on Debian.
#[derive(Clone)]
pub struct ResumeDb {
    pool: Pool<Sqlite>,
}

impl ResumeDb {
    /// Open (or create) the default job database and run migrations.
    pub async fn open_default() -> Result<Self> {
        let xdg_dirs = xdg::BaseDirectories::with_prefix("ddm")?;
        let state_dir = xdg_dirs.get_state_home();
        let db_path = state_dir.join("jobs.db");

        // Ensure parent directory exists.
        tokio::fs::create_dir_all(&state_dir).await?;

        let uri = format!("sqlite://{}", db_path.display());
        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect(&uri)
            .await?;

        let db = ResumeDb { pool };
        db.migrate().await?;
        Ok(db)
    }

    async fn migrate(&self) -> Result<()> {
        // Single-table schema focused on jobs. Additional tables (per-segment
        // metadata, host policy, etc.) can be added later.
        //
        // - `completed_bitmap` is a compact bitmap of finished segments.
        // - `settings_json` holds per-job settings as JSON for flexibility.
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                url TEXT NOT NULL,
                final_filename TEXT,
                temp_filename TEXT,
                total_size INTEGER,
                etag TEXT,
                last_modified TEXT,
                segment_count INTEGER NOT NULL DEFAULT 0,
                completed_bitmap BLOB NOT NULL DEFAULT x'',
                state TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                settings_json TEXT
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert a new queued job with minimal information.
    ///
    /// Metadata such as size, ETag, and segment layout will be filled in
    /// later by the HEAD/segmenter logic.
    pub async fn add_job(&self, url: &str, settings: &JobSettings) -> Result<JobId> {
        let now = unix_timestamp();
        let state = JobState::Queued.as_str();
        let settings_json = serde_json::to_string(settings)?;

        let row_id = sqlx::query(
            r#"
            INSERT INTO jobs (
                url, final_filename, temp_filename, total_size,
                etag, last_modified, segment_count, completed_bitmap,
                state, created_at, updated_at, settings_json
            ) VALUES (?1, NULL, NULL, NULL,
                      NULL, NULL, 0, x'',
                      ?2, ?3, ?4, ?5)
            "#,
        )
        .bind(url)
        .bind(state)
        .bind(now)
        .bind(now)
        .bind(settings_json)
        .execute(&self.pool)
        .await?
        .last_insert_rowid();

        Ok(row_id)
    }

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

    /// Update the state of an existing job.
    pub async fn set_state(&self, id: JobId, state: JobState) -> Result<()> {
        let now = unix_timestamp();
        sqlx::query(
            r#"
            UPDATE jobs
            SET state = ?1,
                updated_at = ?2
            WHERE id = ?3
            "#,
        )
        .bind(state.as_str())
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Permanently remove a job row from the database.
    ///
    /// File cleanup is handled separately by higher layers.
    pub async fn remove_job(&self, id: JobId) -> Result<()> {
        sqlx::query(
            r#"
            DELETE FROM jobs
            WHERE id = ?1
            "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Open an in-memory database for tests (no disk I/O).
    async fn open_memory() -> Result<ResumeDb> {
        // Single connection to avoid in-memory pool handing back a different empty DB.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await?;
        let db = ResumeDb { pool };
        db.migrate().await?;
        Ok(db)
    }

    #[tokio::test]
    async fn job_state_roundtrip_via_db() {
        let db = open_memory().await.unwrap();
        let id = db
            .add_job("https://example.com/file.bin", &JobSettings::default())
            .await
            .unwrap();
        let jobs = db.list_jobs().await.unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, id);
        assert_eq!(jobs[0].state, JobState::Queued);
        assert_eq!(jobs[0].url, "https://example.com/file.bin");

        db.set_state(id, JobState::Running).await.unwrap();
        let jobs = db.list_jobs().await.unwrap();
        assert_eq!(jobs[0].state, JobState::Running);

        db.set_state(id, JobState::Paused).await.unwrap();
        let jobs = db.list_jobs().await.unwrap();
        assert_eq!(jobs[0].state, JobState::Paused);

        db.set_state(id, JobState::Completed).await.unwrap();
        let jobs = db.list_jobs().await.unwrap();
        assert_eq!(jobs[0].state, JobState::Completed);
    }

    #[tokio::test]
    async fn add_list_remove_jobs() {
        let db = open_memory().await.unwrap();
        assert!(db.list_jobs().await.unwrap().is_empty());

        let id1 = db
            .add_job("https://a.com/one", &JobSettings::default())
            .await
            .unwrap();
        let id2 = db
            .add_job("https://b.com/two", &JobSettings::default())
            .await
            .unwrap();
        let jobs = db.list_jobs().await.unwrap();
        assert_eq!(jobs.len(), 2);
        // Newest first
        assert_eq!(jobs[0].url, "https://b.com/two");
        assert_eq!(jobs[0].id, id2);
        assert_eq!(jobs[1].url, "https://a.com/one");
        assert_eq!(jobs[1].id, id1);

        db.remove_job(id1).await.unwrap();
        let jobs = db.list_jobs().await.unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, id2);
    }

    #[tokio::test]
    async fn job_settings_serialized_in_db() {
        let db = open_memory().await.unwrap();
        let settings = JobSettings {
            note: Some("test job".to_string()),
        };
        let id = db
            .add_job("https://example.com/x", &settings)
            .await
            .unwrap();
        let jobs = db.list_jobs().await.unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, id);
        // Summary doesn't include settings; we only check add_job accepted the struct
    }
}

