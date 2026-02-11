//! SQLite-backed job database implementation.
//!
//! Handles connection, migrations, and timestamp helpers. Job CRUD lives in `jobs`.

use anyhow::Result;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Pool, Sqlite};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

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

    /// Open (or create) the database at a specific path. Creates parent dirs if needed.
    /// Intended for tests so the DB can be placed in a temp directory.
    pub async fn open_at(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let uri = format!("sqlite://{}?mode=rwc", path.display());
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
}

/// Current time as Unix seconds (for DB timestamps). Pub for use by `jobs`.
pub(crate) fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
/// Open an in-memory database for tests (no disk I/O).
pub(crate) async fn open_memory() -> Result<ResumeDb> {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;
    let db = ResumeDb { pool };
    db.migrate().await?;
    Ok(db)
}

