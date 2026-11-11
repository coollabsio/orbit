set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

# Orbit requires an app key. Local recipes use this public development key unless the environment or
# .env.local sets one. Never use it for real data.
export ORBIT_APP_KEY := env("ORBIT_APP_KEY", "b3JiaXQtZGV2ZWxvcG1lbnQta2V5LW5vdC1zZWNyZXQ=")

setup:
    cargo --version
    bun --version
    just --version
    bun install --frozen-lockfile
    cd apps/web && bun run build

dev:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ -f .env.local ]; then
        set -a
        source .env.local
        set +a
    fi
    api_port="${ORBIT_DEV_API_PORT:-8080}"
    web_port="${ORBIT_DEV_WEB_PORT:-8888}"
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
    ORBIT_DEV_API_PORT="${api_port}" ORBIT_DEV_WEB_PORT="${web_port}" scripts/dev-server-watch.sh &
    server_pid=$!
    (cd apps/web && VITE_API_PROXY="http://127.0.0.1:${api_port}" bun run dev -- --host 127.0.0.1 --port "${web_port}" --strictPort) &
    web_pid=$!
    wait -n "$server_pid" "$web_pid"

# Metro for the Expo app (apps/mobile). The phone loads it from ORBIT_MOBILE_HOST (for example a Tailscale name).
mobile:
    #!/usr/bin/env bash
    set -euo pipefail
    cd apps/mobile
    if [ -f .env.local ]; then
        set -a
        source .env.local
        set +a
    fi
    REACT_NATIVE_PACKAGER_HOSTNAME="${ORBIT_MOBILE_HOST:-${REACT_NATIVE_PACKAGER_HOSTNAME:-}}" bunx expo start --port "${ORBIT_MOBILE_PORT:-8082}"

test:
    just version-check
    cargo test --workspace
    cd apps/web && bun run test
    cd packages/api && bun run test
    cd packages/editor && bun run test && bun run typecheck

version-check:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo_version=$(sed -n 's/^version = "\([^"]*\)"$/\1/p' Cargo.toml)
    web_version=$(bun -e 'console.log(require("./apps/web/package.json").version)')
    test -n "$cargo_version"
    test "$web_version" = "$cargo_version" || {
        echo "version mismatch: Cargo workspace is $cargo_version, web app is $web_version" >&2
        exit 1
    }

check:
    just version-check
    cd apps/web && bun run build
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    cd apps/web && bun run lint
    cd apps/web && bun run test
    cd packages/api && bun run test
    cd packages/editor && bun run test && bun run typecheck
    cd apps/mobile && bunx tsc --noEmit
    just api-check
    cargo build --release --workspace

api:
    cargo run -p orbit-server -- openapi --output packages/api/src/generated/openapi.json
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
    diff -ru packages/api/src/generated "$first"

# Make a SQLite and a Postgres migration file with a UTC timestamp version, e.g. `just new-migration task_colors`.
# Add `destructive` to take a backup before it runs: `just new-migration drop_old_table destructive`.
new-migration name kind="":
    #!/usr/bin/env bash
    set -euo pipefail
    [[ "{{name}}" =~ ^[a-z0-9_]+$ ]] || { echo "use lowercase letters, digits and _ in the name"; exit 1; }
    case "{{kind}}" in "") suffix="" ;; destructive) suffix=".destructive" ;; *) echo "kind must be empty or destructive"; exit 1 ;; esac
    file="$(date -u +%Y%m%d%H%M%S)_{{name}}${suffix}.sql"
    echo "-- SQLite" > "apps/server/migrations/$file"
    echo "-- Postgres: the same change as apps/server/migrations/$file" > "apps/server/migrations/postgres/$file"
    echo "apps/server/migrations/$file"
    echo "apps/server/migrations/postgres/$file"

db-reset:
    ORBIT_ENV=development cargo run -p orbit-server -- db-reset --yes

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
    setup_url=$(ORBIT_ENV=development cargo run -q -p orbit-server -- --database "$data_dir/orbit.sqlite" setup-token init --origin http://127.0.0.1:8888)
    ORBIT_ENV=development cargo run -p orbit-server -- --database "$data_dir/orbit.sqlite" --attachments "$data_dir/attachments" --backups "$data_dir/backups" serve --listen 127.0.0.1:18080 --origin http://127.0.0.1:8888 &
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

# Print the Orbit version, or set a new one: `just version 0.2.0`.
version *new:
    bash scripts/version.sh {{ new }}

release-image tag=("orbit:" + `bash scripts/version.sh`):
    docker build --build-arg ORBIT_BUILD_REVISION="$(git rev-parse HEAD)" -t "{{ tag }}" .
    docker run --rm "{{ tag }}" --help
