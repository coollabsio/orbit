# Project overview

## Product and milestone status

Orbit is a self-hosted workspace application with a Rust server and a React web client. Milestone one persists identity, sessions, workspaces, memberships, invitations, projects, workflow statuses, labels, tasks, comments, attachments, trash, and audit records in SQLite.

The production binary embeds the matching Vite build. It serves the SPA and `/api/v1` from one HTTP listener. Operators terminate HTTPS at a reverse proxy.

## Persistent and mock-backed routes

These routes use the production API and SQLite:

- `/setup`, `/login`, `/recovery`, and `/accept-invitation`
- `/tasks`, `/tasks/:taskId`, task trash, and project workflow settings
- `/settings`, `/settings/members`, and `/settings/sessions`

These routes still use frontend mock data and show a `Mock data` badge:

- `/`
- `/docs` and `/docs/:docId`
- `/mail` and `/mail/:threadId`
- `/chat`, channel, thread, and chat settings routes
- `/dm` and `/dm/:dmId`
- `/inbox`
- `/profile`

Mock records never flow into production APIs. Refreshing a mock-backed route recreates its seed data.

## Runtime shape

```text
browser
  -> HTTPS reverse proxy
    -> orbit HTTP listener
      -> embedded SPA
      -> /api/v1 routes
      -> /health/live and /health/ready
      -> SQLite in WAL mode
      -> local attachment storage
      -> local verified backups
```

One Orbit process owns a database file. The process validates configuration, runs guarded migrations and a quick integrity check, verifies writable storage and the embedded client contract, then binds HTTP. Retention and attachment reconciliation services start with the server and stop after HTTP begins graceful shutdown. Backups are started manually.

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
