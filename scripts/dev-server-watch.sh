#!/usr/bin/env bash
set -uo pipefail

api_port=${ORBIT_DEV_API_PORT:-8080}
marker=$(mktemp)
backend_pid=""

stop_backend() {
    if [ -n "$backend_pid" ]; then
        kill "$backend_pid" 2>/dev/null || true
        wait "$backend_pid" 2>/dev/null || true
        backend_pid=""
    fi
}

cleanup() {
    stop_backend
    rm -f "$marker"
}
trap 'cleanup; exit 0' INT TERM
trap cleanup EXIT

start_backend() {
    touch "$marker"
    (
        child_pid=""
        stop_child() {
            if [ -n "$child_pid" ]; then
                kill "$child_pid" 2>/dev/null || true
                wait "$child_pid" 2>/dev/null || true
                child_pid=""
            fi
        }
        trap 'stop_child; exit 0' INT TERM
        trap stop_child EXIT

        ORBIT__ENVIRONMENT=development cargo run -p orbit-server -- migrate run --seed &
        child_pid=$!
        wait "$child_pid" || exit $?
        child_pid=""

        ORBIT__ENVIRONMENT=development cargo run -p orbit-server -- serve --listen "127.0.0.1:${api_port}" &
        child_pid=$!
        wait "$child_pid"
    ) &
    backend_pid=$!
}

changed_sources() {
    find \
        Cargo.toml Cargo.lock rust-toolchain.toml \
        apps/server/Cargo.toml apps/server/src apps/server/migrations \
        crates/orbit/Cargo.toml crates/orbit/src \
        crates/platform/Cargo.toml crates/platform/src \
        -type f \( -name '*.rs' -o -name '*.sql' -o -name '*.toml' -o -name 'Cargo.lock' \) \
        -newer "$marker" -print -quit | grep -q .
}

start_backend
while true; do
    sleep 0.5
    if changed_sources; then
        printf '\nBackend source changed. Rebuilding and restarting Orbit...\n' >&2
        stop_backend
        start_backend
    elif [ -n "$backend_pid" ] && ! kill -0 "$backend_pid" 2>/dev/null; then
        wait "$backend_pid" 2>/dev/null || true
        backend_pid=""
        printf '\nOrbit stopped. Change a backend source file to rebuild it.\n' >&2
    fi
done
