# Milestone-one traceability

The source design is [`2026-09-03-backend-platform-design.md`](superpowers/specs/2026-09-03-backend-platform-design.md). The implementation plan is [`2026-09-04-foundation-tasks-backend.md`](superpowers/plans/2026-09-04-foundation-tasks-backend.md).

| Decisions | Production evidence | Automated evidence |
|---|---|---|
| D-001 to D-006, D-019 to D-024 | three-crate workspace, SQLite ownership, SQLx repositories, Axum routers | domain, database, repository, and API suites under `crates/*/tests` and `apps/server/tests` |
| D-007, D-036 to D-042 | migrations, integrity service, durable job store, scheduler, worker drain, automatic backup and retention composition | `crates/platform/tests/database.rs`, `integrity.rs`, `jobs.rs`, `backup.rs`; `apps/server/tests/app_smoke.rs` |
| D-008 to D-018, D-029 to D-035 | setup, sessions, invite-only membership, roles, suspension, trash, audit, admin-link recovery | auth and workspace route/repository tests; `workspaces_api.rs`; Playwright foundation flow |
| D-025, D-026, D-028, D-043 to D-046 | deterministic OpenAPI, generated client, workspace query keys, Problem Details, optimistic conflicts | `openapi_contract.rs`, `just api-check`, Bun API and hook tests |
| D-027, D-047 to D-049 | bounded streaming uploads, local blob abstraction, deduplication, reconciliation | platform file tests and `attachments_api.rs` |
| D-057 to D-060, D-062, D-064 to D-066 | `App::build`, production router, structured logs, health, restrictive HTTP layer, Just/Bun/CI | `app_smoke.rs`, HTTP security tests, `just check`, `just e2e`, Docker help smoke |
| D-061 | append-only scoped audit and one-year retention job | repository audit/retention tests and workspace API tests |
| D-063 | shared escaped source-text rendering | frontend markdown and feature tests |
| D-067 to D-069 | empty production defaults, no mock import, staged frontend cutover and route badges | setup/workspace API tests, React Query tests, Playwright flow, `MockFeatureBadge.test.ts` |

## Deferred by the approved design

- D-050 to D-052: WebSocket outbox, replay, resync, presence, and typing transport.
- D-053: outbound provider delivery. Milestone one keeps manual invitation and administrator-issued recovery links.
- D-054 to D-056: inbound SMTP, public-MX and relay modes, domains, and mailboxes.
- Persistent Docs, Mail, Chat, DMs, Inbox, Profile, and Home data.

The application labels deferred UI routes `Mock data`. It does not expose a mock-state importer, public registration, custom permission roles, public attachment URLs, or an inbound SMTP listener.
