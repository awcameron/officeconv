use clap::Parser;
use officeconv::cli::Cli;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    officeconv::run(&cli)?;
    Ok(())
}
