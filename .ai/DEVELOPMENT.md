# Development and operations

## Requirements

- Rust 1.97.1 from `rust-toolchain.toml`
- Bun 1.3.14 or later
- Just
- Docker only for OCI builds

## Setup and local development

```bash
just setup
just dev
```

`just dev` starts Orbit at `http://127.0.0.1:8080` and Vite at `http://127.0.0.1:8888`. Vite proxies `/api`, `/mcp`, OAuth discovery, and OAuth protocol endpoints to the Rust server. The backend public origin follows the web port for OAuth redirects. Vite updates frontend code with hot module replacement. A lightweight watcher rebuilds and restarts the Rust server when backend source, migration, or Cargo files change. It runs pending migrations and idempotent development seeding before each backend start, so schema changes do not require a manual restart. The recipe records and stops only its own child processes.

`just db-reset` is an explicit development-only destructive reset. `just seed` is idempotent and prints the local seed account credentials after creating a representative workspace, project, workflow, and task.

The underlying commands are:

```bash
cd apps/web
bun install --frozen-lockfile
bun run dev -- --host 127.0.0.1 --port 8888 --strictPort

ORBIT_ENV=development ORBIT_APP_KEY=b3JiaXQtZGV2ZWxvcG1lbnQta2V5LW5vdC1zZWNyZXQ= cargo run -p orbit-server -- \
  --database data/development.sqlite \
  --attachments data/attachments \
  --backups backups \
  serve --listen 127.0.0.1:8080 --origin http://127.0.0.1:8888
```

The `justfile` sets a public development `ORBIT_APP_KEY` when none is set. To develop on Postgres, put `ORBIT_DATABASE_URL` in `.env.local` (see [`AGENTS.md`](../AGENTS.md) for the required database locale). `just dev` loads that file; `just db-reset` and `just seed` do not, so export the variables first.

## Verification commands

```bash
just test
just check
just e2e
```

`just check` runs Rust formatting, Clippy, all Rust tests, frontend lint and tests, deterministic OpenAPI/client drift checks, the Vite production build, embedded-asset drift detection, and release compilation. `just e2e` uses isolated SQLite and storage paths and exercises the setup-to-task flow in Playwright.

Direct commands remain supported:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ORBIT_TEST_POSTGRES_URL=postgres://orbit:orbit@127.0.0.1:15432/orbit cargo test --workspace  # same suite on Postgres
cd apps/web && bun run lint && bun run test && bun run build
```

## Native production build

Build the frontend first because Cargo embeds `apps/web/dist`:

```bash
just build
(umask 077 && openssl rand -base64 32 > /etc/orbit/app-key)  # once; keep a copy of this file off the server
export ORBIT_ENV=production \
  ORBIT_PUBLIC_ORIGIN=https://orbit.example.com \
  ORBIT_TRUSTED_PROXIES=127.0.0.1/32,::1/128 \
  ORBIT_APP_KEY_FILE=/etc/orbit/app-key
./target/release/orbit config check
./target/release/orbit config show
./target/release/orbit serve
```

Orbit reads its configuration only from `ORBIT_*` environment variables. For the full list and the defaults, see [Configuration](../docs/operations.md#configuration).

On a fresh database, the server prints one `Initial setup URL` to stderr. Open it in the browser, complete the Owner and workspace form, and store no copy of the token. If it expires before use, stop the server and run:

```bash
./target/release/orbit setup-token rotate
```

## OCI image

```bash
docker build --build-arg ORBIT_BUILD_REVISION="$(git rev-parse HEAD)" -t orbit:milestone-1 .
docker run --rm orbit:milestone-1 --help
docker run --name orbit --restart unless-stopped \
  -p 127.0.0.1:8080:8080 \
  -e ORBIT_PUBLIC_ORIGIN=https://orbit.example.com \
  -e ORBIT_TRUSTED_PROXIES=172.17.0.1/32 \
  -e ORBIT_APP_KEY=<base64 key, see docs/operations.md> \
  -v orbit-data:/var/lib/orbit \
  -v "$PWD/backups:/var/backups/orbit" \
  orbit:milestone-1
