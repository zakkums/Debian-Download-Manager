//! Tests for resume_db (use in-memory DB helper from db).

use crate::resume_db::db::open_memory;
use crate::resume_db::{JobMetadata, JobState, JobSettings};

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
        custom_headers: None,
        download_dir: None,
    };
    let id = db
        .add_job("https://example.com/x", &settings)
        .await
        .unwrap();
    let jobs = db.list_jobs().await.unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].id, id);
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
