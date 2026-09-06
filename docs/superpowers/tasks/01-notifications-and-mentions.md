# Task 01: In-app task notifications and mentions

**Status:** Backlog, not started
**Priority:** High
**Type:** Feature
**Dependencies:** None

## Outcome

Help teammates notice assignments and comments without watching a task page.

## Scope

- Add persisted, per-recipient notifications for assignments and explicit mentions in task comments.
- Provide an inbox, unread count, read/unread controls, and links to the relevant task/comment.
- Resolve mentions to workspace member IDs rather than relying on display-name text; specify who receives comment notifications before implementation.
- Keep notification delivery transactional with the task/comment change and reuse authenticated live invalidations.

## Acceptance criteria

- [ ] An assigned or mentioned member sees one notification after a successful write; a rolled-back write produces none.
- [ ] Read state persists across sessions and stays isolated between recipients and workspaces.
- [ ] A user removed from a workspace cannot fetch its notification content or follow its protected links.
- [ ] Keyboard and mobile flows work; retries do not duplicate notifications.

## Starting points

- [apps/server/src/task_routes.rs](../../../apps/server/src/task_routes.rs)
- [apps/server/src/repositories](../../../apps/server/src/repositories)
- [apps/server/src/realtime.rs](../../../apps/server/src/realtime.rs)
- [apps/web/src/features/tasks/components/TaskCommentComposer.tsx](../../../apps/web/src/features/tasks/components/TaskCommentComposer.tsx)
- [apps/web/src/components/shell/SidebarNav.tsx](../../../apps/web/src/components/shell/SidebarNav.tsx)

## Limits

Do not add email delivery, presence, typing, chat persistence, or a general automation engine.

## Execution handoff

This is a backlog task, not an approved step-by-step implementation plan. Recheck current code and the [backlog constraints](README.md#execution-rules) before starting.

- [ ] Confirm task-specific decisions and write the bounded design in `docs/superpowers/specs/`.
- [ ] For code changes, use the writing-plans skill to create a test-first implementation plan in `docs/superpowers/plans/`.
- [ ] Implement only this task, or perform its authorized operational verification; record commands, outcomes, and unresolved failures.
- [ ] Review against every acceptance criterion and link the implementation plan or verification report below before marking complete.

[Back to team-readiness backlog](README.md)
