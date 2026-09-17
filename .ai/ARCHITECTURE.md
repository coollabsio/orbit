# Architecture

## Backend composition

The Cargo workspace has three crates with one-way dependencies:

- `crates/platform` owns reusable infrastructure such as configuration, SQLite setup, migrations, jobs, backup, files, HTTP policy, health, identifiers, and timestamps.
- `crates/orbit` owns domain rules and ports. It does not depend on Axum or SQLx.
- `apps/server` owns SQLx repositories, Axum route adapters, application wiring, the CLI, and the production binary.

`App::build(Config)` in `apps/server/src/app.rs` is the startup boundary. It validates configuration, opens and locks SQLite, creates any required pre-migration backup, runs migrations, performs `quick_check`, checks storage, initializes auth and the one-time setup URL, verifies the embedded frontend contract, then builds the router. `orbit serve` binds only after this returns.

`router.rs` merges independent auth, workspace, task, and attachment routers. It also adds liveness, readiness, embedded assets, SPA fallback, and the common Tower platform layer. Unknown `/api/*` paths return RFC 9457 Problem Details. Only non-API browser routes receive `index.html`.

The public router records bounded request counters. When `metrics.listen` is configured, a separate private listener exposes those identifier-free Prometheus metrics; it is disabled by default and never mounts `/metrics` on the public listener.

On shutdown, the HTTP server stops accepting new connections and drains active requests. Orbit then cancels retention and attachment reconciliation services. Durable job handlers get up to 30 seconds to finish before local tasks are aborted and leases recover naturally.

## Persistence and security

- SQLite uses WAL, foreign keys, a busy timeout, and one process ownership lock.
- Repositories own explicit SQL and workspace scoping. HTTP handlers do not issue feature SQL.
- Major mutations use record versions and return conflict metadata.
- Session cookies are host-only, HTTP-only, SameSite Lax, and Secure in production.
- Insecure cookies are accepted only for an explicit loopback development listener.
- Forwarded transport, client address, and request IDs are trusted only from configured proxy networks.
- All API failures use Problem Details with stable codes and correlated request IDs.
- Attachments stream through bounded request handling and a local blob-store abstraction.
- Backups include SQLite, attachment files, checksums, and schema metadata.

## Frontend data boundaries

React Query and the generated OpenAPI client own persistent server state. Query keys begin with `['workspace', workspaceId]` for workspace-scoped data. Optimistic task writes snapshot relevant caches, roll back rejected requests, and refresh conflict state.

Identity, workspaces, tasks, comments, attachments, trash, and sessions use the API. Docs, mail, chat, DMs, inbox, profile, and home summaries keep their isolated in-memory mock domains. `MockFeatureBadge` labels those routes in development and production.

## Frontend stack

- React 19, TypeScript 6, React Router 8
- TanStack Query and a generated `@hey-api` client
- Vite 8 and React Compiler
- Bun for installs, tests, generation, and builds
- Oxlint and plain CSS with semantic design tokens

`apps/web/index.html` applies the saved theme before paint. `App.tsx` owns routing and providers. `AppShell.tsx` owns global navigation, mobile chrome, and the command palette.

## Embedded build contract

Vite emits `dist/orbit-build.json` from the committed OpenAPI contract ID and `ORBIT_BUILD_REVISION`. Rust embeds the complete `dist` directory and the same compile-time revision. `StaticAssets::verified` refuses to build the application router if either value differs. Hashed assets use a one-year immutable cache policy; HTML and the manifest use `no-cache`.

CI regenerates OpenAPI and TypeScript output twice for determinism, compares it with committed output, rebuilds the frontend, and checks committed embedded assets for drift.

## UI structure

Feature code lives under `apps/web/src/features`. Shared controls live under `components/ui`; global chrome lives under `components/shell`. Chat, DMs, task comments, attachments, markdown, emoji, and mentions share components rather than maintaining lookalikes.

The main responsive breakpoint is 899px. Desktop uses multi-pane layouts. Mobile uses a bottom dock and route-driven master/detail screens. The URL remains the source of truth where practical.
