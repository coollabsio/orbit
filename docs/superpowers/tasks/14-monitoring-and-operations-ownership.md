# Task 14: Readiness monitoring and operational ownership

**Status:** Backlog, not started
**Priority:** Before rollout
**Type:** Operations
**Dependencies:** [Task 10](10-production-deployment.md)

## Outcome

Make service failures actionable and establish who owns recovery and upgrades.

## Scope

- Choose monitoring and alert delivery with the operator; use existing readiness/liveness endpoints and optional private metrics.
- Monitor backup freshness, service availability, and storage capacity using supported host/deployment tooling.
- Assign primary/fallback responsibility for account recovery, backups, upgrades, and incident response.
- Document alert thresholds and test a safe failure scenario without taking down the team instance.

## Acceptance criteria

- [ ] A simulated outage or failing readiness check reaches the chosen responder.
- [ ] Backup freshness and storage alerts have an explicit action in the runbook.
- [ ] Metrics remain private; alerts and logs contain no invitation/recovery tokens or credentials.
- [ ] The operator can follow the documented upgrade, rollback, recovery, and escalation procedures.

## Starting points

- [apps/server/src/metrics.rs](../../../apps/server/src/metrics.rs)
- [docs/operations.md](../../../docs/operations.md)
- [deploy/README.md](../../../deploy/README.md)
- [.ai/DEVELOPMENT.md](../../../.ai/DEVELOPMENT.md)

## Limits

Prefer existing monitoring services. Do not build a monitoring platform inside Orbit.

## Execution handoff

This is a backlog task, not an approved step-by-step implementation plan. Recheck current code and the [backlog constraints](README.md#execution-rules) before starting.

- [ ] Confirm task-specific decisions and write the bounded design in `docs/superpowers/specs/`.
- [ ] For code changes, use the writing-plans skill to create a test-first implementation plan in `docs/superpowers/plans/`.
- [ ] Implement only this task, or perform its authorized operational verification; record commands, outcomes, and unresolved failures.
- [ ] Review against every acceptance criterion and link the implementation plan or verification report below before marking complete.

[Back to team-readiness backlog](README.md)
