# Task 03: Profile settings and authenticated password changes

**Status:** Backlog, not started
**Priority:** High
**Type:** Feature
**Dependencies:** None

## Outcome

Let team members maintain their accounts without administrator intervention.

## Scope

- Replace the disabled profile route with real account settings and link it from the sidebar profile menu.
- Allow editing the display name and changing the password after verifying the current password.
- Show the account email read-only until a verified email-change workflow is separately designed.
- Define other-session invalidation behavior after a password change and explain it in the UI.

## Acceptance criteria

- [ ] Display-name edits persist and appear consistently after refresh.
- [ ] An incorrect current password cannot change credentials; errors allow retry without losing unrelated edits.
- [ ] The new password works, the old password fails, and session invalidation follows the documented policy.
- [ ] Only the authenticated account can be edited; sensitive values never enter logs or audit payloads.

## Starting points

- [apps/web/src/features/auth](../../../apps/web/src/features/auth)
- [apps/web/src/components/shell/UserMenu.tsx](../../../apps/web/src/components/shell/UserMenu.tsx)
- [apps/web/src/App.tsx](../../../apps/web/src/App.tsx)
- [apps/server/src/auth_routes.rs](../../../apps/server/src/auth_routes.rs)
- [crates/platform/src/auth](../../../crates/platform/src/auth)

## Limits

No avatar uploads, arbitrary email changes, MFA, or SSO in this task. Self-service recovery delivery is Task 04.

## Execution handoff

This is a backlog task, not an approved step-by-step implementation plan. Recheck current code and the [backlog constraints](README.md#execution-rules) before starting.

- [ ] Confirm task-specific decisions and write the bounded design in `docs/superpowers/specs/`.
- [ ] For code changes, use the writing-plans skill to create a test-first implementation plan in `docs/superpowers/plans/`.
- [ ] Implement only this task, or perform its authorized operational verification; record commands, outcomes, and unresolved failures.
- [ ] Review against every acceptance criterion and link the implementation plan or verification report below before marking complete.

[Back to team-readiness backlog](README.md)
