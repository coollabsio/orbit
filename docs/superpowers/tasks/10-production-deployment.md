# Task 10: Verify a team production deployment

**Status:** Backlog, not started
**Priority:** Before rollout
**Type:** Operations
**Dependencies:** None

## Outcome

Deploy the existing application safely for team access, rather than exposing development.

## Scope

- Choose the actual hostname, host, storage locations, and HTTPS proxy before touching infrastructure.
- Use the existing deployment recipe with fresh production storage, correct public origin, and narrow trusted-proxy configuration.
- Build the current frontend/backend together, complete owner setup, and verify container restart persistence.
- Record deployed version, URL, upgrade procedure, and ownership without storing credentials or setup tokens.

## Acceptance criteria

- [ ] Real HTTPS, readiness, Secure/HttpOnly session cookies, and origin enforcement pass on the selected host.
- [ ] Development credentials and seeding are rejected; no development database is copied into production.
- [ ] Tasks and attachments survive restart; only one application owns the database.
- [ ] Actual verification results and any remaining deployment restrictions are documented.

## Starting points

- [deploy/README.md](../../../deploy/README.md)
- [deploy/compose.yaml](../../../deploy/compose.yaml)
- [deploy/orbit.toml](../../../deploy/orbit.toml)
- [deploy/Caddyfile](../../../deploy/Caddyfile)
- [deploy/smoke.sh](../../../deploy/smoke.sh)
- [docs/operations.md](../../../docs/operations.md)

## Limits

Most mechanisms already exist. This task verifies/configures the target deployment; do not rebuild them or provision infrastructure without authorization.

## Execution handoff

This is a backlog task, not an approved step-by-step implementation plan. Recheck current code and the [backlog constraints](README.md#execution-rules) before starting.

- [ ] Confirm task-specific decisions and write the bounded design in `docs/superpowers/specs/`.
- [ ] For code changes, use the writing-plans skill to create a test-first implementation plan in `docs/superpowers/plans/`.
- [ ] Implement only this task, or perform its authorized operational verification; record commands, outcomes, and unresolved failures.
- [ ] Review against every acceptance criterion and link the implementation plan or verification report below before marking complete.

[Back to team-readiness backlog](README.md)
