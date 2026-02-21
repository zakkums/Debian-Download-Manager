use ddm_core::logging;

mod cli;

use crate::cli::CliCommand;

#[tokio::main]
async fn main() {
    if let Err(e) = logging::init_logging() {
        eprintln!("ddm: log file unavailable ({}), using stderr", e);
        logging::init_logging_stderr();
    }

    if let Err(err) = CliCommand::run_from_args().await {
        eprintln!("ddm error: {:#}", err);
        std::process::exit(1);
    }
}

