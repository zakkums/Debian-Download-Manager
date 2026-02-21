//! Tests for add and run subcommands.

use super::parse;
use crate::cli::CliCommand;

#[test]
fn cli_parse_add() {
    match parse(&["ddm", "add", "https://example.com/file.iso"]) {
        CliCommand::Add { url, download_dir } => {
            assert_eq!(url, "https://example.com/file.iso");
            assert!(download_dir.is_none());
        }
        _ => panic!("expected Add"),
    }
}

#[test]
fn cli_parse_add_download_dir() {
    match parse(&[
        "ddm",
        "add",
        "https://example.com/x",
        "--download-dir",
        "/tmp",
    ]) {
        CliCommand::Add { url, download_dir } => {
            assert_eq!(url, "https://example.com/x");
            assert_eq!(download_dir.as_deref(), Some(std::path::Path::new("/tmp")));
        }
        _ => panic!("expected Add with --download-dir"),
    }
}

#[test]
fn cli_parse_run() {
    match parse(&["ddm", "run"]) {
        CliCommand::Run {
            force_restart,
            jobs,
            overwrite,
        } => {
            assert!(!force_restart);
            assert_eq!(jobs, 1);
            assert!(!overwrite);
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn cli_parse_run_force_restart() {
    match parse(&["ddm", "run", "--force-restart"]) {
        CliCommand::Run {
            force_restart,
            jobs,
            overwrite,
        } => {
            assert!(force_restart);
            assert_eq!(jobs, 1);
            assert!(!overwrite);
        }
        _ => panic!("expected Run with force_restart"),
    }
}

#[test]
fn cli_parse_run_overwrite() {
    match parse(&["ddm", "run", "--overwrite"]) {
        CliCommand::Run { overwrite, .. } => assert!(overwrite),
        _ => panic!("expected Run with overwrite"),
    }
}

#[test]
fn cli_parse_run_jobs() {
    match parse(&["ddm", "run", "--jobs", "4"]) {
        CliCommand::Run {
            force_restart,
            jobs,
            overwrite,
        } => {
            assert!(!force_restart);
            assert_eq!(jobs, 4);
            assert!(!overwrite);
        }
        _ => panic!("expected Run with --jobs 4"),
    }
}
