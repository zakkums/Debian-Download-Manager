//! Integration test: local HTTP server with Range support, multi-segment download and resume.
//!
//! Starts a minimal range-capable server, adds a job, runs it via the scheduler,
//! and asserts the downloaded file matches the served body.

mod common;

use ddm_core::config::{DdmConfig, DownloadBackend};
use ddm_core::host_policy::HostPolicy;
use ddm_core::resume_db::{JobSettings, JobState, ResumeDb};
use ddm_core::scheduler;
use tempfile::tempdir;

#[tokio::test]
async fn multi_segment_download_completes_and_file_matches() {
    let body: Vec<u8> = (0u8..100).cycle().take(64 * 1024).collect();
    let url = common::range_server::start(body.clone());

    let download_dir = tempdir().unwrap();
    let state_dir = tempdir().unwrap();
    let db_path = state_dir.path().join("jobs.db");
    let db = ResumeDb::open_at(&db_path).await.unwrap();

    db.add_job(&url, &JobSettings::default()).await.unwrap();
    let jobs = db.list_jobs().await.unwrap();
    let job_id = jobs[0].id;
    db.recover_running_jobs().await.unwrap();

    let cfg = DdmConfig::default();
    let mut host_policy = HostPolicy::new(cfg.min_segments, cfg.max_segments);
    scheduler::run_one_job(
        &db,
        job_id,
        false,
        false,
        &cfg,
        download_dir.path(),
        &mut host_policy,
        None,
        None,
        None,
    )
    .await
    .expect("run_one_job");

    let job = db.get_job(job_id).await.unwrap().expect("job exists");
    assert_eq!(job.state, JobState::Completed, "job should be completed");
    let final_name = job
        .final_filename
        .as_deref()
        .unwrap_or("download.bin");
    let final_path = download_dir.path().join(final_name);
    assert!(final_path.exists(), "final file should exist");
    let content = std::fs::read(&final_path).unwrap();
    assert_eq!(content.len(), body.len(), "file size must match");
    assert_eq!(content, body, "file content must match");
}

#[tokio::test]
async fn multi_backend_download_completes_and_file_matches() {
    let body: Vec<u8> = (0u8..100).cycle().take(64 * 1024).collect();
    let url = common::range_server::start(body.clone());

    let download_dir = tempdir().unwrap();
    let state_dir = tempdir().unwrap();
    let db_path = state_dir.path().join("jobs.db");
    let db = ResumeDb::open_at(&db_path).await.unwrap();

    db.add_job(&url, &JobSettings::default()).await.unwrap();
    let jobs = db.list_jobs().await.unwrap();
    let job_id = jobs[0].id;
    db.recover_running_jobs().await.unwrap();

    let mut cfg = DdmConfig::default();
    cfg.download_backend = Some(DownloadBackend::Multi);
    let mut host_policy = HostPolicy::new(cfg.min_segments, cfg.max_segments);
    scheduler::run_one_job(
        &db,
        job_id,
        false,
        false,
        &cfg,
        download_dir.path(),
        &mut host_policy,
        None,
        None,
        None,
    )
    .await
    .expect("run_one_job with multi backend");

    let job = db.get_job(job_id).await.unwrap().expect("job exists");
    assert_eq!(job.state, JobState::Completed, "multi backend should complete");
    let final_name = job
        .final_filename
        .as_deref()
        .unwrap_or("download.bin");
    let final_path = download_dir.path().join(final_name);
    assert!(final_path.exists(), "final file should exist");
    let content = std::fs::read(&final_path).unwrap();
    assert_eq!(content.len(), body.len(), "file size must match");
    assert_eq!(content, body, "file content must match");
}

#[tokio::test]
async fn head_blocked_falls_back_to_range_probe_and_completes() {
    let body: Vec<u8> = (0u8..100).cycle().take(32 * 1024).collect();
    let url = common::range_server::start_with_options(
        body.clone(),
        common::range_server::RangeServerOptions {
            head_allowed: false,
            support_ranges: true,
            advertise_ranges: true,
        },
    );

    let download_dir = tempdir().unwrap();
    let state_dir = tempdir().unwrap();
    let db_path = state_dir.path().join("jobs.db");
    let db = ResumeDb::open_at(&db_path).await.unwrap();

    db.add_job(&url, &JobSettings::default()).await.unwrap();
    let job_id = db.list_jobs().await.unwrap()[0].id;
    db.recover_running_jobs().await.unwrap();

    let cfg = DdmConfig::default();
    let mut host_policy = HostPolicy::new(cfg.min_segments, cfg.max_segments);
    scheduler::run_one_job(
        &db,
        job_id,
        false,
        false,
        &cfg,
        download_dir.path(),
        &mut host_policy,
        None,
        None,
        None,
    )
    .await
    .expect("run_one_job");

    let job = db.get_job(job_id).await.unwrap().expect("job exists");
    assert_eq!(job.state, JobState::Completed);
    let final_path = download_dir
        .path()
        .join(job.final_filename.as_deref().unwrap_or("download.bin"));
    let content = std::fs::read(&final_path).unwrap();
    assert_eq!(content, body);
}

#[tokio::test]
async fn no_range_server_falls_back_to_single_stream_get() {
    let body: Vec<u8> = (0u8..100).cycle().take(32 * 1024).collect();
    let url = common::range_server::start_with_options(
        body.clone(),
        common::range_server::RangeServerOptions {
            head_allowed: true,
            support_ranges: false,
            advertise_ranges: false,
        },
    );

    let download_dir = tempdir().unwrap();
    let state_dir = tempdir().unwrap();
    let db_path = state_dir.path().join("jobs.db");
    let db = ResumeDb::open_at(&db_path).await.unwrap();

    db.add_job(&url, &JobSettings::default()).await.unwrap();
    let job_id = db.list_jobs().await.unwrap()[0].id;
    db.recover_running_jobs().await.unwrap();

    let cfg = DdmConfig::default();
    let mut host_policy = HostPolicy::new(cfg.min_segments, cfg.max_segments);
    scheduler::run_one_job(
        &db,
        job_id,
        false,
        false,
        &cfg,
        download_dir.path(),
        &mut host_policy,
        None,
        None,
        None,
    )
    .await
    .expect("run_one_job");

    let job = db.get_job(job_id).await.unwrap().expect("job exists");
    assert_eq!(job.state, JobState::Completed);
    let final_path = download_dir
        .path()
        .join(job.final_filename.as_deref().unwrap_or("download.bin"));
    let content = std::fs::read(&final_path).unwrap();
    assert_eq!(content, body);
}
