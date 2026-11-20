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

## UI: help text goes in a tooltip

Do not put help paragraphs under a title, a field label or a switch label. For a card or section title, give the
text to `SettingsCard`'s `description`; for another heading, use `TitleTip`
(`apps/web/src/components/common/TitleTip.tsx`). Both give the title a solid underline (as in Coolify) and show the
text on hover. For a field or switch label, put the text in an info icon with a tooltip next to the label: `InfoTip`
(`apps/web/src/components/common/InfoTip.tsx`), for example `<FieldLabel>Request URL <InfoTip text="…" /></FieldLabel>`.
Short errors (`FieldError`), empty states, steps, links and "why is this disabled" notes stay visible. Use this in new
views, and change an old view to it when you edit that view. Mobile has no hover: there, a tap shows the text. Use
`Input`'s `help` prop, `LabelTip` (`apps/mobile/src/ui/LabelTip.tsx`), `OptionText`'s `help` prop, or the mobile
`SettingsCard` `description`.

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
- **Transactions:** on Postgres, an error ends the whole transaction, so do not catch a database error and continue
  in the same transaction.
- **Write locks:** on Postgres, write transactions take advisory locks that all servers share. On SQLite, every scope is
  `BEGIN IMMEDIATE`. Choose the scope for each new write transaction
  (classification: [`docs/specs/2026-11-19-write-lock-scopes.md`](docs/specs/2026-11-19-write-lock-scopes.md)):
  - `workspace_transaction(workspace_id)`: the transaction reads and writes rows of one workspace only, and you know
    the workspace id before it starts. It can also write a shared row, but only when a unique constraint,
    `ON CONFLICT` or one atomic statement protects that row. Writes to other workspaces continue in parallel.
  - `immediate_transaction()` (global, blocks every writer on every server): the transaction reads and then writes
    rows that more than one workspace shares (users, sessions, instance settings, billing), or it touches more than
    one workspace. If you are not sure, use this scope.
  - `jobs_transaction()`: only for the job store.
- **Keep write transactions short.** Every writer of the scope waits for a write transaction, and after 30 s
  (`lock_timeout`) the waiting writers fail.
  - Do not make a network call inside a write transaction: no S3 or blob store call, HTTP, mail or DNS. Do the call
    before the transaction (for files, use `prepare` / `prepare_bytes`, then `finalize_*_in_transaction`) or after
    the commit (for example, a job or an outbox row).
  - Do not wait (`sleep`, a channel, an in-process lock) and do not do heavy CPU work (document merges, imports,
    search reindex) inside a write transaction. Do that work before the transaction, then check a version inside it.
  - Put a limit on loops: use batches of about 200 items per transaction, and multi-row `INSERT` statements built
    with `SqlBuilder`. A batch job must be safe to run again after a stop.
  - A background loop treats `is_transient_lock_error` (lock timeout, pool timeout) and S3 errors as "retry on the
    next round". Such an error must never stop the server.
  - The monitor logs `slow write transaction holds a write lock` for each transaction that is older than 2 s. Its
    `application_name` field gives the file and line. Fix a new warning before you ship.
- **Migrations:** make a new one with `just new-migration <name>` (add `destructive` if it needs a backup first). It
  makes a SQLite and a Postgres file with the same UTC timestamp version, so parallel worktrees do not collide. Write
  the same change in both files. `crates/platform/build.rs` finds the files and refuses a new 4-digit number, a
  duplicate version or a missing Postgres file. Do not add migrations by hand to a list, and never rename or edit
  0001–0099.
- **Tests:** run the tests that cover your change on both databases:
  `cargo test -p orbit-server --test <name>`, then the same with
  `ORBIT_TEST_POSTGRES_URL=postgres://orbit:orbit@127.0.0.1:15432/orbit`. Tests that only make sense on SQLite
  start with `if orbit_platform::testing_postgres() { return; }` and a one-line reason.
- **Postgres setup:** the database must use `LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8' LOCALE 'C.UTF-8'`
  (Orbit refuses other databases), and the server needs `ORBIT_APP_KEY`. S3 for files and backups stays optional
  and is set in Admin → Storage or with `ORBIT_S3_*`. Operator details: [`docs/operations.md`](docs/operations.md).

## Development servers: one per worktree

Jean starts the dev server with `./scripts/dev run` (from `jean.json`). It wraps `just dev`, so several worktrees
can run at the same time:

- **Ports:** the main checkout uses web 18888 and API 18080. Each worktree gets a fixed slot for its branch: web
  `24000 + slot*10`, API one higher. Slots are kept in `.dev-instances/slots` next to the shared git directory.
- **Database:** a worktree uses its own SQLite file in `data/` and does not read the main checkout's `.env.local`.
  Thus a branch's migrations never change the main dev database. A worktree can have its own `.env.local`.
- **URL:** when Tailscale runs, the web port is published over HTTPS on the tailnet name
  (`https://<host>.ts.net:<web port>`), and the server uses it as its public origin.
- **Network:** `ORBIT_DEV_HOST` is the address the web server listens on (default `127.0.0.1`; `0.0.0.0`
  opens it to the local network). The API stays on `127.0.0.1` (Orbit refuses another address in
  development); the web server proxies `/api` to it. `ORBIT_DEV_PUBLIC_HOST` sets the host of the public origin
  (`http://<host>:<web port>`) in place of the tailnet name. Set them in your shell, not in a tracked file.

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

Without Mailpit, sending fails. A mail server set in **Admin → System email** stays: the seed does not change it. If the
container runs but `docker port orbit-mailpit` prints nothing, it lost its network (seen on 2026-11-04): run
`docker network connect bridge orbit-mailpit && docker restart orbit-mailpit`.

## Release video

To make the product update video or release-note screenshots of a release, follow
[`marketing/release-video/GUIDE.md`](marketing/release-video/GUIDE.md).
