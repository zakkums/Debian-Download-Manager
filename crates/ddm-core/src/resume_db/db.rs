//! SQLite-backed job database implementation.

use anyhow::Result;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Pool, Row, Sqlite};
use std::time::{SystemTime, UNIX_EPOCH};

use super::types::{JobDetails, JobId, JobMetadata, JobSettings, JobState, JobSummary};

/// Handle to the SQLite-backed job database.
///
/// The database file is stored under the XDG state directory:
/// `~/.local/state/ddm/jobs.db` on Debian.
#[derive(Clone)]
pub struct ResumeDb {
    pub(crate) pool: Pool<Sqlite>,
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

    /// Update metadata fields for an existing job after HEAD/segment planning.
    pub async fn update_metadata(&self, id: JobId, meta: &JobMetadata) -> Result<()> {
        let now = unix_timestamp();
        sqlx::query(
            r#"
            UPDATE jobs
            SET final_filename = ?1,
                temp_filename = ?2,
                total_size = ?3,
                etag = ?4,
                last_modified = ?5,
                segment_count = ?6,
                completed_bitmap = ?7,
                updated_at = ?8
            WHERE id = ?9
            "#,
        )
        .bind(&meta.final_filename)
        .bind(&meta.temp_filename)
        .bind(meta.total_size)
        .bind(&meta.etag)
        .bind(&meta.last_modified)
        .bind(meta.segment_count)
        .bind(&meta.completed_bitmap)
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update only the completed-segment bitmap (and updated_at).
    /// Used for durable progress: persist bitmap as segments complete so a crash doesn't lose progress.
    pub async fn update_bitmap(&self, id: JobId, bitmap: &[u8]) -> Result<()> {
        let now = unix_timestamp();
        sqlx::query(
            r#"
            UPDATE jobs
            SET completed_bitmap = ?1,
                updated_at = ?2
            WHERE id = ?3
            "#,
        )
        .bind(bitmap)
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Normalize any job left in `running` to `queued` (e.g. after a crash).
    /// Call before scheduling so stranded jobs are picked up again.
    /// Returns the number of jobs reset.
    pub async fn recover_running_jobs(&self) -> Result<u64> {
        let now = unix_timestamp();
        let r = sqlx::query(
            r#"
            UPDATE jobs
            SET state = 'queued',
                updated_at = ?1
            WHERE state = 'running'
            "#,
        )
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected())
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
    async fn recover_running_jobs_resets_to_queued() {
        let db = open_memory().await.unwrap();
        let id = db
            .add_job("https://example.com/x", &JobSettings::default())
            .await
            .unwrap();
        db.set_state(id, JobState::Running).await.unwrap();
        assert_eq!(db.list_jobs().await.unwrap()[0].state, JobState::Running);

        let n = db.recover_running_jobs().await.unwrap();
        assert_eq!(n, 1);
        let jobs = db.list_jobs().await.unwrap();
        assert_eq!(jobs[0].state, JobState::Queued);
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

    #[tokio::test]
    async fn get_job_and_update_metadata_roundtrip() {
        let db = open_memory().await.unwrap();
        let id = db
            .add_job("https://example.com/file.iso", &JobSettings::default())
            .await
            .unwrap();

        // Initially metadata should be defaults.
        let job = db.get_job(id).await.unwrap().expect("job exists");
        assert_eq!(job.id, id);
        assert_eq!(job.url, "https://example.com/file.iso");
        assert_eq!(job.final_filename, None);
        assert_eq!(job.temp_filename, None);
        assert_eq!(job.total_size, None);
        assert_eq!(job.segment_count, 0);
        assert!(job.completed_bitmap.is_empty());

        // Update metadata.
        let meta = JobMetadata {
            final_filename: Some("file.iso".to_string()),
            temp_filename: Some("file.iso.part".to_string()),
            total_size: Some(1024),
            etag: Some("etag-1".to_string()),
            last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".to_string()),
            segment_count: 4,
            completed_bitmap: vec![0b0000_1111],
        };
        db.update_metadata(id, &meta).await.unwrap();

        let job2 = db.get_job(id).await.unwrap().expect("job exists");
        assert_eq!(job2.final_filename.as_deref(), Some("file.iso"));
        assert_eq!(job2.temp_filename.as_deref(), Some("file.iso.part"));
        assert_eq!(job2.total_size, Some(1024));
        assert_eq!(job2.etag.as_deref(), Some("etag-1"));
        assert_eq!(job2.completed_bitmap, vec![0b0000_1111]);

        // update_bitmap only changes the bitmap (durable progress).
        db.update_bitmap(id, &[0b1111_0000]).await.unwrap();
        let job3 = db.get_job(id).await.unwrap().expect("job exists");
        assert_eq!(job3.completed_bitmap, vec![0b1111_0000]);
        assert_eq!(job3.final_filename.as_deref(), Some("file.iso"));
        assert_eq!(
            job3.last_modified.as_deref(),
            Some("Wed, 21 Oct 2015 07:28:00 GMT")
        );
        assert_eq!(job3.segment_count, 4);
    }
}

