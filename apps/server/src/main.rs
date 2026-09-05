mod cli;

use std::future::Future;
use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;

fn main() -> ExitCode {
    run_with_bounded_runtime(run())
}

fn run_with_bounded_runtime<F, T>(future: F) -> T
where
    F: Future<Output = T>,
{
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Tokio runtime initializes");
    let output = runtime.block_on(future);
    // App::serve already gives cooperative drains their full deadline. Never let a blocked
    // syscall in an abandoned task extend process shutdown beyond that deadline.
    runtime.shutdown_timeout(Duration::ZERO);
    output
}

async fn run() -> ExitCode {
    let format = if std::env::var("ORBIT__ENVIRONMENT").as_deref() == Ok("development")
        || std::env::var("ORBIT_ENV").as_deref() == Ok("development")
    {
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

#[cfg(test)]
mod runtime_tests {
    use super::*;

    #[test]
    fn process_runtime_teardown_does_not_join_blocked_work() {
        let started = std::time::Instant::now();
        run_with_bounded_runtime(async {
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            tokio::task::spawn_blocking(move || {
                let _ = started_tx.send(());
                std::thread::sleep(Duration::from_millis(500));
            });
            started_rx.await.unwrap();
        });

        assert!(started.elapsed() < Duration::from_millis(250));
    }
}
