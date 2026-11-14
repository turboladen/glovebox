//! The `glovebox` binary.

use std::path::Path;

use clap::Parser;
use glovebox_cli::{Cli, run};

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let args = Cli::parse();
    match run(args, |k| std::env::var(k).ok(), Path::new("data")).await {
        Ok(text) => {
            println!("{text}");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("glovebox: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
