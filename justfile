set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

setup:
    cargo --version
    bun --version
    just --version
    cd apps/web && bun install --frozen-lockfile

dev:
    #!/usr/bin/env bash
    set -euo pipefail
    server_pid=""
    web_pid=""
    cleanup() {
        for pid in "$server_pid" "$web_pid"; do
            [ -z "$pid" ] || kill "$pid" 2>/dev/null || true
        done
        for pid in "$server_pid" "$web_pid"; do
            [ -z "$pid" ] || wait "$pid" 2>/dev/null || true
        done
    }
    trap cleanup EXIT
    trap 'exit 0' INT TERM
    cargo run -p orbit-server -- serve --port 8080 &
    server_pid=$!
    (cd apps/web && bun run dev -- --port 8888) &
    web_pid=$!
    wait -n "$server_pid" "$web_pid"

test:
    cargo test --workspace
    cd apps/web && bun run test

check:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    cd apps/web && bun run lint
    cd apps/web && bun run test
    just api-check
    cd apps/web && bun run build
    cargo build --release --workspace

api:
    cargo run -p orbit-server -- openapi --output apps/web/src/api/generated/openapi.json

api-check:
    just api
    git diff --exit-code -- apps/web/src/api/generated

db-reset:
    ORBIT_ENV=development cargo run -p orbit-server -- db-reset

seed:
    ORBIT_ENV=development cargo run -p orbit-server -- seed

e2e:
    #!/usr/bin/env bash
    set -euo pipefail
    data_dir=$(mktemp -d)
    server_pid=""
    e2e_pid=""
    cleanup() {
        for pid in "$server_pid" "$e2e_pid"; do
            [ -z "$pid" ] || kill "$pid" 2>/dev/null || true
        done
        for pid in "$server_pid" "$e2e_pid"; do
            [ -z "$pid" ] || wait "$pid" 2>/dev/null || true
        done
        rm -rf "$data_dir"
    }
    trap cleanup EXIT
    trap 'exit 0' INT TERM
    ORBIT_ENV=e2e ORBIT_DATA_DIR="$data_dir" cargo run -p orbit-server -- serve --port 8080 &
    server_pid=$!
    (cd apps/web && bunx playwright test) &
    e2e_pid=$!
    wait -n "$server_pid" "$e2e_pid"

build:
    cargo build --release --workspace
    cd apps/web && bun run build
