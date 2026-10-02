use std::process::ExitCode;

use clap::Parser;
use officeconv::cli::Cli;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match officeconv::run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // Each error message already includes its cause, so print just that one line.
            eprintln!("Error: {err}");
            ExitCode::FAILURE
        }
    }
}
