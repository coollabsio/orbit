# Orbit AI handoff

This directory is the fast-start guide for a new maintainer and their AI agent. Read these files in order before changing code:

1. [`PROJECT_OVERVIEW.md`](./PROJECT_OVERVIEW.md) — product scope, current state, routes, and feature map.
2. [`ARCHITECTURE.md`](./ARCHITECTURE.md) — runtime architecture, state flow, folders, models, and shared components.
3. [`FEATURES.md`](./FEATURES.md) — detailed behavior of every product area.
4. [`DEVELOPMENT.md`](./DEVELOPMENT.md) — setup, package-manager options, commands, testing, and delivery workflow.
5. [`UI_CONVENTIONS.md`](./UI_CONVENTIONS.md) — design rules and interaction patterns that should not regress.
6. [`lessons.md`](./lessons.md) — mistakes already made and project-specific implementation lessons.

`todo.md` is the historical build log. It is useful context, but the documents above describe the current product and take precedence where the old log is stale.

## Current status in one paragraph

Orbit now ships a Rust, Axum, SQLx, and SQLite (or Postgres; see [`AGENTS.md`](../AGENTS.md)) foundation with persistent setup, authentication, workspaces, tasks, collaborative docs, chat, activity, profile, audit, backups, and operations. The matching React build is embedded in one production binary. Mail is the only area without a backend: its UI uses mock data, and `/mail` redirects to `/tasks`. The built-in AI assistant is planned; AI clients use tasks and read docs and chat through the scoped MCP endpoint (`docs/mcp.md`).

## First commands

```bash
cd apps/web
bun run dev        # http://127.0.0.1:8888
bun run build
bun run lint
```

Bun is the only supported frontend package manager in this milestone; see `DEVELOPMENT.md`.