```

The final Alpine image runs as numeric user and group 65532. It contains the static Orbit binary, Bash for operator diagnostics, CA certificates, timezone data, and empty mount points. The image sets `ORBIT_ENV=production`, `ORBIT_TRUSTED_PROXIES=0.0.0.0/0,::/0`, binds to `0.0.0.0`, and keeps data in `/var/lib/orbit` and backups in `/var/backups/orbit`. Port 2525 is reserved for the deferred inbound SMTP milestone; this release does not listen on it.

## Reverse proxy and Tailscale

Set `ORBIT_PUBLIC_ORIGIN` to the exact external HTTPS origin. Bind to `0.0.0.0` for a container or Tailscale-reachable interface, then restrict access with the host firewall. Add only the actual proxy network to `ORBIT_TRUSTED_PROXIES`.

Prometheus metrics are disabled by default. To expose them on a private interface only, set `ORBIT_METRICS_LISTEN`, for example `127.0.0.1:9090`, and scrape `http://127.0.0.1:9090/metrics`. Never proxy this listener through the public site.

A local Caddy example:

```caddyfile
orbit.example.com {
    reverse_proxy 127.0.0.1:8080
}
```

For Tailscale Serve:

```bash
tailscale serve --bg --https=443 http://127.0.0.1:8080
```

Use the resulting `https://<host>.<tailnet>.ts.net` value as `ORBIT_PUBLIC_ORIGIN`. Keep Orbit bound to `127.0.0.1` when Tailscale Serve runs on the same host. Trust `127.0.0.1/32` and `::1/128`, not the whole tailnet, unless a separate tailnet proxy sends forwarding headers.

## Migration, backup, and restore

Inspect and run migrations while Orbit is stopped:

```bash
orbit migrate status
orbit migrate run
```

Startup runs the same forward-only migration gate automatically. A destructive migration requires a verified pre-migration backup before it runs.

Backup commands:

```bash
orbit backup create
orbit backup list
orbit backup list --pre-migration
orbit backup verify <backup-id>
```

Restore procedure:

1. Stop Orbit. Restore refuses a database owned by a running process.
2. Copy the current data directory somewhere safe.
3. Run `orbit backup verify <backup-id>`.
4. Run `orbit backup restore <backup-id>`. A snapshot restores only into the same kind of database (SQLite or Postgres) it came from.
5. Run `orbit migrate status`.
6. Start Orbit and check `/health/ready` before sending traffic.

Weekly full database integrity checks use a durable schedule (on Postgres this check only tests the connection). Backups are created manually from Settings or the backup CLI while Orbit is stopped. Upload and retention file mutations share the in-process backup pause. Orbit retains up to 24 snapshots from the last day, plus 7 daily and 4 weekly snapshot buckets when enough manual snapshots exist. Operators must copy backups off-host and protect their permissions.

## Invitations, recovery, and SMTP status

Workspace Owners and Admins can generate manual invitation links in `/settings/members`. SMTP invitation delivery remains deferred.

Password recovery requests use the server's bounded administrator-delivery boundary. Until an outbound provider is added, an operator can issue a 30-minute link while Orbit is stopped:

```bash
orbit recovery-link \
  --email user@example.com \
  --origin https://orbit.example.com
```

Share that URL through an authenticated channel. Do not put it in logs or tickets. Orbit has no inbound SMTP listener or mailbox persistence in milestone one.

## Health and shutdown

- `GET /health/live` proves the event loop responds.
- `GET /health/ready` checks migrations, database integrity (SQLite `quick_check`, Postgres connection check), writable storage, scheduler tables, and critical config.
- The optional private metrics listener exposes bounded, identifier-free Prometheus counters at `GET /metrics`.
- Send SIGINT or SIGTERM through the container runtime. Orbit stops accepting HTTP and stops new job claims immediately, then gives the combined HTTP and worker drain 30 seconds to finish.
