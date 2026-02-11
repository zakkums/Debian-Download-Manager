use super::*;

fn parse(args: &[&str]) -> CliCommand {
    let cli = Cli::try_parse_from(args).unwrap();
    cli.command
}

#[test]
fn cli_parse_add() {
    match parse(&["ddm", "add", "https://example.com/file.iso"]) {
        CliCommand::Add { url } => assert_eq!(url, "https://example.com/file.iso"),
        _ => panic!("expected Add"),
    }
}

#[test]
fn cli_parse_run() {
    match parse(&["ddm", "run"]) {
        CliCommand::Run { force_restart } => assert!(!force_restart),
        _ => panic!("expected Run"),
    }
}

#[test]
fn cli_parse_run_force_restart() {
    match parse(&["ddm", "run", "--force-restart"]) {
        CliCommand::Run { force_restart } => assert!(force_restart),
        _ => panic!("expected Run with force_restart"),
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
        CliCommand::Remove { id } => assert_eq!(id, 99),
        _ => panic!("expected Remove"),
    }
}

#[test]
fn cli_parse_import_har_without_cookies() {
    match parse(&["ddm", "import-har", "/path/to/file.har"]) {
        CliCommand::ImportHar { path, allow_cookies } => {
            assert_eq!(path, "/path/to/file.har");
            assert!(!allow_cookies);
        }
        _ => panic!("expected ImportHar"),
    }
}

#[test]
fn cli_parse_import_har_allow_cookies() {
    match parse(&["ddm", "import-har", "x.har", "--allow-cookies"]) {
        CliCommand::ImportHar { path, allow_cookies } => {
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

