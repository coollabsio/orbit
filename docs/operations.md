# Orbit milestone-one operations runbook

This runbook covers the failure paths that need an operator decision. Exact build, first-run, proxy, backup, and restore commands live in [`.ai/DEVELOPMENT.md`](../.ai/DEVELOPMENT.md).

## Startup fails before HTTP binds

Orbit deliberately binds last. Read the first error and keep the process stopped.

1. Run `orbit --config config/orbit.toml config check`.
2. Run `orbit --config config/orbit.toml config show` and verify paths, origin, bind address, and trusted proxies. The database file must remain outside attachment and backup storage. Secrets remain redacted.
3. Run `orbit --config config/orbit.toml migrate status`.
4. Verify the newest backup with `orbit --config config/orbit.toml backup verify <id>`.
5. Fix ownership or capacity on the database, attachment, and backup paths.
6. Start Orbit again. Never delete SQLite WAL files while the process runs.

## Integrity or readiness fails

`/health/ready` returns 503 when a required check fails. Orbit also runs a durable weekly full SQLite integrity and foreign-key check. A failed or interrupted full check stops normal HTTP traffic and exits the process; an unfinished integrity job forces another full check before a restart can bind. Remove the instance from proxy traffic and preserve the current data directory.

Do not attempt automatic repair. Verify a known-good backup, restore it with the documented stopped-server procedure, and retain the damaged copy for investigation. `/health/live` may remain 200 because it reports process liveness, not data safety.

## A background service fails

Retention, reconciliation, scheduler, or integrity supervision failure cancels normal serving. Weekly integrity checks and retention use durable schedules, so an overdue run is materialized immediately after restart. Backups are manual. The process stops accepting HTTP and tells workers to stop claiming jobs immediately, then bounds the combined drain to 30 seconds. Fix the reported database or storage condition, verify backups, and restart. Durable job leases recover after expiry; do not edit the jobs table.

## Restore rollback or ownership error

Restore refuses an active database ownership lock (the `<database>.lock` file next to the database; do not delete it while Orbit runs). Stop every Orbit process that uses that file. If restore reports rollback failure, do not start Orbit. Preserve the target and rollback paths named by the error and recover from a separate verified snapshot.

On Postgres (`data.database_url`), the ownership lock is a Postgres advisory lock held by the serving process, so stop every Orbit process that uses the database before `backup restore`. A Postgres snapshot holds one `COPY` file per table (`database/<table>.copy`) instead of `database.sqlite`. Restore replaces every table in one transaction: if it fails, the database is unchanged. A snapshot restores only into the same kind of database it came from.

## Lost or expired setup token

Stop Orbit and run `orbit --config config/orbit.toml setup-token rotate`. Share the new setup URL directly with the installation owner; the account created there becomes the root user. Rotation invalidates the unused old token. Once setup completes, setup cannot run again.

## Root user and Admin area

The account created at setup is the root user. **Admin** in the sidebar (`/admin`) has accounts (suspend, reinstate, password recovery links), the global audit log with CSV export, backups, and instance settings (email, registration).

The root user can make other accounts **instance admins** (Admin → Users → Manage → Make admin). Admins see the same Admin area, but they cannot suspend or reset the root account or another admin, and they cannot choose admins. Nobody can suspend, reset or demote the root account. Removing an admin takes their Admin access away at once.

## Choosing the root user from the server

When nobody can fix the root user in the web app (the root account is suspended, the wrong account became root after an upgrade, or the root person left), use the CLI on the server. Like `recovery-link`, it needs Orbit stopped, because a running Orbit owns the database:

```sh
orbit --config config/orbit.toml root show
orbit --config config/orbit.toml root set --email new-root@example.com
```

With the Compose deployment:

```sh
docker compose --env-file deploy/.env -f deploy/compose.yaml stop orbit
docker compose --env-file deploy/.env -f deploy/compose.yaml run --rm orbit root set --email new-root@example.com
docker compose --env-file deploy/.env -f deploy/compose.yaml start orbit
```

`root set` makes the account root, gives it Admin access and reinstates it if it was suspended. The previous root user stays an instance admin; remove that in Admin → Users if needed. The change is in the audit log as `instance.root_changed`. On upgrade, Orbit makes the oldest account with Admin access the root user, which is the account created at setup.

## Email, registration and the app key

Orbit sends email only for invitations, open-registration sign-up links and password reset links. The root user saves the SMTP server in **Admin → Settings → Email** and checks it with **Send test email**. Without a mail server, invitations are shared as links and password recovery works as described below.

Registration is closed after setup: people join only by invitation. The root user can turn on **Open registration** in **Admin → Settings** once a mail server is saved. A new person then enters their email, opens the emailed link (60 minutes, one use) and chooses a name and password. Removing the mail server closes registration.

Saved secrets (the SMTP password, GitHub App keys, Notion import tokens) are encrypted with the app key. Set `secrets.app_key` (base64 of 32 random bytes, for example `openssl rand -base64 32`) to manage it yourself; otherwise Orbit creates `app.key` beside the database on first start (mode 600). Keep that file with your backups: a restored database without it cannot read its saved secrets, and the root user must enter them again.

## Postgres

With `data.database_url`, the database is on another host. Orbit then needs `secrets.app_key` (`ORBIT__SECRETS__APP_KEY`, base64 of 32 random bytes) instead of an `app.key` file: it encrypts saved secrets, the S3 secret among them, so a new server needs the same key. Keep it with the database credentials.

Files and backups still go to `data.attachments` and `data.backups` on this server until the root user saves a bucket in **Admin → Storage** and turns on attachments and backups in S3. Do that to make the server replaceable; pre-migration backups always stay local.

