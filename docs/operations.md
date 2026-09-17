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

Restore refuses an active database ownership lock. Stop every Orbit process that uses that file. If restore reports rollback failure, do not start Orbit. Preserve the target and rollback paths named by the error and recover from a separate verified snapshot.

## Lost or expired setup token

Stop Orbit and run `orbit --config config/orbit.toml setup-token rotate`. Share the new setup URL directly with the installation owner. Rotation invalidates the unused old token. Once setup completes, setup cannot run again.

## Password recovery without SMTP

Stop Orbit and use `orbit --config config/orbit.toml recovery-link --email <email> --origin <https-origin>`. Share the 30-minute link through an authenticated channel. Never log it. Unknown or suspended accounts do not receive a link.

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
