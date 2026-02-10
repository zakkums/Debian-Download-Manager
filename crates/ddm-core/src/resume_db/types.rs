//! Types used by the resume/job database.

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
    pub fn as_str(self) -> &'static str {
        match self {
            JobState::Queued => "queued",
            JobState::Running => "running",
            JobState::Paused => "paused",
            JobState::Completed => "completed",
            JobState::Error => "error",
        }
    }

    pub fn from_str(s: &str) -> Self {
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

/// Full job record used by the scheduler / downloader.
#[derive(Debug, Clone)]
pub struct JobDetails {
    pub id: JobId,
    pub url: String,
    pub final_filename: Option<String>,
    pub temp_filename: Option<String>,
    pub total_size: Option<i64>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub segment_count: i64,
    pub completed_bitmap: Vec<u8>,
    pub state: JobState,
    pub created_at: i64,
    pub updated_at: i64,
    pub settings: JobSettings,
}

/// Metadata fields updated after HEAD / segment planning.
#[derive(Debug, Clone)]
pub struct JobMetadata {
    pub final_filename: Option<String>,
    pub temp_filename: Option<String>,
    pub total_size: Option<i64>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub segment_count: i64,
    pub completed_bitmap: Vec<u8>,
}

