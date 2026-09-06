# Team-readiness Features Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the 14 local Superpowers team-readiness tasks so a small team can use Orbit daily.

**Architecture:** Add bounded migrations and APIs on the existing Axum/SQLite stack. Notifications, saved views, subtasks, dependencies, and recurrences are workspace-scoped tables. Account settings stay on `/api/v1/auth`. SMTP is an optional job-backed adapter. Ops tasks add verification scripts and regression tests rather than new infrastructure.

**Tech Stack:** Rust, Axum, SQLx, SQLite, durable jobs, utoipa-generated client, React, TanStack Query, bun:test, Playwright.

## Global Constraints

- Follow `docs/superpowers/specs/2026-09-06-team-readiness-features-design.md` exactly.
- TDD: failing test first, then minimal code.
- Reuse Problem Details, optimistic versions, workspace auth, audit privacy.
- `just api` after OpenAPI changes; do not hand-edit `apps/web/src/api/generated`.
- No tokens, passwords, SMTP secrets, or backup payloads in logs/audit/docs.
- Do not create git worktrees.
- Do not mark ops tasks 10/11/14 complete without real operator evidence.

---

### Task 1: Command palette cleanup (backlog 09)

**Files:**
- Modify: `apps/web/src/components/shell/CommandPalette.tsx`
- Create: `apps/web/src/components/shell/CommandPalette.test.tsx`
- Modify: `apps/web/src/components/shell/productNavigation.ts` only if palette tests require it; keep `/inbox` and `/profile` disabled until tasks 3 and 4.

**Interfaces:**
- Produces: palette entries limited to navigation of enabled product surfaces plus live tasks.

- [ ] Write failing tests that the palette never lists Docs, Mail, Chat, DM, Inbox, or mock store titles; that a real task result navigates to `/tasks/{id}`; Escape closes; empty query shows nav; no-results copy.

- [ ] Implement: remove `useAppState` and mock product entries; placeholder “Search tasks and navigation…”; keep Tasks, Settings, Sessions, Members, Trash, Home.

- [ ] Run `cd apps/web && bun test src/components/shell/CommandPalette.test.tsx src/components/shell/productNavigation.test.ts`

- [ ] Commit `fix(web): remove mock products from command palette`

---

### Task 2: Account self-service (backlog 03)

**Files:**
- Modify: `apps/server/src/auth_routes.rs`, `apps/server/src/repositories/identity.rs`, `apps/server/src/openapi.rs`
- Modify: `apps/web/src/features/profile/ProfilePage.tsx`, `apps/web/src/features/auth/api.ts`, `apps/web/src/components/shell/UserMenu.tsx`, `apps/web/src/App.tsx`, `apps/web/src/components/shell/productNavigation.ts`
- Test: `apps/server/src/auth_routes.rs` (module tests), `apps/web/src/features/profile/ProfilePage.test.tsx`

**Interfaces:**
- Produces: `PATCH /api/v1/auth/me { display_name }`, `POST /api/v1/auth/password { current_password, new_password }`. Password change revokes other sessions only.

- [ ] Failing backend tests: display name persists; empty name 422; wrong current password does not change hash; new password logs in; other sessions revoked, current cookie still authenticates; password/hash never in audit payload.

- [ ] Implement repository + routes + utoipa; `just api`.

- [ ] Failing frontend tests: profile form uses `/api/v1/auth/me` and `/password`; email read-only; no avatar/2FA/email-change; wrong password keeps new-password draft; UserMenu has Account settings.

- [ ] Enable `/profile` route; strip mock features.

- [ ] Commit `feat: add profile settings and authenticated password change`

---

### Task 3: Notifications and mentions (backlog 01)

**Files:**
- Create: `apps/server/migrations/0012_notifications.sql`
- Modify: migrate.rs, task repository/routes, new notification routes, comment composer, InboxPage, SidebarNav, App.tsx, queryKeys
- Tests: `apps/server/tests/notifications_api.rs`, frontend inbox + composer tests

**Interfaces:**
- Consumes: comment create, task assignee updates
- Produces: `NotificationRecord`, list/read/read-all, `mentioned_user_ids` on comments

