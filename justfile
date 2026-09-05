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
    ORBIT__ENVIRONMENT=development cargo run -p orbit-server -- serve --listen 127.0.0.1:8080 --origin http://127.0.0.1:8888 &
    server_pid=$!
    (cd apps/web && bun run dev -- --port 8888) &
    web_pid=$!
    wait -n "$server_pid" "$web_pid"

test:
    cargo test --workspace
    cd apps/web && bun run test

check:
    cd apps/web && bun run build
    git diff --exit-code -- apps/web/dist
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    cd apps/web && bun run lint
    cd apps/web && bun run test
    just api-check
    cargo build --release --workspace

api:
    cargo run -p orbit-server -- openapi --output apps/web/src/api/generated/openapi.json
    cd apps/web && bun run api:generate

api-check:
    #!/usr/bin/env bash
    set -euo pipefail
    first=$(mktemp -d)
    second=$(mktemp -d)
    trap 'rm -rf "$first" "$second"' EXIT
    generate() {
        local output=$1
        cargo run -q -p orbit-server -- openapi --output "$output/openapi.json" >/dev/null
        (cd apps/web && bun run scripts/generate-api.ts "$output/openapi.json" "$output")
    }
    generate "$first"
    generate "$second"
    diff -ru "$first" "$second"
    diff -ru apps/web/src/api/generated "$first"

db-reset:
    ORBIT_ENV=development ORBIT__ENVIRONMENT=development cargo run -p orbit-server -- db-reset --yes

seed:
    ORBIT_ENV=development ORBIT__ENVIRONMENT=development cargo run -p orbit-server -- seed

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
    setup_url=$(ORBIT__ENVIRONMENT=development cargo run -q -p orbit-server -- --database "$data_dir/orbit.sqlite" setup-token init --origin http://127.0.0.1:8888)
    ORBIT__ENVIRONMENT=development cargo run -p orbit-server -- --database "$data_dir/orbit.sqlite" --attachments "$data_dir/attachments" --backups "$data_dir/backups" serve --listen 127.0.0.1:18080 --origin http://127.0.0.1:8888 &
    server_pid=$!
    (cd apps/web && ORBIT_SETUP_URL="$setup_url" bun x playwright test) &
    e2e_pid=$!
    wait -n "$server_pid" "$e2e_pid"

build:
    #!/usr/bin/env bash
    set -euo pipefail
    revision=$(git rev-parse HEAD)
    (cd apps/web && ORBIT_BUILD_REVISION="$revision" bun run build)
    ORBIT_BUILD_REVISION="$revision" cargo build --release --workspace

release-image tag="orbit:milestone-1":
    docker build --build-arg ORBIT_BUILD_REVISION="$(git rev-parse HEAD)" -t "{{ tag }}" .
    docker run --rm "{{ tag }}" --help
