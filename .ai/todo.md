# Orbit shell-less health check

- [x] Add a failing CLI/probe test for an exec-form health check.
- [x] Add the minimal Orbit health-check command.
- [x] Add an exec-form Docker `HEALTHCHECK` that does not require `/bin/sh`.
- [x] Run focused tests, formatting, lint checks, and container verification.
- [x] Review the final diff and record results.

## Review

- Root cause: Orbit uses a scratch runtime image, while Coolify generated a `CMD-SHELL` probe that required `/bin/sh`.
- Added `orbit healthcheck`, which probes the existing `/health/ready` endpoint with the configured bind address and port.
- The image now declares an exec-form health check, so Coolify preserves it instead of adding a shell-based probe.
- `cargo fmt --check`, strict Clippy, all `orbit-server` tests, Docker image build, image metadata inspection, and a live container health transition passed.
