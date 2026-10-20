# Deploy Orbit

## Docker with HTTPS

Requirements: Docker Compose, a DNS hostname pointing to this host, and inbound TCP ports 80/443 for Caddy certificate issuance. The example uses a dedicated `172.30.0.0/24` Docker network. Change both its proxy address and the trusted-proxy setting if that subnet conflicts with your host.

```sh
cp deploy/.env.example deploy/.env
# Edit deploy/.env: set ORBIT_DOMAIN (without https://) and ORBIT_APP_KEY (openssl rand -base64 32).
docker compose --env-file deploy/.env -f deploy/compose.yaml up -d --build
```

Orbit reads its configuration only from `ORBIT_*` environment variables. The image sets production mode, the bind address, the data paths and `ORBIT_TRUSTED_PROXIES=0.0.0.0/0,::/0`. A container needs only `ORBIT_PUBLIC_ORIGIN` and `ORBIT_APP_KEY`. The image trusts forwarding headers from every address, so only the reverse proxy may reach port 8080. If anything else can reach it, set `ORBIT_TRUSTED_PROXIES` to the proxy network. To use Postgres, also set `ORBIT_DATABASE_URL`. Keep a copy of `ORBIT_APP_KEY` outside the server: without it, Orbit cannot read the secrets it saved. For all variables, see [Configuration](../docs/operations.md#configuration).

The application port is not published. Only Caddy can reach it through the deployment network. Caddy forwards WebSocket upgrades automatically. The application runs as UID 65532 with a read-only root filesystem and separate named volumes for SQLite/attachments and backups.

On first startup, retrieve the one-time setup URL locally:

```sh
docker compose --env-file deploy/.env -f deploy/compose.yaml logs orbit
```

Treat that URL as a secret. Open it at your HTTPS domain and create the root account (the only account that can open Admin). Orbit then asks for your first workspace and project. Production has no seeded login or automatic setup bypass. Never copy the development database into production. The `seed`, `migrate run --seed`, and reset commands reject production mode. Dev seeding remains idempotent and available through `just dev`.

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

Set the [`ORBIT_*` environment variables](../docs/operations.md#configuration) for the environment, the origin, the trusted proxies and the writable storage paths, then run:

```sh
(umask 077 && openssl rand -base64 32 > /etc/orbit/app-key)  # once; keep a copy of this file off the server
export ORBIT_ENV=production \
  ORBIT_PUBLIC_ORIGIN=https://orbit.example.com \
  ORBIT_TRUSTED_PROXIES=127.0.0.1/32,::1/128 \
  ORBIT_APP_KEY_FILE=/etc/orbit/app-key \
  ORBIT_DATABASE_PATH=/var/lib/orbit/orbit.sqlite \
  ORBIT_ATTACHMENTS_PATH=/var/lib/orbit/attachments \
  ORBIT_BACKUPS_PATH=/var/backups/orbit
./orbit config check
./orbit serve
```

Run under an unprivileged service account and an HTTPS reverse proxy. The binary architecture matches the Docker build platform. The latest commit on `main` publishes one image manifest for `linux/amd64` and `linux/arm64` to `ghcr.io/<owner>/<repository>:<seven-character-commit-sha>`. A newer commit cancels an older image build. Each image is built and smoke-tested on a native runner; the image workflow does not use QEMU. The workflow does not create GitHub Releases or downloadable binaries. Local builds do not publish anything.

## Backups and upgrades

Backups are created from **Admin → Backups** while Orbit runs, or automatically (hourly or daily) when the root user turns that on in **Admin → Storage**; with backups in S3 they are uploaded to the bucket. Snapshot retention keeps up to 24 snapshots from the last day, plus seven daily and four weekly buckets when enough backups exist. A backup volume on the same machine is not disaster recovery: copy it off-host, protect access, and regularly test restore into separate storage.

On SQLite the running server owns the database file, so the CLI `backup create` works only while the app is stopped. On Postgres (`ORBIT_DATABASE_URL`) it also works while servers run. `backup list` works at any time.

```sh
docker compose --env-file deploy/.env -f deploy/compose.yaml stop orbit
docker compose --env-file deploy/.env -f deploy/compose.yaml run --rm orbit backup create
docker compose --env-file deploy/.env -f deploy/compose.yaml start orbit
docker compose --env-file deploy/.env -f deploy/compose.yaml exec orbit /orbit backup list
```

Before upgrading, create and verify a backup. Keep the old image and matching backup; older binaries reject newer database schemas. On SQLite, never run two app processes against one database. On Postgres several servers can share one database; see [Orbit Cloud](../docs/operations.md#orbit-cloud-one-postgres-several-orbit-servers). Stop every app process before restoring. See [operations](../docs/operations.md) and [restore commands](../.ai/DEVELOPMENT.md#migration-backup-and-restore).

## Live task collaboration

Each tab opens a session-authenticated WebSocket for its active workspace. Every successful workspace audit mutation records a notification in the same database transaction. The server coalesces ordered invalidations to a high-water sequence and checks for new events every second. No resource content is sent over the socket; clients refresh through authorized HTTP endpoints.

Reconnects resume from the last successfully refreshed sequence. Missing, expired, or invalid cursors trigger a full workspace refresh. Event retention is seven days; persistent counters never reset when old events are pruned. Revoked/expired sessions, suspended users, and removed memberships are checked before delivery and at least once per second. Chat, presence and typing use the separate live socket, and page co-editing its own socket per page.

Focused text drafts and in-flight writes defer remote refresh until editing finishes. Version conflicts use the existing conflict flow rather than silently overwriting another user's write. A connection banner appears while live updates are unavailable; reconnection uses capped backoff. Without SMTP (Admin → Settings → Email), invitation and recovery links are shared manually.

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
