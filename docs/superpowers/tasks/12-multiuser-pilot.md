# Task 12: Run the end-to-end multiuser team pilot

**Status:** Backlog, not started
**Priority:** Before rollout
**Type:** Verification
**Dependencies:** [Task 10](10-production-deployment.md)

## Outcome

Validate normal teamwork against the real backend before relying on Orbit.

## Scope

- Use two distinct accounts and browser profiles, not two tabs sharing one identity.
- Exercise invitations for new/existing/wrong-account users, then assignment, edits, board moves, comments, uploads, and trash/restore.
- Exercise simultaneous edits, connection loss/reconnect, and mobile task interaction.
- Record outcomes and create separate local tasks for reproducible defects; do not hide failures by weakening tests.

## Acceptance criteria

- [ ] Invited users join only the intended workspace and receive the correct role.
- [ ] Task/comment changes arrive in the other profile without a manual reload.
- [ ] Reconnect catches up, conflicts preserve recoverable drafts, and task attachments remain accessible to authorized users.
- [ ] The report states tested build, URL, browsers, account roles, and any unresolved blockers without exposing secrets.

## Starting points

- [apps/web/e2e/foundation-tasks.spec.ts](../../../apps/web/e2e/foundation-tasks.spec.ts)
- [apps/web/e2e/invitation-acceptance.spec.ts](../../../apps/web/e2e/invitation-acceptance.spec.ts)
- [apps/web/scripts/smoke-collaboration.cjs](../../../apps/web/scripts/smoke-collaboration.cjs)
- [deploy/README.md](../../../deploy/README.md)

## Limits

Existing smoke automation uses a seeded development account. Do not run it unchanged against production; use authorized disposable pilot data.

## Execution handoff

This is a backlog task, not an approved step-by-step implementation plan. Recheck current code and the [backlog constraints](README.md#execution-rules) before starting.

- [ ] Confirm task-specific decisions and write the bounded design in `docs/superpowers/specs/`.
- [ ] For code changes, use the writing-plans skill to create a test-first implementation plan in `docs/superpowers/plans/`.
- [ ] Implement only this task, or perform its authorized operational verification; record commands, outcomes, and unresolved failures.
- [ ] Review against every acceptance criterion and link the implementation plan or verification report below before marking complete.

[Back to team-readiness backlog](README.md)
