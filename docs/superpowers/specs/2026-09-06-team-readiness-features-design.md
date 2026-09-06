# Team-readiness features design

**Date:** 2026-09-06  
**Status:** Approved for implementation (YOLO execution of the local Superpowers backlog)

This spec locks product decisions for the 14 team-readiness tasks. Reuse the existing Rust/Axum/SQLx/SQLite backend, durable jobs, generated API client, React, and TanStack Query. Preserve workspace authorization, optimistic versions, Problem Details, audit privacy, and production/development separation.

## Shared rules

- New tables live in forward-only migrations `0012` onward, registered in `crates/platform/src/db/migrate.rs`.
- Workspace-scoped resources go under `/api/v1/workspaces/{workspace_id}/...` except account routes, which stay under `/api/v1/auth/...`.
- Successful workspace writes keep using `audit_events` so the existing realtime trigger emits `workspace.changed`. Frontend query keys for new workspace data must sit under `queryKeys.workspace(id)`.
- Regenerate the OpenAPI client with `just api` after route changes.
- Tests first. No tokens, passwords, SMTP credentials, or backup payloads in logs, audit JSON, or docs.

## Task 01 — Notifications and mentions

**Who is notified**

- `task_assigned`: each newly added assignee except the actor.
- `comment_mentioned`: each `mentioned_user_ids` entry except the actor.
- Commenting does **not** notify all assignees or previous commenters.
- Mentions are member **user IDs** supplied by the client. Display-name text is never used to resolve recipients.

**Persistence**

```
notifications (
  id TEXT PK,
  workspace_id TEXT NOT NULL,
  recipient_user_id TEXT NOT NULL,
  actor_user_id TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('task_assigned', 'comment_mentioned')),
  task_id TEXT NOT NULL,
  comment_id TEXT,
  read_at INTEGER,
  created_at INTEGER NOT NULL
)
UNIQUE (workspace_id, recipient_user_id, kind, task_id, comment_id)
```

Insert in the same transaction as the task/comment write. A unique constraint plus `INSERT OR IGNORE` makes retries idempotent.

**API**

- `GET /api/v1/workspaces/{id}/notifications?unread=true|false` cursor page
- `POST /api/v1/workspaces/{id}/notifications/{id}/read`
- `POST /api/v1/workspaces/{id}/notifications/read-all`
- `CreateCommentBody.mentioned_user_ids: Vec<String>` (default empty); each ID must be an active workspace member

Removed members cannot list or mark that workspace’s notifications. Following a notification link uses existing task auth.

**UI**

- Enable `/inbox` as the real inbox (All / Unread / Mentions). Drop mock Docs/Mail/Chat resource links.
- Sidebar Inbox shows unread count; enable the nav item.
- Comment composer: `@` opens a member picker and records IDs; the body may still show `@Name` for humans.

## Task 02 — Personal views

**Server filters** on `GET .../tasks` (applied to the full matching set, not the current page):

- `view=mine` — assigned to the authenticated user
- `view=overdue` — `due_at < start of current UTC day` and status category not `completed` or `cancelled`
- `view=due_soon` — `due_at` in `[start of current UTC day, start of current UTC day + 7 days)` and not completed/cancelled
- Tasks without `due_at` are excluded from overdue and due-soon
- Timezone for v1 is **UTC**. Document that in the UI.

**Saved views**

```
saved_views (
  id TEXT PK,
  workspace_id, user_id, name TEXT NOT NULL,
  query_json TEXT NOT NULL,
  layout TEXT NOT NULL CHECK (layout IN ('list','board')),
  created_at, updated_at,
  UNIQUE (workspace_id, user_id, name)
)
```

CRUD under `/api/v1/workspaces/{id}/saved-views`. Isolated per user. Query JSON stores `{view, project_id, status_id, assignee_id, label_id, priority, search, sort, order, layout}`.

**UI:** sidebar personal links My tasks / Overdue / Due soon; Tasks page reads `?view=` and saved-view id from the URL; empty states; browser history preserved.

## Task 03 — Account self-service

- `PATCH /api/v1/auth/me` `{ display_name }` — trim, reject empty, return `AuthUserResponse`
- `POST /api/v1/auth/password` `{ current_password, new_password }` — verify current, validate new with existing password policy, hash, update. **Revoke every other session; keep the current session.** UI copy: “Other devices will be signed out.”
- Email is read-only. No avatar, email change, or 2FA in this task.
- Enable `/profile`. Strip mock avatar / email-change / 2FA from `ProfilePage`. Link “Account settings” from the sidebar user menu.
- Wrong current password: 401/422 problem details, form keeps the new-password fields.

## Task 04 — Invitation and recovery email

Optional `[smtp]` config:

```
[smtp]
mode = "disabled" | "file" | "smtp"
from = "Orbit <orbit@example.com>"
# file:
directory = "data/mail"
# smtp:
host = "127.0.0.1"
port = 587
username = ""
# password via ORBIT__SECRETS__SMTP_PASSWORD
starttls = true
```

Job kinds `email.invitation` and `email.recovery`. Payload: `{to, subject, text, token_id}` — **never the raw token after enqueue if it can be avoided**; the job body may include the one-time URL because it is the delivery vehicle, but logs/audit must redact `token`, `url`, and credentials (`JobKind::with_sensitive_fields`).

