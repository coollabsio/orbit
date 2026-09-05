mod cli;

use std::process::ExitCode;

use clap::Parser;

#[tokio::main]
async fn main() -> ExitCode {
    let format = if std::env::var("ORBIT_ENV").as_deref() == Ok("development") {
        orbit_platform::TracingFormat::Compact
    } else {
        orbit_platform::TracingFormat::Json
    };
    let _ = orbit_platform::init_tracing(format);
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
