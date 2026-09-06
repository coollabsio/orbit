# Task 06: Blocked-by and blocking task dependencies

**Status:** Backlog, not started
**Priority:** Medium
**Type:** Feature
**Dependencies:** None

## Outcome

Make task blockers visible without relying on comments.

## Scope

- Add directed dependency links between tasks within a workspace.
- Show blocked-by and blocking relationships in task details with links and removal controls.
- Define whether incomplete blockers merely warn or prevent completion; keep the first version informational unless the design explicitly requires enforcement.
- Reject cycles and handle deleted or inaccessible tasks without leaking details.

## Acceptance criteria

- [ ] Both ends of a dependency show the relationship after refresh.
- [ ] Self-links, duplicate edges, cycles, and cross-workspace edges are rejected.
- [ ] Completing, reopening, deleting, and restoring blockers updates the displayed blocked state.
- [ ] Concurrent writes and unauthorized access have regression tests.

## Starting points

- [apps/server/src/repositories/tasks.rs](../../../apps/server/src/repositories/tasks.rs)
- [apps/server/src/task_routes.rs](../../../apps/server/src/task_routes.rs)
- [apps/web/src/features/tasks/components/TaskDetail.tsx](../../../apps/web/src/features/tasks/components/TaskDetail.tsx)

## Limits

Independent of subtasks. No Gantt chart, automatic date propagation, or critical-path calculation.

## Execution handoff

This is a backlog task, not an approved step-by-step implementation plan. Recheck current code and the [backlog constraints](README.md#execution-rules) before starting.

- [ ] Confirm task-specific decisions and write the bounded design in `docs/superpowers/specs/`.
- [ ] For code changes, use the writing-plans skill to create a test-first implementation plan in `docs/superpowers/plans/`.
- [ ] Implement only this task, or perform its authorized operational verification; record commands, outcomes, and unresolved failures.
- [ ] Review against every acceptance criterion and link the implementation plan or verification report below before marking complete.

[Back to team-readiness backlog](README.md)
