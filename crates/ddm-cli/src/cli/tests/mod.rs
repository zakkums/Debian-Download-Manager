//! CLI parse tests (multi-file to keep each file <200 lines).

use super::{Cli, CliCommand};
use clap::Parser;

pub(super) fn parse(args: &[&str]) -> CliCommand {
    let cli = Cli::try_parse_from(args).unwrap();
    cli.command
}

mod add_run;
mod rest;
