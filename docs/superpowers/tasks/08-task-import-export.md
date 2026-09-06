# Task 08: Task import and export

**Status:** Backlog, not started
**Priority:** Medium
**Type:** Feature
**Dependencies:** None

## Outcome

Let teams migrate task data into Orbit and retrieve it in a documented portable format.

## Scope

- Define a versioned task exchange format and its supported fields before implementation.
- Export all matching authorized tasks, not just the first loaded page.
- Provide import validation/preview with explicit project, status, assignee, and label mapping.
- Specify duplicate handling and atomicity; report rejected rows instead of silently dropping them.

## Acceptance criteria

- [ ] An export/import round trip preserves all fields declared supported by the format.
- [ ] Large exports include every matching task and exclude other workspaces.
- [ ] Malformed data, unknown identities, and repeat imports produce understandable, tested outcomes.
- [ ] Document exclusions such as attachments/history; do not present task export as a complete installation backup.

## Starting points

- [apps/server/src/task_routes.rs](../../../apps/server/src/task_routes.rs)
- [apps/server/src/repositories/tasks.rs](../../../apps/server/src/repositories/tasks.rs)
- [apps/web/src/features/tasks/TasksPage.tsx](../../../apps/web/src/features/tasks/TasksPage.tsx)
- [apps/web/src/features/tasks/api/tasks.ts](../../../apps/web/src/features/tasks/api/tasks.ts)

## Limits

No third-party integrations, continuous synchronization, or complete backup replacement.

## Execution handoff

This is a backlog task, not an approved step-by-step implementation plan. Recheck current code and the [backlog constraints](README.md#execution-rules) before starting.

- [ ] Confirm task-specific decisions and write the bounded design in `docs/superpowers/specs/`.
- [ ] For code changes, use the writing-plans skill to create a test-first implementation plan in `docs/superpowers/plans/`.
- [ ] Implement only this task, or perform its authorized operational verification; record commands, outcomes, and unresolved failures.
- [ ] Review against every acceptance criterion and link the implementation plan or verification report below before marking complete.

[Back to team-readiness backlog](README.md)
