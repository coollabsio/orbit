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

Orbit is currently a polished **frontend prototype** of a team workspace combining tasks, docs, mail, channel chat, direct messages, notifications, and administration. The web app is React 19 + TypeScript + Vite and uses a synchronous in-memory mock store. Almost all visible interactions work, but changes reset on refresh and there is no backend integration yet. `apps/server` is only a Rust placeholder. The next major phase is replacing the mock store with real APIs, persistence, authentication, permissions, and realtime events without rewriting the established UI.

## First commands

```bash
cd apps/web
aube run dev       # current checkout; http://localhost:8888
aube run build
aube run lint
```

Aube is **not required**. Bun or pnpm can replace it; see `DEVELOPMENT.md`.
