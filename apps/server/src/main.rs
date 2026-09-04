pub mod auth_routes;
mod cli;
pub mod repositories;

use std::process::ExitCode;

use clap::Parser;

#[tokio::main]
async fn main() -> ExitCode {
    match cli::run(cli::Cli::parse()).await {
        Ok(output) => {
            if !output.is_empty() {
                println!("{output}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}", cli::failure_message(&error.to_string()));
            ExitCode::FAILURE
        }
    }
}
