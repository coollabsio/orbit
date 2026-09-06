# Team-readiness backlog

Created 2026-09-06 from the team-readiness assessment. Implementation started 2026-09-06; see [the design](../specs/2026-09-06-team-readiness-features-design.md) and [plan](../plans/2026-09-06-team-readiness-features.md).

Task persistence, roles, invitations, live updates, backups, and deployment packaging already exist. Feature tasks close product gaps; operational tasks verify/configure existing mechanisms on the selected host rather than rebuild them.

## Tasks

| Status | Task | Priority | Dependencies |
|---|---|---|---|
| [x] | [01. In-app task notifications and mentions](01-notifications-and-mentions.md) | High | Implemented: persisted assignment/mention notifications, inbox, `@` member picker |
| [~] | [02. My tasks, overdue, due-soon, and saved views](02-personal-task-views.md) | High | Views `mine`/`overdue`/`due_soon` (UTC) + sidebar. Saved views not yet |
| [x] | [03. Profile settings and authenticated password changes](03-account-self-service.md) | High | `/profile`, PATCH me, POST password, other sessions revoked |
| [ ] | [04. Automatic invitation and password-recovery email](04-invitation-and-recovery-email.md) | Medium | None |
| [ ] | [05. Task subtasks](05-subtasks.md) | Medium | None |
| [ ] | [06. Blocked-by and blocking task dependencies](06-task-dependencies.md) | Medium | None |
| [ ] | [07. Recurring tasks](07-recurring-tasks.md) | Medium | None |
| [ ] | [08. Task import and export](08-task-import-export.md) | Medium | None |
| [x] | [09. Remove unavailable products and mock results from search](09-command-palette-cleanup.md) | High | Mock Docs/Mail/Chat removed from palette |
| [~] | [10. Verify a team production deployment](10-production-deployment.md) | Before rollout | Script only: `deploy/verify-production.sh`. Needs operator host |
| [~] | [11. Off-host backups and a restore rehearsal](11-offsite-backup-restore.md) | Before rollout | Script only: `deploy/backup-rehearsal.sh`. Needs destination |
| [ ] | [12. Run the end-to-end multiuser team pilot](12-multiuser-pilot.md) | Before rollout | [Task 10](10-production-deployment.md) |
| [ ] | [13. Verify membership removal and session revocation](13-access-revocation-verification.md) | Before rollout | Existing API tests; production check still needs Task 10 |
| [~] | [14. Readiness monitoring and operational ownership](14-monitoring-and-operations-ownership.md) | Before rollout | Runbook template in `docs/operations.md`. Needs named owners |

## Suggested order and parallel work

- **Pilot preparation:** Task 09 is a small independent cleanup. Task 10 establishes the real deployment.
- **Rollout checks:** After Task 10, Tasks 11–14 can be scheduled independently. Coordinate test accounts, storage, and maintenance windows so checks do not disrupt one another.
- **First product improvements:** Prioritize Task 01, then Task 02. Task 03 can proceed separately with disjoint file ownership; coordinate auth changes with Task 04.
- **Later task features:** Tasks 05–08 are independent product choices, not requirements to start a small pilot. Although they have no hard dependencies, they share task schemas/routes and need coordinated migrations and generated-client changes.
- **Email:** Task 04 does not block in-app notifications, account password changes, or manually shared invitations.

## Execution rules

- Keep solutions small and reuse the current Rust/Axum/SQLx/SQLite backend, durable jobs, generated API client, React, and TanStack Query.
- Preserve workspace/recipient authorization, server-side validation, optimistic record versions, Problem Details, audit privacy, and production/development separation.
- Before implementing a feature, inspect the linked files and produce its own bounded design and test-first plan. Do not treat this backlog as a fully specified API or database design.
- Do not create worktrees unless explicitly requested; if requested, use Jean's worktree tools.
- For live verification, call Jean `get_run_environments` first and reuse the returned URL/ports/startup command. Record the exact URL used; do not start a competing server.
- Do not change production DNS, infrastructure, credentials, backup destinations, or real team access without the necessary operator choices and authorization.
- Run focused failing tests before code changes, then the relevant suites and review. For operations, record real evidence rather than marking a check complete from documentation alone.
- Keep tokens, passwords, SMTP credentials, and real backup data out of task documents and reports.
- Update the individual task and this index together on completion, with links to the resulting plan/report.

## Related work

- [Existing deployment and collaboration plan](../plans/2026-09-06-deployment-collaboration.md)
- [Deployment guide](../../../deploy/README.md)
- [Operations runbook](../../operations.md)
- [Milestone-one traceability](../../milestone-1-traceability.md)
