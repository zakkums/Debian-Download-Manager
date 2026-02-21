use super::*;

fn parse(args: &[&str]) -> CliCommand {
    let cli = Cli::try_parse_from(args).unwrap();
    cli.command
}

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

#[test]
fn cli_parse_status() {
    match parse(&["ddm", "status"]) {
        CliCommand::Status => {}
        _ => panic!("expected Status"),
    }
}

#[test]
fn cli_parse_pause() {
    match parse(&["ddm", "pause", "42"]) {
        CliCommand::Pause { id } => assert_eq!(id, 42),
        _ => panic!("expected Pause"),
    }
}

#[test]
fn cli_parse_resume() {
    match parse(&["ddm", "resume", "1"]) {
        CliCommand::Resume { id } => assert_eq!(id, 1),
        _ => panic!("expected Resume"),
    }
}

#[test]
fn cli_parse_remove() {
    match parse(&["ddm", "remove", "99"]) {
        CliCommand::Remove {
            id,
            delete_files,
            download_dir,
        } => {
            assert_eq!(id, 99);
            assert!(!delete_files);
            assert!(download_dir.is_none());
        }
        _ => panic!("expected Remove"),
    }
}

#[test]
fn cli_parse_remove_delete_files() {
    match parse(&["ddm", "remove", "1", "--delete-files"]) {
        CliCommand::Remove {
            id,
            delete_files,
            download_dir,
        } => {
            assert_eq!(id, 1);
            assert!(delete_files);
            assert!(download_dir.is_none());
        }
        _ => panic!("expected Remove with --delete-files"),
    }
}

#[test]
fn cli_parse_remove_delete_files_download_dir() {
    match parse(&[
        "ddm",
        "remove",
        "2",
        "--delete-files",
        "--download-dir",
        "/tmp",
    ]) {
        CliCommand::Remove {
            id,
            delete_files,
            download_dir,
        } => {
            assert_eq!(id, 2);
            assert!(delete_files);
            assert_eq!(download_dir.as_deref(), Some(std::path::Path::new("/tmp")));
        }
        _ => panic!("expected Remove with --delete-files --download-dir"),
    }
}

#[test]
fn cli_parse_import_har_without_cookies() {
    match parse(&["ddm", "import-har", "/path/to/file.har"]) {
        CliCommand::ImportHar {
            path,
            allow_cookies,
        } => {
            assert_eq!(path, "/path/to/file.har");
            assert!(!allow_cookies);
        }
        _ => panic!("expected ImportHar"),
    }
}

#[test]
fn cli_parse_import_har_allow_cookies() {
    match parse(&["ddm", "import-har", "x.har", "--allow-cookies"]) {
        CliCommand::ImportHar {
            path,
            allow_cookies,
        } => {
            assert_eq!(path, "x.har");
            assert!(allow_cookies);
        }
        _ => panic!("expected ImportHar"),
    }
}

#[test]
fn cli_parse_bench() {
    match parse(&["ddm", "bench", "https://example.com/large.bin"]) {
        CliCommand::Bench { url } => assert_eq!(url, "https://example.com/large.bin"),
        _ => panic!("expected Bench"),
    }
}

#[test]
fn cli_parse_checksum() {
    match parse(&["ddm", "checksum", "/path/to/file.bin"]) {
        CliCommand::Checksum { path } => assert_eq!(path, "/path/to/file.bin"),
        _ => panic!("expected Checksum"),
    }
}
