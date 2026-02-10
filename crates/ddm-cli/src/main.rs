use ddm_core::logging;

mod cli;

use crate::cli::CliCommand;

#[tokio::main]
async fn main() {
    // Initialize logging as early as possible.
    logging::init_logging().expect("failed to initialize logging");

    // Parse CLI and dispatch.
    if let Err(err) = CliCommand::run_from_args().await {
        eprintln!("ddm error: {:#}", err);
        std::process::exit(1);
    }
}

