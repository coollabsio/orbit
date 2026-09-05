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

`just dev` starts Orbit at `http://127.0.0.1:8080` and Vite at `http://127.0.0.1:8888`. Vite proxies `/api` to the Rust server. The recipe records and stops only its own child processes.

`just db-reset` is an explicit development-only destructive reset. `just seed` is idempotent and prints the local seed account credentials after creating a representative workspace, project, workflow, and task.

The underlying commands are:

```bash
cd apps/web
bun install --frozen-lockfile
bun run dev -- --host 127.0.0.1 --port 8888 --strictPort

ORBIT_ENV=development ORBIT__ENVIRONMENT=development cargo run -p orbit-server -- \
  --database data/development.sqlite \
  --attachments data/attachments \
  --backups backups \
  serve --listen 127.0.0.1:8080 --origin http://127.0.0.1:8888
```

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
cd apps/web && bun run lint && bun run test && bun run build
```

## Native production build

Build the frontend first because Cargo embeds `apps/web/dist`:

```bash
just build
cp config/orbit.example.toml config/orbit.toml
./target/release/orbit --config config/orbit.toml config check
./target/release/orbit --config config/orbit.toml config show
./target/release/orbit --config config/orbit.toml serve
```

On a fresh database, the server prints one `Initial setup URL` to stderr. Open it in the browser, complete the Owner and workspace form, and store no copy of the token. If it expires before use, stop the server and run:

```bash
./target/release/orbit --config config/orbit.toml setup-token rotate
```

## OCI image

```bash
docker build --build-arg ORBIT_BUILD_REVISION="$(git rev-parse HEAD)" -t orbit:milestone-1 .
docker run --rm orbit:milestone-1 --help
docker run --name orbit --restart unless-stopped \
  -p 127.0.0.1:8080:8080 \
  -v "$PWD/config/orbit.toml:/etc/orbit/orbit.toml:ro" \
  -v orbit-data:/var/lib/orbit \
  -v "$PWD/backups:/var/backups/orbit" \
  orbit:milestone-1
```

The final scratch image runs as numeric user and group 65532. It contains the static Orbit binary, CA certificates, timezone data, and empty mount points. Port 2525 is reserved for the deferred inbound SMTP milestone; this release does not listen on it.

## Reverse proxy and Tailscale

Set `http.public_origin` to the exact external HTTPS origin. Bind to `0.0.0.0` for a container or Tailscale-reachable interface, then restrict access with the host firewall. Add only the actual proxy network to `http.trusted_proxies`.

Prometheus metrics are disabled by default. To expose them on a private interface only, set `metrics.listen`, for example `127.0.0.1:9090`, and scrape `http://127.0.0.1:9090/metrics`. Never proxy this listener through the public site.

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

Use the resulting `https://<host>.<tailnet>.ts.net` value as `http.public_origin`. Keep Orbit bound to `127.0.0.1` when Tailscale Serve runs on the same host. Trust `127.0.0.1/32` and `::1/128`, not the whole tailnet, unless a separate tailnet proxy sends forwarding headers.

## Migration, backup, and restore

Inspect and run migrations while Orbit is stopped:

```bash
orbit --config config/orbit.toml migrate status
orbit --config config/orbit.toml migrate run
```

Startup runs the same forward-only migration gate automatically. A destructive migration requires a verified pre-migration backup before it runs.

Backup commands:

```bash
orbit --config config/orbit.toml backup create
orbit --config config/orbit.toml backup list
orbit --config config/orbit.toml backup list --pre-migration
orbit --config config/orbit.toml backup verify <backup-id>
```

Restore procedure:

1. Stop Orbit. Restore refuses a database owned by a running process.
2. Copy the current data directory somewhere safe.
3. Run `orbit --config config/orbit.toml backup verify <backup-id>`.
4. Run `orbit --config config/orbit.toml backup restore <backup-id>`.
5. Run `orbit --config config/orbit.toml migrate status`.
6. Start Orbit and check `/health/ready` before sending traffic.

Daily backups and weekly full database integrity checks use durable schedules. An overdue run executes after restart instead of resetting its clock. Upload and retention file mutations share the backup pause, and scheduled backup file work is cancellable during shutdown. Orbit retains 7 daily and 4 weekly verified snapshots. Operators must copy backups off-host and protect their permissions.

## Invitations, recovery, and SMTP status

Workspace Owners and Admins can generate manual invitation links in `/settings/members`. SMTP invitation delivery remains deferred.

Password recovery requests use the server's bounded administrator-delivery boundary. Until an outbound provider is added, an operator can issue a 30-minute link while Orbit is stopped:

```bash
orbit --config config/orbit.toml recovery-link \
  --email user@example.com \
  --origin https://orbit.example.com
```

Share that URL through an authenticated channel. Do not put it in logs or tickets. Orbit has no inbound SMTP listener or mailbox persistence in milestone one.

## Health and shutdown

- `GET /health/live` proves the event loop responds.
- `GET /health/ready` checks migrations, SQLite integrity, writable storage, scheduler tables, and critical config.
- The optional private metrics listener exposes bounded, identifier-free Prometheus counters at `GET /metrics`.
- Send SIGINT or SIGTERM through the container runtime. Orbit stops accepting HTTP and stops new job claims immediately, then gives the combined HTTP and worker drain 30 seconds to finish.
