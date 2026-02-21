//! Job write operations: add, update, state, remove.

use anyhow::Result;
use sqlx::Row;

use super::super::db::{unix_timestamp, ResumeDb};
use super::super::types::{JobId, JobMetadata, JobSettings, JobState};

impl ResumeDb {
    /// Atomically claim the next queued job (smallest id) by setting its state to Running.
    /// Returns the claimed job id, or None if no job is queued. Used by the parallel scheduler
    /// so multiple workers never pick the same job. Stranded Running jobs are reset by
    /// `recover_running_jobs()` before scheduling.
    pub async fn claim_next_queued_job(&self) -> Result<Option<JobId>> {
        let now = unix_timestamp();
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            r#"
            SELECT id FROM jobs
            WHERE state = 'queued'
            ORDER BY id ASC
            LIMIT 1
            "#,
        )
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        let id: i64 = row.get("id");
        sqlx::query(
            r#"
            UPDATE jobs
            SET state = 'running',
                updated_at = ?1
            WHERE id = ?2
            "#,
        )
        .bind(now)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(id))
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
