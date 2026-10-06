use std::io::ErrorKind;
use std::process::ExitCode;

use clap::Parser;
use officeconv::cli::Cli;
use officeconv::error::ConvertError;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match officeconv::run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        // The reader went away (e.g. `officeconv big.xlsx --to csv | head`); that's not a failure.
        Err(ConvertError::Write(err)) if err.kind() == ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(err) => {
            // Each error message already includes its cause, so print just that one line.
            eprintln!("Error: {err}");
            ExitCode::from(err.exit_code())
        }
    }
}
