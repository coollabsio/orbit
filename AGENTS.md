# Orbit agent instructions

Read [`.ai/README.md`](.ai/README.md) first: it lists the project guides in order.

## Apps

The repo root is one Bun workspace: `apps/web` (Vite) and `apps/mobile` (Expo, see [`apps/mobile/AGENTS.md`](apps/mobile/AGENTS.md)).
Run `bun install` at the root. Both apps share one hoisted `node_modules`, so they must use the same `react` and
`react-dom` version. Find a package's files with module resolution, not a fixed `node_modules/…` path.

**Always port web changes to mobile.** When you change a feature, a behavior or a style in `apps/web`, make the
same change in `apps/mobile` in the same task, without waiting for the user to ask. A web task is not done until the
mobile port is done and checked (`bunx tsc --noEmit -p .` and `bun test src` in `apps/mobile`, and the iOS bundle
builds on the running Metro server). Skip a change only when mobile does not have the problem it fixes (for example,
a keyboard-only or desktop-only feature, or a fix for a component that mobile does not use). Then tell the user
which changes you did not port, and why. When mobile lacks a screen that the change needs, say so and offer to port
that screen.

## Databases: SQLite and Postgres

Orbit runs on SQLite (default, `ORBIT_DATABASE_PATH`) or Postgres (`ORBIT_DATABASE_URL`).
Every change must work on both.

- **SQL:** all queries use the sqlx `Any` driver. Write placeholders as `$1, $2, …` (never `?`). Build dynamic SQL
  with `orbit_platform::SqlBuilder`, not `sqlx::QueryBuilder`.
- **Rows and types:** import `orbit_platform::Row`, not `sqlx::Row`. Flags are 0/1 `BIGINT` columns: bind `i64`
  (never `bool`, `u32` or `u64`), and decode boolean results such as `EXISTS (…)` as `orbit_platform::Flag`.
- **Portable SQL:** use `ON CONFLICT` (not `INSERT OR IGNORE`), `IS [NOT] DISTINCT FROM` (not `IS $n`),
  `CAST(SUM(…) AS BIGINT)`, and `lower(…)` for case-insensitive text. When SQL must differ (search, JSON, time),
  branch on `database.backend()` (`Backend::Sqlite` / `Backend::Postgres`).
- **Transactions:** writes use `immediate_transaction()`. It serializes writers on both databases. On Postgres, an
  error ends the whole transaction, so do not catch a database error and continue in the same transaction.
- **Migrations:** a new SQLite migration in `apps/server/migrations/` also needs the same change in the Postgres
  schema in `apps/server/migrations/postgres/` (see `.ai/lessons.md` for the other places to update).
- **Tests:** run the tests that cover your change on both databases:
  `cargo test -p orbit-server --test <name>`, then the same with
  `ORBIT_TEST_POSTGRES_URL=postgres://orbit:orbit@127.0.0.1:15432/orbit`. Tests that only make sense on SQLite
  start with `if orbit_platform::testing_postgres() { return; }` and a one-line reason.
- **Postgres setup:** the database must use `LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8' LOCALE 'C.UTF-8'`
  (Orbit refuses other databases), and the server needs `ORBIT_APP_KEY`. S3 for files and backups stays optional
  and is set in Admin → Storage. Operator details: [`docs/operations.md`](docs/operations.md).

## Development servers: one per worktree

Jean starts the dev server with `./scripts/dev run` (from `jean.json`). It wraps `just dev`, so several worktrees
can run at the same time:

- **Ports:** the main checkout uses web 18888 and API 18080. Each worktree gets a fixed slot for its branch: web
  `24000 + slot*10`, API one higher. Slots are kept in `.dev-instances/slots` next to the shared git directory.
- **Database:** a worktree uses its own SQLite file in `data/` and does not read the main checkout's `.env.local`.
  Thus a branch's migrations never change the main dev database. A worktree can have its own `.env.local`.
- **URL:** when Tailscale runs, the web port is published over HTTPS on the tailnet name
  (`https://<host>.ts.net:<web port>`), and the server uses it as its public origin.

To test a running app, call Jean MCP `get_run_environments` with your `worktreeId`, and use the ports and URL it
gives for your worktree. If your worktree has no running environment, start it with `start_run_environment`. Do not
use another worktree's server: it runs other code and another database. `./scripts/dev urls` lists all instances.
Before you trust a page, check who owns its port (`ss -ltnp`, then `readlink /proc/<pid>/cwd`).
`./scripts/dev teardown` frees a worktree's slot (Jean runs it when it removes the worktree). `just e2e` still uses
the fixed port 18080, so it cannot run while the main checkout's server runs.

## Development mail: Mailpit

In development, Orbit sends mail to Mailpit (SMTP `127.0.0.1:11025`, inbox at `http://127.0.0.1:18026`). The dev
seed (`just dev`, `just seed`) sets this mail server when none is set, and opens registration. If you test sign-up,
invitations, recovery or email changes, make sure Mailpit runs (`docker ps --filter name=orbit-mailpit`), and start
it if needed:

```bash
docker start orbit-mailpit 2>/dev/null || docker run -d --name orbit-mailpit --restart unless-stopped \
  -p 127.0.0.1:11025:1025 -p 127.0.0.1:18026:8025 axllent/mailpit
```

Without Mailpit, sending fails. A mail server set in **Admin → Email** stays: the seed does not change it. If the
container runs but `docker port orbit-mailpit` prints nothing, it lost its network (seen on 2026-11-04): run
`docker network connect bridge orbit-mailpit && docker restart orbit-mailpit`.

## Release video

To make the product update video or release-note screenshots of a release, follow
[`marketing/release-video/GUIDE.md`](marketing/release-video/GUIDE.md).
