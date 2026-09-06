# Task 04: Automatic invitation and password-recovery email

**Status:** Backlog, not started
**Priority:** Medium
**Type:** Feature
**Dependencies:** None

## Outcome

Remove manual link sharing as the normal onboarding and recovery workflow.

## Scope

- Add optional outbound SMTP configuration and durable delivery jobs for invitations and recovery messages.
- Retain manual invitation links and administrator-issued recovery as supported fallbacks.
- Use the configured HTTPS public origin; preserve token expiry, single-use, replacement, and rate-limit protections.
- Expose actionable delivery failures without logging tokens, passwords, or SMTP credentials.

## Acceptance criteria

- [ ] A controlled mail sink receives usable invitation and recovery links.
- [ ] Recovery responses do not reveal whether an account exists.
- [ ] Retries and process restarts cannot create duplicate accounts or bypass single-use token rules.
- [ ] Expired/revoked links fail, SMTP failure is visible, and manual workflows still work when SMTP is disabled.

## Starting points

- [apps/server/src/workspace_routes.rs](../../../apps/server/src/workspace_routes.rs)
- [apps/server/src/auth_routes.rs](../../../apps/server/src/auth_routes.rs)
- [crates/platform/src/jobs](../../../crates/platform/src/jobs)
- [apps/web/src/features/auth/AuthGate.tsx](../../../apps/web/src/features/auth/AuthGate.tsx)
- [docs/operations.md](../../../docs/operations.md)

## Limits

Outbound transactional email only; no inbound SMTP, mailbox product, notification digests, or provider abstraction unless required.

## Execution handoff

This is a backlog task, not an approved step-by-step implementation plan. Recheck current code and the [backlog constraints](README.md#execution-rules) before starting.

- [ ] Confirm task-specific decisions and write the bounded design in `docs/superpowers/specs/`.
- [ ] For code changes, use the writing-plans skill to create a test-first implementation plan in `docs/superpowers/plans/`.
- [ ] Implement only this task, or perform its authorized operational verification; record commands, outcomes, and unresolved failures.
- [ ] Review against every acceptance criterion and link the implementation plan or verification report below before marking complete.

[Back to team-readiness backlog](README.md)
