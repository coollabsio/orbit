# Project overview

## Product and milestone status

Orbit is a self-hosted workspace application with a Rust server and a React web client. It keeps tasks, docs, and chat for a team in one place, in SQLite (default) or Postgres. The product goal is an AI assistant with the full context of the workspace. Because Orbit is self-hosted, that context stays on the team's own server.

The production binary embeds the matching Vite build. It serves the SPA and `/api/v1` from one HTTP listener. Operators terminate HTTPS at a reverse proxy.

## Persistent and mock-backed routes

These routes use the production API and the database:

- `/setup`, `/login`, `/register`, `/recovery`, `/onboarding`, and `/accept-invitation`
- `/` (home summaries), `/tasks`, `/views`, task trash, and project settings
- `/docs` (collaborative pages, trash, Notion import)
- `/chat` (channels, DMs, threads); `/dm/*` redirects to `/chat`
- `/activity` (`/inbox` redirects to it), `/profile`, `/settings`, and `/admin`

Mail has no backend yet. Its UI in `features/mail` uses mock data, and `/mail` redirects to `/tasks`. `mail.rs` on the server sends only account email (invitations, registration, recovery).

AI: there is no built-in assistant yet. AI clients use tasks and read docs and chat through the scoped MCP endpoint (`/mcp`, see `docs/mcp.md`).

## Runtime shape

```text
browser
  -> HTTPS reverse proxy
    -> orbit HTTP listener
      -> embedded SPA
      -> /api/v1 routes
      -> /health/live and /health/ready
      -> SQLite in WAL mode, or Postgres
      -> local attachment storage
      -> local verified backups
```

One Orbit process owns a SQLite database (file lock); on Postgres, servers share an advisory lock that restore and reset take exclusively. The process validates configuration, runs guarded migrations and a quick integrity check, verifies writable storage and the embedded client contract, then binds HTTP. Retention and attachment reconciliation services start with the server and stop after HTTP begins graceful shutdown. Backups are started manually.

## Repository layout

```text
apps/web/       React, Vite, generated API client, unit tests, Playwright
apps/server/    orbit binary, Axum routes, SQLx repositories, composition
crates/orbit/   domain rules and ports
crates/platform reusable config, DB, jobs, files, backup, HTTP, health
config/         deployment configuration example
docs/           design, implementation plan, operations, traceability
```

## Product rules to preserve

- Purple is the accent in light and dark mode.
- The UI is flat-first. Hairline borders divide panes; page-sized card wrappers do not.
- The backend enforces workspace boundaries and roles. UI visibility is not authorization.
- Migrated routes never fall back to mock data on an API error.
- Major records use optimistic versions and return explicit conflicts.
- Destructive actions require confirmation when data loss matters.
- Motion stays restrained and honors reduced-motion settings.
