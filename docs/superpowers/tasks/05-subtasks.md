# Task 05: Task subtasks

**Status:** Backlog, not started
**Priority:** Medium
**Type:** Feature
**Dependencies:** None

## Outcome

Break a larger task into independently assignable, trackable work.

## Scope

- Add parent/child relationships scoped to one project and workspace.
- Show subtasks and a parent link in task details, with create, attach, and detach actions.
- Define parent completion and delete/restore behavior before implementation; do not silently complete children.
- Reject self-parenting and cycles.

## Acceptance criteria

- [ ] Subtasks retain their own assignees, status, and due date.
- [ ] Cross-workspace/project links and cycles are rejected server-side.
- [ ] Parent and child navigation works after reload; list/board display does not accidentally duplicate entries.
- [ ] Deletion, restoration, and concurrent relationship edits follow tested rules.

## Starting points

- [apps/server/src/repositories/tasks.rs](../../../apps/server/src/repositories/tasks.rs)
- [apps/server/src/task_routes.rs](../../../apps/server/src/task_routes.rs)
- [apps/web/src/features/tasks/components/TaskDetail.tsx](../../../apps/web/src/features/tasks/components/TaskDetail.tsx)
- [apps/web/src/features/tasks/api/models.ts](../../../apps/web/src/features/tasks/api/models.ts)

## Limits

No unlimited hierarchy UI, progress rollups, or dependency scheduling.

## Execution handoff

This is a backlog task, not an approved step-by-step implementation plan. Recheck current code and the [backlog constraints](README.md#execution-rules) before starting.

- [ ] Confirm task-specific decisions and write the bounded design in `docs/superpowers/specs/`.
- [ ] For code changes, use the writing-plans skill to create a test-first implementation plan in `docs/superpowers/plans/`.
- [ ] Implement only this task, or perform its authorized operational verification; record commands, outcomes, and unresolved failures.
- [ ] Review against every acceptance criterion and link the implementation plan or verification report below before marking complete.

[Back to team-readiness backlog](README.md)
