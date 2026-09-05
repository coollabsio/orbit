# Tasks Polish Design

## Goal

Turn Orbit into a focused, daily-usable task application. Only persistent milestone features remain navigable; unfinished mock modules are visibly unavailable.

## Product shell

- After authentication, `/` redirects to the selected workspace's Tasks view.
- Home, Docs, Mail, Chat, direct messages, Inbox, and Profiles remain visible only as disabled `Coming soon` navigation where useful for product direction.
- Disabled items cannot be activated by pointer or keyboard. Direct navigation to their routes redirects to Tasks.
- The usable shell contains no mock counts, mock notifications, or claims that mock data is live.
- Tasks, task trash, workspace settings, member management, session management, and account actions remain enabled.

## Task essentials

- Tasks have an optional due date stored as an RFC 3339 UTC timestamp and protected by the existing optimistic version contract.
- Task detail and list/board views render overdue, due-today, upcoming, and no-date states consistently.
- Workspace task search runs on the server across title and description, uses existing cursor pagination, and is available from the task page and command palette.
- Each task exposes an immutable activity stream derived from audited task, comment, attachment, assignment, label, status, priority, due-date, delete, and restore mutations. Entries are workspace/task scoped and cursor-paginated.
- Existing labels, assignees, priorities, filters, integer board ordering, bulk operations, comments, attachments, trash, and conflict handling remain authoritative and persistent.

## Team delivery

- Outbound mail uses an SMTP adapter configured through TOML/environment variables.
- Invitation and recovery messages use durable jobs and record delivery outcome without exposing bearer links in logs.
- When SMTP is disabled, manual administrator-issued invitation and recovery links remain available and public recovery stays a generic no-op.

## Polish requirements

- Every usable route has explicit loading, empty, error, retry, and mutation-progress states.
- Version conflicts refresh controlled fields from the authoritative response without losing unrelated drafts.
- Keyboard focus, labels, live regions, disabled semantics, and contrast meet the existing accessibility conventions.
- Primary task workflows remain usable on narrow mobile viewports.
- No operation silently partially succeeds; multi-file progress and retry resume from persisted work.

## Verification

- Backend tests cover due-date validation/version conflicts, activity isolation/pagination, search binding, and SMTP retry/redaction behavior.
- Frontend tests cover disabled navigation/direct-route redirects, due-date states, activity/search loading, and accessible failures.
- Playwright covers login, task create/edit/due date/search, comment, attachment, board/list transition, trash/restore, and disabled-module navigation.
- `just check`, API drift, production build, and the browser milestone must pass.

