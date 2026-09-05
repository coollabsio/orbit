# Tasks Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver a focused, polished, persistent task product and disable all unfinished mock-backed modules.

**Architecture:** Extend the existing task repository/API and generated client for due dates, activity, and search. Keep SMTP behind the existing delivery boundaries and durable job system. Restrict the frontend shell and route table to persistent functionality, then verify complete workflows through mounted tests and Playwright.

**Tech Stack:** Rust, SQLite/SQLx, Axum, Utoipa/OpenAPI, React, TanStack Query, Bun, Playwright.

## Global Constraints

- HTTP handlers issue no SQL; persistence stays in repositories.
- Workspace-owned data is scoped by workspace in SQL and query keys.
- Mutations use observed versions and atomic audit writes.
- No bearer token appears in logs or diagnostics.
- No mock-backed route remains activatable.

---

### Task 1: Focus the product shell

**Files:** `apps/web/src/app/*`, `apps/web/src/components/shell/*`, related tests.

- [ ] Add failing route/navigation tests for Tasks as the landing page, disabled mock modules, keyboard behavior, direct-route redirects, and absent mock counts.
- [ ] Implement the focused shell and redirects with accessible `Coming soon` affordances.
- [ ] Run Bun tests, lint, and build; commit.

### Task 2: Due dates, search, and activity API

**Files:** next additive server migration, `repositories/tasks.rs`, `task_routes.rs`, OpenAPI and API tests.

- [ ] Add failing repository/API tests for optional due dates, version conflicts, search pagination/query binding, and task-scoped activity isolation/pagination.
- [ ] Add the schema and repository operations, then expose typed routes without handler SQL.
- [ ] Regenerate OpenAPI/client and pass drift tests; commit.

### Task 3: Task UI polish

**Files:** task hooks/components/pages and mounted tests.

- [ ] Add failing UI tests for due-date states/editing, search pagination, activity, loading/empty/error/retry, mobile layout, keyboard and live-region behavior.
- [ ] Implement using only the generated client and workspace-scoped query keys.
- [ ] Verify optimistic rollback, controlled conflict refresh, attachment progress, board order, and bulk actions; commit.

### Task 4: SMTP delivery

**Files:** platform/server mail adapter, config, jobs, auth/workspace delivery wiring, tests, operations docs.

- [ ] Add failing tests for SMTP configuration, durable invitation/recovery jobs, retries, generic public responses, manual fallback, and secret redaction.
- [ ] Implement the smallest SMTP adapter behind existing delivery boundaries and durable jobs.
- [ ] Document configuration and operational failure handling; commit.

### Task 5: Milestone verification

**Files:** Playwright flow and any focused regression files.

- [ ] Expand Playwright across login, task due date/search/activity/comments/attachments/board/trash and disabled routes.
- [ ] Run `just check`, API drift, production build, and Playwright.
- [ ] Fix regressions, verify Jean/Tailscale URLs, and commit the gate evidence.

