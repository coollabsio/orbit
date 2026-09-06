# Task 11: Off-host backups and a restore rehearsal

**Status:** Backlog, not started
**Priority:** Before rollout
**Type:** Operations
**Dependencies:** [Task 10](10-production-deployment.md)

## Outcome

Prove that team tasks and attachments can be recovered after losing the host.

## Scope

- Choose a protected off-host destination and retention policy with the operator.
- Copy verified existing backup snapshots off-host on a documented schedule.
- Restore a snapshot into separate stopped-instance storage and inspect the recovered data.
- Record recovery steps, observed restore time, and expected recovery-point window.

## Acceptance criteria

- [ ] A verified backup is recoverable without access to the original host.
- [ ] The restored instance contains expected tasks, comments, memberships, and downloadable attachments.
- [ ] Backup failure can be detected and the procedure identifies the responsible operator.
- [ ] The rehearsal never overwrites the active instance; backup credentials and data remain access-controlled.

## Starting points

- [deploy/README.md](../../../deploy/README.md)
- [docs/operations.md](../../../docs/operations.md)
- [.ai/DEVELOPMENT.md](../../../.ai/DEVELOPMENT.md)
- [crates/platform/src/backup.rs](../../../crates/platform/src/backup.rs)

## Limits

Reuse the backup/restore implementation. This is not a new backup format or automatic repair system.

## Execution handoff

This is a backlog task, not an approved step-by-step implementation plan. Recheck current code and the [backlog constraints](README.md#execution-rules) before starting.

- [ ] Confirm task-specific decisions and write the bounded design in `docs/superpowers/specs/`.
- [ ] For code changes, use the writing-plans skill to create a test-first implementation plan in `docs/superpowers/plans/`.
- [ ] Implement only this task, or perform its authorized operational verification; record commands, outcomes, and unresolved failures.
- [ ] Review against every acceptance criterion and link the implementation plan or verification report below before marking complete.

[Back to team-readiness backlog](README.md)
