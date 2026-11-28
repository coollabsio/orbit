use std::process::ExitCode;

use include_dir::{Dir, include_dir};

/// Embedded here, not in the library: a web-only change then compiles only this small binary again. Call only
/// non-generic, non-async library functions here: a release build compiles again, in the binary, every generic
/// function and async state machine that the binary uses.
static FRONTEND: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../web/dist");

fn main() -> ExitCode {
    orbit_server::cli::main(&FRONTEND)
}