- [ ] Failing tests for transactional insert, unique retry, mention IDs only, skip actor, removed member 403, read isolation.

- [ ] Migration + repository + routes + composer picker + real Inbox + unread badge.

- [ ] `just api`; focused cargo + bun tests.

- [ ] Commit `feat: add in-app assignment and mention notifications`

---

### Task 4: Personal task views (backlog 02)

**Files:** task_routes, tasks.rs, TasksPage, TaskFilters, SidebarNav, new saved_views migration 0013, frontend saved-view API

**Interfaces:**
- Produces: `view=mine|overdue|due_soon` on list/export; saved-views CRUD

- [ ] Failing tests: mine only current user; overdue excludes completed/cancelled and undated; due-soon 7 UTC days; midnight boundary; saved views isolated per user.

- [ ] Implement filters, saved views, sidebar, URL `?view=` / `?saved=`.

- [ ] Commit `feat: add my-tasks overdue due-soon and saved views`

---

### Task 5: Invitation and recovery email (backlog 04)

**Files:** config.rs, new `crates/platform/src/mail.rs` or `apps/server/src/mail.rs`, jobs, workspace_routes invitation create, auth recovery_request, orbit.example.toml, operations.md

**Interfaces:**
- Produces: `smtp.mode` disabled|file|smtp; jobs `email.invitation`, `email.recovery`

- [ ] Failing tests: file sink contains invitation URL; recovery 202 whether or not user exists; token not in logs; disabled mode no-op recovery; retry does not duplicate tokens.

- [ ] Implement; keep CLI recovery-link.

- [ ] Commit `feat: deliver invitation and recovery email through optional SMTP`

---

### Task 6: Subtasks (backlog 05)

Migration 0014 `parent_task_id`. One level. Detach on parent delete. List defaults to roots.

- [ ] Tests: cycle/self/cross-project rejected; children keep own fields; list does not duplicate; restore does not reattach.

- [ ] Commit `feat: add one-level task subtasks`

---

### Task 7: Dependencies (backlog 06)

Migration 0015 `task_dependencies`. Informational.

- [ ] Tests: both ends visible; cycle/self/cross-workspace rejected; deleted blocker shows unavailable; no completion enforcement.

- [ ] Commit `feat: add informational task dependencies`

---

### Task 8: Recurring tasks (backlog 07)

Migration 0016 recurrences + occurrences. Job `tasks.recurrence`.

- [ ] Tests: idempotent occurrence key; DST; month-end clamp; pause; missing project pauses recurrence.

- [ ] Commit `feat: add daily weekly monthly recurring tasks`

---

### Task 9: Task import/export (backlog 08)

`GET .../tasks/export`, `POST .../tasks/import`. Format `orbit.tasks.v1`.

- [ ] Tests: round trip supported fields; other workspace excluded; rejected rows reported; dry_run writes nothing.

- [ ] Commit `feat: add versioned task import and export`

---

### Task 10: Access revocation regression tests (backlog 13)

Extend `workspaces_api`, `realtime`, `attachments_api` tests: removed member cannot fetch tasks/comments/attachments; revoked session 401; websocket stops; reinvite restores.

- [ ] Commit `test: verify membership and session revocation`

---

### Task 11: Multiuser collaboration coverage (backlog 12)

Extend e2e/API coverage for two identities: invite, assign, comment, live invalidation. Do not point smoke at production.

- [ ] Commit `test: cover multiuser collaboration flows`

---

### Task 12: Production verification scripts (backlog 10, 11, 14)

Add `deploy/verify-production.sh`, `deploy/backup-rehearsal.sh`, operations monitoring/ownership templates. Record operator-blocked items in task files. Do not claim production HTTPS verified without a real host.

- [ ] Commit `docs: add production verification and backup rehearsal scripts`

---

### Task 13: Update Superpowers task index

Mark completed code tasks done with links to this plan and spec. Leave 10/11/14 blocked on operator host/destination/alerting.

- [ ] Commit `docs: record team-readiness implementation status`
