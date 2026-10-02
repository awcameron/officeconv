use clap::Parser;
use officeconv::cli::Cli;

fn main() {
    let cli = Cli::parse();
    println!("{cli:#?}");
}