One server process at a time uses a database: a second one waits for the first one's advisory lock, then refuses to start. Live editing, presence and chat events are kept in the server's memory, so Orbit does not run as several replicas behind a load balancer.

## Object storage (S3), backups and a lost server

The root user saves an S3-compatible bucket (AWS S3, Cloudflare R2, MinIO, …) in **Admin → Storage**. Orbit writes, reads and deletes a test object before it saves the bucket. The secret key is encrypted with the app key. Then three settings apply:

- **Attachments in S3**: new files go to the bucket, and existing files move there in the background (the page shows how many are left). Turning it off moves them back to this server. Uploads are still staged on the local disk first. The bucket can be removed only when no attachment file is left in it.
- **Backups in S3**: each new backup is uploaded to `backups/snapshots/{id}/` in the bucket (manifest last) and removed from this server. The Backups page lists local and S3 backups; download and restore fetch an S3 backup into `<backups>/downloads/` and verify its checksums first. Pre-migration backups stay local.
- **Automatic backups**: off, every hour or every day. Retention keeps the newest backup of each of the last 7 days and 4 weeks, local and in S3.

With attachments in S3, a backup holds the database (and the files still on this server), not the bucket's files. So Orbit does not delete an unused file from the bucket at once: it keeps it 40 days, longer than any kept backup, so a restored older database still finds its files.

When the server is lost, restore on a new server from the bucket. The command line reads the bucket from environment variables, not from the (lost) database:

```sh
export ORBIT_BACKUP_S3_ENDPOINT=https://s3.eu-central-1.amazonaws.com ORBIT_BACKUP_S3_BUCKET=orbit-files \
  ORBIT_BACKUP_S3_ACCESS_KEY_ID=… ORBIT_BACKUP_S3_SECRET_ACCESS_KEY=…   # optional: _REGION, _PREFIX, _PATH_STYLE=true
orbit --config config/orbit.toml backup list            # the last column says local or s3
orbit --config config/orbit.toml backup restore <id>
```

Put the old `app.key` (or `secrets.app_key`) on the new server before you start Orbit: without it, Orbit cannot read the saved S3 secret and does not start while a bucket is saved. Data written after the last backup is lost, so choose the schedule by how much data you can lose.

## Large images

The web client makes a JPEG, PNG or WebP of 512 KiB or more smaller before upload: at most 2560 px on the long edge, re-encoded as WebP. GIF, SVG and animated images stay as they are.

To do the same to images that were uploaded before, the root user clicks **Compress images** in **Admin → Storage**. It runs in the background while Orbit serves, for files on this server and in S3, and the page shows its progress. Each file keeps its task, page and chat references; the file names change to `.webp`. The originals are replaced: make a backup first if you want to keep them. The old file is released like any unused file, so in S3 it stays 40 days for older backups. Running it again does not change the result.

While Orbit is stopped, the same works from the command line for files on this server (files in S3 are skipped):

```sh
orbit --config config/orbit.toml attachments compress --dry-run   # reports the savings, changes nothing
orbit --config config/orbit.toml attachments compress
```

## Password recovery without SMTP

The root user opens **Admin → Users**, selects **Manage → Create password recovery link** for the account and shares the 30-minute link through an authenticated channel. The root user changes their own password in Profile instead.

If the root user lost their own password, stop Orbit and use `orbit --config config/orbit.toml recovery-link --email <email> --origin <https-origin>`. Never log a link. Unknown or suspended accounts do not receive a link.

## Security checks after proxy changes

- An HTTPS request through a trusted proxy must receive HSTS.
- A login response must use `__Host-orbit_session` with `Secure`, `HttpOnly`, `SameSite=Lax`, and no `Domain`.
- Requests from untrusted peers cannot supply trusted request IDs or forwarded transport.
- Unsafe API calls with a missing or different `Origin` must return `origin_forbidden`.
- Unknown `/api/*` paths must return `application/problem+json`, never SPA HTML.
- `/api` itself and wrong methods on known API routes must return Problem Details.
- Production mode requires an HTTPS public origin and at least one explicitly trusted proxy. Requests without a verified HTTPS transport are rejected. Development mode is loopback-only.
- Tune `[rate_limits]` only after observing the endpoint-class defaults. Values must be between 1 and 1,000,000 requests per minute. Limit responses are correlated `429` Problem Details; IPv6 clients are grouped by `/64` and the in-memory limiter has a hard entry cap.
- If metrics are enabled, the configured private `/metrics` listener must be reachable only by the monitoring network and must not appear on the public listener.

## Monitoring and operational ownership

Existing endpoints:

- `GET /health/live` — process liveness
- `GET /health/ready` — serving readiness; treat 503 as take-out-of-proxy
- Optional private `GET /metrics` when `[metrics].listen` is set

Alerting should live in the operator's existing monitor (systemd, Caddy, Prometheus, host disk checks). Do not scrape `/metrics` through the public origin.

Suggested thresholds:

- Readiness 503 for more than 2 minutes → page the primary
- Backup directory missing a verified snapshot newer than 36 hours → page the primary
- Filesystem for database/attachments/backups above 85% → ticket the fallback owner

Ownership template (fill with real names before rollout):

| Area | Primary | Fallback |
|---|---|---|
| Account recovery / invitations | | |
| Backups and restore rehearsal | | |
| Upgrades and rollback | | |
| Incident response | | |

Automated subset: `ORBIT_VERIFY_ORIGIN=https://… deploy/verify-production.sh`. Restore rehearsal: `deploy/backup-rehearsal.sh config/orbit.toml <backup-id> /path/to/scratch`. Both require an operator-chosen host and off-host backup destination; they do not complete Tasks 10, 11, or 14 by themselves.
