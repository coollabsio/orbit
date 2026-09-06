# Task 13: Verify membership removal and session revocation

**Status:** Backlog, not started
**Priority:** Before rollout
**Type:** Verification
**Dependencies:** [Task 10](10-production-deployment.md)

## Outcome

Prove that removing access works for already-open clients as well as new requests.

## Scope

- Test member removal, session revocation/expiry, and account suspension using disposable authorized accounts.
- Exercise open HTTP pages, live WebSocket connections, direct task links, and attachment downloads.
- Verify access boundaries between two workspaces and role restrictions for sensitive operations.
- Capture expected versus observed behavior and turn failures into scoped regression fixes.

## Acceptance criteria

- [ ] Removed members and revoked sessions cannot fetch protected task/comment/attachment content.
- [ ] Already-open live connections stop delivering workspace updates after access is revoked.
- [ ] A member cannot gain another workspace role or perform owner-only operations through crafted requests.
- [ ] Reinvitation or a legitimate new session restores only the intended access.

## Starting points

- [apps/server/tests/workspaces_api.rs](../../../apps/server/tests/workspaces_api.rs)
- [apps/server/tests/realtime.rs](../../../apps/server/tests/realtime.rs)
- [apps/server/tests/attachments_api.rs](../../../apps/server/tests/attachments_api.rs)
- [apps/web/src/app/authSession.ts](../../../apps/web/src/app/authSession.ts)
- [docs/operations.md](../../../docs/operations.md)

## Limits

Do not revoke real teammates or disable the only owner as part of testing.

## Execution handoff

This is a backlog task, not an approved step-by-step implementation plan. Recheck current code and the [backlog constraints](README.md#execution-rules) before starting.

- [ ] Confirm task-specific decisions and write the bounded design in `docs/superpowers/specs/`.
- [ ] For code changes, use the writing-plans skill to create a test-first implementation plan in `docs/superpowers/plans/`.
- [ ] Implement only this task, or perform its authorized operational verification; record commands, outcomes, and unresolved failures.
- [ ] Review against every acceptance criterion and link the implementation plan or verification report below before marking complete.

[Back to team-readiness backlog](README.md)
