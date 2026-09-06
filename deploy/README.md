# Deploy Orbit

## Docker with HTTPS

Requirements: Docker Compose, a DNS hostname pointing to this host, and inbound TCP ports 80/443 for Caddy certificate issuance. The example uses a dedicated `172.30.0.0/24` Docker network. Change both its proxy address and the trusted-proxy setting if that subnet conflicts with your host.

```sh
cp deploy/.env.example deploy/.env
# Edit ORBIT_DOMAIN in deploy/.env. Do not include https://.
docker compose --env-file deploy/.env -f deploy/compose.yaml up -d --build
```

The application port is not published. Only Caddy can reach it through the deployment network. Caddy forwards WebSocket upgrades automatically. The application runs as UID 65532 with a read-only root filesystem and separate named volumes for SQLite/attachments and backups.

On first startup, retrieve the one-time setup URL locally:

```sh
docker compose --env-file deploy/.env -f deploy/compose.yaml logs orbit
```

Treat that URL as a secret. Open it at your HTTPS domain and create the owner account. Production has no seeded login or automatic setup bypass. Never copy the development database into production. The `seed`, `migrate run --seed`, and reset commands reject production mode. Dev seeding remains idempotent and available through `just dev`.

For Tailscale-only hosting, use an existing Tailscale HTTPS proxy instead of the Caddy service. Configure the actual HTTPS public origin and trust only the exact proxy address. Do not set production to development mode to work around HTTPS/origin checks.

## Single executable

The image contains a statically linked Linux executable with all frontend assets embedded. It needs no Node, Bun, Rust installation, or separate web directory at runtime.

```sh
docker build -t orbit:local .
container=$(docker create orbit:local)
docker cp "$container:/orbit" ./orbit
docker rm -v "$container"
chmod +x orbit
./orbit --help
```

Copy `config/orbit.example.toml` to your deployment configuration, set your origin/proxy and writable storage paths, then run:

```sh
./orbit --config /etc/orbit/orbit.toml config check
./orbit --config /etc/orbit/orbit.toml serve
```

Run under an unprivileged service account and an HTTPS reverse proxy. The binary architecture matches the Docker build platform. Tagged GitHub releases build an x86_64 Linux musl binary with a checksum and publish the tested image to `ghcr.io/<owner>/<repository>:<tag>`. Registry publishing happens only when the workflow runs; local builds do not publish anything.

## Backups and upgrades

Automatic daily backups retain seven daily and four weekly snapshots. A backup volume on the same machine is not disaster recovery: copy it off-host, protect access, and regularly test restore into separate storage.

```sh
docker compose --env-file deploy/.env -f deploy/compose.yaml exec orbit /orbit --config /etc/orbit/orbit.toml backup create
docker compose --env-file deploy/.env -f deploy/compose.yaml exec orbit /orbit --config /etc/orbit/orbit.toml backup list
```

Before upgrading, create and verify a backup. Keep the old image and matching backup; older binaries reject newer database schemas. Never run two app processes against one database. Stop the app before restoring. See [operations](../docs/operations.md) and [restore commands](../.ai/DEVELOPMENT.md#migration-backup-and-restore).

## Live task collaboration

Each tab opens a session-authenticated WebSocket for its active workspace. Every successful workspace audit mutation records a notification in the same SQLite transaction. The server coalesces ordered invalidations to a high-water sequence and checks for new events every second. No resource content is sent over the socket; clients refresh through authorized HTTP endpoints.

Reconnects resume from the last successfully refreshed sequence. Missing, expired, or invalid cursors trigger a full workspace refresh. Event retention is seven days; persistent counters never reset when old events are pruned. Revoked/expired sessions, suspended users, and removed memberships are checked before delivery and at least once per second. This is not collaborative text merging or presence/typing.

Focused text drafts and in-flight writes defer remote refresh until editing finishes. Version conflicts use the existing conflict flow rather than silently overwriting another user's write. A connection banner appears while live updates are unavailable; reconnection uses capped backoff. Invitation/recovery links are shared manually; SMTP is not required.

## Acceptance checks

- Open `https://<your-domain>/health/ready` through Caddy; expect readiness success.
- Verify the fresh instance requires setup and rejects `test@example.com / password`.
- Use two browser profiles: create/edit a task, move it on the board, add a comment, and delete/restore it. The other profile should update without reloading, normally within about one second plus request latency.
- Disconnect/reconnect a browser, then confirm it catches up. Remove its membership or revoke its session and confirm access is lost.
- Restart the container and verify tasks and attachments persist.
- Restore a verified backup into separate stopped-instance storage and verify its tasks and attachments before depending on the deployment.

A repeatable two-browser smoke test against an **existing development server** is available without starting another server:

```sh
cd apps/web
ORBIT_SMOKE_URL=http://127.0.0.1:18888 node scripts/smoke-collaboration.cjs
```

It signs in with the seeded dev account, creates a uniquely named task, checks another browser receives it and catches up after a disconnect, then moves the smoke task to trash. Set `PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH` if using a separately installed Chromium.

Test a built production image in isolated temporary storage:

```sh
bash deploy/smoke.sh orbit:local
```

This uses a dynamically assigned localhost port and a simulated trusted HTTPS proxy header. It checks non-root execution, HTTPS enforcement, embedded assets, first-run setup, rejected development credentials/seed commands, and restart readiness. It does not replace testing real TLS and backups on the deployment host.
