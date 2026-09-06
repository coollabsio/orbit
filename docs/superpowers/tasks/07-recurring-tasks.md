# Task 07: Recurring tasks

**Status:** Backlog, not started
**Priority:** Medium
**Type:** Feature
**Dependencies:** None

## Outcome

Support routine team work without repeatedly creating the same tasks by hand.

## Scope

- Design an initial daily/weekly/monthly recurrence model, including timezone and missed-run behavior.
- Use the existing durable jobs infrastructure to create task occurrences idempotently.
- Provide controls to create, edit, pause, and stop recurrence.
- Specify which task fields carry forward; do not copy comments or attachments by default.

## Acceptance criteria

- [ ] A scheduled occurrence creates exactly one task even after worker retries or restart.
- [ ] Tests cover DST, month-end dates, missed schedules, and pause/resume behavior.
- [ ] New occurrences receive the intended project, status, assignees, and due dates.
- [ ] Removed workspaces, deleted projects, and invalid assignees cannot cause unauthorized or endlessly failing jobs.

## Starting points

- [crates/platform/src/jobs](../../../crates/platform/src/jobs)
- [apps/server/src/app.rs](../../../apps/server/src/app.rs)
- [apps/server/src/repositories/tasks.rs](../../../apps/server/src/repositories/tasks.rs)
- [apps/web/src/features/tasks/components/TaskDetail.tsx](../../../apps/web/src/features/tasks/components/TaskDetail.tsx)

## Limits

No generic workflow automation or natural-language schedule parser.

## Execution handoff

This is a backlog task, not an approved step-by-step implementation plan. Recheck current code and the [backlog constraints](README.md#execution-rules) before starting.

- [ ] Confirm task-specific decisions and write the bounded design in `docs/superpowers/specs/`.
- [ ] For code changes, use the writing-plans skill to create a test-first implementation plan in `docs/superpowers/plans/`.
- [ ] Implement only this task, or perform its authorized operational verification; record commands, outcomes, and unresolved failures.
- [ ] Review against every acceptance criterion and link the implementation plan or verification report below before marking complete.

[Back to team-readiness backlog](README.md)