When SMTP is enabled:

- Creating an invitation enqueues `email.invitation` with `{public_origin}/accept-invitation?token=...`
- `POST /api/v1/auth/recovery/request` creates a 30-minute recovery token (same as CLI) and enqueues email when the account exists. Response stays generic `202` either way.

When disabled: current manual invitation links and CLI `recovery-link` remain. Public recovery remains a generic no-op **except** it still returns 202.

`file` mode writes `.eml` files for tests and local sinks. Failures surface on the invitation row (`last_delivery_error`) and recovery remains generic. Retries use job idempotency keys `email.invitation:{invitation_id}` / `email.recovery:{token_hash}`.

## Task 05 — Subtasks

- `tasks.parent_task_id TEXT NULL REFERENCES tasks(id)` same workspace **and** project, one level only.
- Reject self-parent, cycles, cross-project/workspace.
- Completing a parent does **not** complete children.
- Deleting a parent **detaches** children (`parent_task_id = NULL`); restore does not reattach.
- List/board default: `parent_task_id IS NULL` so children are not duplicated. `GET` accepts `parent_task_id` (including the parent id) and `include_subtasks=true`.
- Task detail: subtask list, create, attach existing, detach, link to parent.

## Task 06 — Dependencies

Informational only; completion is **not** blocked.

```
task_dependencies (
  workspace_id, blocker_task_id, blocked_task_id,
  PRIMARY KEY (blocker_task_id, blocked_task_id)
)
```

Reject self-links, duplicates, cycles, cross-workspace. Deleted tasks omit inaccessible titles (`Unavailable task`). Show blocked-by and blocking lists with add/remove on task detail.

## Task 07 — Recurring tasks

```
task_recurrences (
  id, workspace_id, project_id, status_id, title, description, priority,
  assignee_ids_json, label_ids_json,
  frequency TEXT CHECK (frequency IN ('daily','weekly','monthly')),
  interval INTEGER NOT NULL DEFAULT 1,
  timezone TEXT NOT NULL DEFAULT 'UTC',
  next_run_at INTEGER NOT NULL,
  paused_at INTEGER,
  created_by, created_at, updated_at
)
task_recurrence_occurrences (
  recurrence_id, occurrence_key TEXT, task_id,
  PRIMARY KEY (recurrence_id, occurrence_key)
)
```

Worker job `tasks.recurrence` on a one-minute schedule (or claim due recurrences). Occurrence key is the intended civil date in the recurrence timezone (`YYYY-MM-DD`). Missed runs catch up one occurrence per claim, then advance `next_run_at`. Pause skips creation. Stop deletes the recurrence (existing tasks remain). Do not copy comments or attachments. Invalid project/workspace/assignee: mark recurrence paused with a delivery error, do not loop.

Monthly: clamp to last day of month. DST: use timezone offset at `next_run_at`.

## Task 08 — Import / export

Versioned JSON `orbit.tasks.v1`:

```
{ "format": "orbit.tasks.v1", "exported_at": RFC3339,
  "tasks": [{ title, description, priority, due_at, status_name, project_key,
              assignee_emails, label_names, parent_title?, blocked_by_titles? }] }
```

- `GET .../tasks/export` uses the same filters as list, **all pages**, current workspace only. Excludes attachments, comments, activity, trash.
- `POST .../tasks/import` `{ dry_run, project_id, mapping, tasks }`. Preview (`dry_run=true`) returns `{ accepted, rejected[{index, reason}] }`. Apply is **not** all-or-nothing: accepted rows commit, rejected rows are reported. Repeat import creates new tasks (no implicit upsert). Unknown emails/labels/statuses reject that row.

Document exclusions in the UI.

## Task 09 — Command palette

Remove Docs, Mail, Chat, DM, Inbox mock results and those navigation entries. Results: Home (redirects to tasks), Tasks, Settings, Sessions, Members, Task trash, and live task search for the active workspace. Placeholder: “Search tasks and navigation…”. Drop `useAppState` from the palette.

Inbox/Profile palette entries are added by tasks 01 and 03 when those routes are real.

## Tasks 10–14 — Operations

These verify existing mechanisms. This implementation adds:

- `deploy/verify-production.sh` — HTTPS, readiness, Secure cookies, origin, restart persistence checks against `ORBIT_VERIFY_ORIGIN`.
- `deploy/backup-rehearsal.sh` — verify snapshot, restore into a **separate** data dir, never the live path.
- Automated API/e2e coverage for invitation/task/comment/realtime/reconnect (task 12) and membership removal, session revoke, attachment 401 after revoke (task 13).
- `docs/operations.md` sections: monitoring endpoints, backup freshness, ownership table template, upgrade/rollback pointers.

**Cannot complete without an operator:** real DNS/HTTPS host, off-host backup destination, alert destination, named on-call. Those remain recorded as blocked operator steps in the task files, not fake-passed.

## Out of scope

Email for notifications, MFA, SSO, avatars, unlimited subtask trees, Gantt, third-party sync, building a monitoring product inside Orbit.
