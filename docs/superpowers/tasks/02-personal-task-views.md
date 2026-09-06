# Task 02: My tasks, overdue, due-soon, and saved views

**Status:** Backlog, not started
**Priority:** High
**Type:** Feature
**Dependencies:** None

## Outcome

Give each teammate a reliable view of their own upcoming work.

## Scope

- Add My tasks, Overdue, and Due soon views using the authenticated user and real task data.
- Support saving named personal filter/sort/layout combinations within a workspace.
- Define due-soon boundaries and timezone behavior explicitly; exclude completed/cancelled work from overdue by default.
- Apply filters across the full matching dataset rather than only loaded pages.

## Acceptance criteria

- [ ] My tasks contains only tasks assigned to the current user within the selected workspace.
- [ ] Boundary tests cover today, midnight, timezone changes, and tasks without due dates.
- [ ] Saved views survive reload and sign-in; another user cannot read or modify them.
- [ ] Navigation, browser history, empty states, and pagination preserve the selected view.

## Starting points

- [apps/web/src/features/tasks/TasksPage.tsx](../../../apps/web/src/features/tasks/TasksPage.tsx)
- [apps/web/src/features/tasks/components/TaskFilters.tsx](../../../apps/web/src/features/tasks/components/TaskFilters.tsx)
- [apps/web/src/features/tasks/api/tasks.ts](../../../apps/web/src/features/tasks/api/tasks.ts)
- [apps/server/src/task_routes.rs](../../../apps/server/src/task_routes.rs)

## Limits

No dashboards, charts, shared team views, or reporting engine in the first version.

## Execution handoff

This is a backlog task, not an approved step-by-step implementation plan. Recheck current code and the [backlog constraints](README.md#execution-rules) before starting.

- [ ] Confirm task-specific decisions and write the bounded design in `docs/superpowers/specs/`.
- [ ] For code changes, use the writing-plans skill to create a test-first implementation plan in `docs/superpowers/plans/`.
- [ ] Implement only this task, or perform its authorized operational verification; record commands, outcomes, and unresolved failures.
- [ ] Review against every acceptance criterion and link the implementation plan or verification report below before marking complete.

[Back to team-readiness backlog](README.md)
