# Task 10 report: workspace membership, invitations, audit, and APIs

## Approach

I implemented the workspace slice from the HTTP boundary inward using focused red-green cycles:

1. Added an integration-test target and first described workspace listing, creation, scoped invitations, and filtered audit behavior.
2. Added the additive workspace migration, SQLx repository, audit writer/reader, and Axum routes needed to make that slice green.
3. Expanded the API tests around invitation identity matching, single use, replacement, manual-link provenance, idempotent membership acceptance, role protections, ownership transfer, suspension, trash/restore, and strict DTO failures. Each expansion exposed a concrete failure before its production fix.
4. Ran focused checks, the full workspace test suite, formatting, clippy, and a final diff review.

Migration `0003_attachment_references.sql` was already released at the task base. To preserve that checksum/version contract, the workspace migration is deliberately `0004_workspaces.sql`, rather than reusing the stale `0003_workspaces.sql` filename from the original plan.

## Files

Created:

- `apps/server/migrations/0004_workspaces.sql`
- `apps/server/src/audit.rs`
- `apps/server/src/lib.rs`
- `apps/server/src/repositories/workspaces.rs`
- `apps/server/src/workspace_routes.rs`
- `apps/server/tests/workspaces_api.rs`

Updated:

- `apps/server/src/auth_routes.rs`: shares the validated cookie name inside the server crate.
- `apps/server/src/cli.rs` and `apps/server/src/main.rs`: use the new server library target without compiling duplicate route/repository modules.
- `apps/server/src/repositories/identity.rs`: adds installation-admin-only global suspension with atomic session revocation and audit.
- `apps/server/src/repositories/mod.rs`: exports the workspace repository.
- `crates/platform/src/db/migrate.rs`: embeds additive migration 4 without changing migration 3.
- `crates/platform/src/backup.rs`: permits backups at the new current schema version.
- `crates/platform/tests/backup.rs` and `crates/platform/tests/database.rs`: assert schema version 4.

## Behavior delivered

- Authenticated users can list every active workspace membership and create additional workspaces with an owner membership atomically.
- Workspace reads and mutations require a membership in the workspace named by the path. Existing inaccessible resources and unknown resources both return the same not-found problem.
- Owners and Admins can update settings, list cursor-paginated members/invitations, invite Admins or Members, revoke invitations, change non-owner roles, and remove non-owner memberships.
- The sole Owner is protected by both use-case checks and a partial unique database index. Ownership transfer atomically demotes the former Owner and promotes an in-workspace target before the former Owner can be removed.
- Invitation tokens contain cryptographic randomness, are stored only as SHA-256 hashes, expire after seven days, and are single use. Resending uses an immediate transaction to invalidate all prior pending invitations for the normalized email/workspace before inserting the replacement.
- Manual and SMTP delivery provenance is persisted. Manual responses return the one-time copy URL only to the authorized caller, listings omit tokens/URLs, and secret-bearing issued invitations redact their token from `Debug`. Manual acceptance does not verify email; SMTP provenance can mark it verified without changing the acceptance request.
- Signed-in acceptance rejects a different normalized email, creates membership and consumes the invitation in one transaction, and returns idempotent success if the membership already exists.
- Workspace deletion is soft, owner-only, appears in an owner trash list, and can be restored only by its Owner.
- Installation administrators can globally suspend a user; the mutation atomically revokes all sessions. Workspace administrators cannot suspend global accounts.
- Security mutations append scoped audit events with bounded metadata. Workspace audit reads require Owner/Admin membership and always filter by workspace. Writes purge entries outside the 365-day retention window.
- JSON and query DTO failures return correlated `application/problem+json` responses. Route functions carry Utoipa path annotations.

## Red-green evidence

1. The initial integration test failed because `orbit_server`, `WorkspaceState`, and `workspace_router` did not exist. Adding the library boundary then produced missing-module failures, and the migration/repository/routes made the first workspace slice pass.
2. The invitation acceptance test returned `500` instead of `200`. Root cause was using `fetch_one` for an optional existing membership. Switching to a scoped `fetch_optional` made new membership acceptance green.
3. The ownership test returned `500` during transfer. Root cause was an ambiguous `id` projection in a memberships/workspaces join. Qualifying the membership columns made transfer green.
4. The delete/restore test returned `404` for an active workspace deletion. Root cause was checking for the post-mutation deletion state rather than the current state. Checking the inverse current state made delete and restore green.
5. The owner-protection test showed an Admin received the Owner's transfer-required response. Applying the domain distinction (Owner must transfer; Admin is forbidden) made the policy regression green.
6. The global-suspension test initially reached no route (`404`). Adding the installation-admin mutation and atomic session revocation made the authorization and revocation assertions green.
7. The malformed-query regression returned Axum's plain-text rejection. A mapped query extractor made both query and JSON DTO errors use the strict problem-details contract.
8. The first full workspace run failed in backup restore with `UnsupportedSchema { found: 4, maximum_supported: 3 }`. The new migration correctly advanced the database schema, while backup validation still advertised version 3. Updating the single supported-schema constant and its expectations made that existing regression pass.

## Verification

- `cargo test -p orbit-server --test workspaces_api`: 4 passed, 0 failed.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy -p orbit-server --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed, including 22 server library tests, 5 server binary tests, the 4 workspace API tests, and all platform/domain suites.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `git diff --check`: passed.

## Commit

Commit subject: `feat(workspaces): add memberships and invitations`

Base commit: `5479074`

## Concerns

- SMTP transport is intentionally not wired in this slice. The repository returns the one-time token plus persisted `smtp` provenance to the authorized use-case boundary, while the HTTP response never exposes an SMTP invitation URL.
- Invitation acceptance currently covers authenticated existing global accounts, which is the identity-mismatch and idempotence contract exercised here. Creating a brand-new password account from an invitation will need a dedicated invite-registration flow that applies the existing password executor without weakening the signed-in existing-account rule.
- Audit retention is enforced opportunistically on security-event writes. A later durable maintenance job should also call retention cleanup so an entirely idle installation still purges on schedule.

---

## Important-review remediation

### Approach

I verified the nine Important findings against the implementation and treated each as a contract gap. Remediation proceeded regression-first at the API/database boundary: invitation registration and default workspace data, optimistic versions, owner integrity, trash/audit retention, explicit audit outcomes and operator access, then concurrency and lifecycle coverage. Released migrations 1 through 4 remain byte-for-byte unchanged; all integrity and retention schema changes are additive migration 5.

### Files changed

- `apps/server/migrations/0005_workspace_integrity.sql`: adds the owner-membership pointer, owner synchronization/protection triggers, and durable maintenance summaries.
- `apps/server/src/repositories/workspaces.rs`: adds invitation-bound account/session creation, default workspace project/status creation, expected-version checks, owner-pointer transfer, bounded trash restoration, durable retention jobs/worker/summary, explicit audit outcomes, failure events, and global audit reads.
- `apps/server/src/repositories/identity.rs`: makes installation-admin checks reusable and records failed suspension attempts with an explicit outcome.
- `apps/server/src/audit.rs`: accepts explicit success/failure outcomes, removes opportunistic retention, adds installation-wide filtered reads, and fails visibly rather than dropping malformed audit rows.
- `apps/server/src/workspace_routes.rs`: adds invite registration, session-cookie issuance, expected-version DTOs, conflict refresh metadata, global audit search/export, and strict query validation.
- `apps/server/src/auth_routes.rs`: exposes the existing secure session-cookie constructor inside the server crate so invitation registration uses the identical policy.
- `apps/server/tests/workspaces_api.rs`: expands focused coverage from 4 to 12 tests across the reviewed high-risk boundaries.
- `apps/server/Cargo.toml` and `Cargo.lock`: make the existing cancellation-token crate available to the server retention-worker integration test.
- `crates/platform/src/db/migrate.rs`, `crates/platform/src/backup.rs`, and their tests: embed and accept schema version 5 while preserving prior migration checksums.

### Result

- A recipient without a global account can submit the invitation token, matching email, display name, and password. Password hashing uses the bounded async executor. The transaction creates the global credential, membership, invitation consumption, security audit, and durable browser session together; any session-write failure rolls everything back. Existing global accounts still must sign in and match the normalized invited email.
- Ordinary workspace creation now uses `WorkspaceDefaults`, atomically creating the owner membership, General project, and Backlog, Todo, In Progress, Done, and Cancelled statuses.
- Rename, role change, membership removal, ownership transfer, delete, and restore require observed versions. SQL predicates reject stale versions, and `409 conflict` responses carry `conflict.current_version` plus a refresh URL. Rename retains the authorized role/current version inside its transaction and cannot commit before a later response-building query fails.
- Migration 5 gives each workspace a deferred owner-membership foreign key. Inserts require a pointer, owner changes validate in-workspace targets, triggers synchronize the role swap, direct role demotion/promotion is constrained, and deleting either the sole owner membership or its user is rejected while the workspace exists. Concurrent transfers serialize and leave one linked `owner` role.
- Trash listing and restore enforce the 30-day boundary. A `workspace.retention` job uses the existing durable job store/worker, purges expired workspaces in deletion/id order through database cascades, purges audits older than 365 days, and persists one bounded count summary rather than per-row audit noise.
- Audit writes require an explicit outcome. Invalid/mismatched invitation acceptance, forbidden membership changes, and failed suspension attempts now retain safe bounded failure events. Installation administrators can search filtered installation-wide audit history and export a CSV; workspace administrators remain limited to their active workspace scope.
- Audit decoding now reports corrupt identifiers/metadata as a safe repository failure instead of silently omitting history and corrupting cursors.
- Pending invitation revocation now excludes expired rows. Cursor DTOs reject unknown keys, and malformed cursors return `400 invalid_request` rather than a workspace non-enumeration response.

### Red-green evidence

1. New-recipient registration returned `400` because the strict token-only DTO rejected registration fields. The new route/repository path made the registration/session test green. A trigger-forced session insert failure then verified that account, membership, and token consumption all roll back.
2. The default-workspace regression found zero projects and statuses. Reusing `WorkspaceDefaults` and inserting the workflow in the workspace transaction made it pass.
3. The version regression returned `400` because rename did not accept an expected version. Required version DTOs, scoped compare-and-write predicates, and conflict metadata made fresh mutation/stale retry behavior green; membership-role and delete stale cases were added to the same focused test.
4. The owner/retention regression initially could not compile because no durable retention API existed. Migration 5, the job-store enqueue path, registered worker, ordered purge, and durable summary made it pass. The same regression proves direct owner membership deletion, owner-user cascade deletion, and direct owner-role demotion fail.
5. The global audit regression initially received `404` because no operator audit boundary existed. Installation-admin search/export plus explicit failure outcomes made it green for invitation mismatch and suspension denial while a non-admin remains forbidden.
6. Invitation lifecycle coverage verifies the exact seven-day lifetime, expiry boundary, SMTP verification provenance, secret-redacted debug output, and concurrent normalized-email resend with exactly one usable token.
7. Concurrent ownership transfers verify one winner and one database-linked `owner` after serialization. Pagination and foreign nested invitation IDs verify continuation and path scoping.

### Verification

- `cargo test -p orbit-server --test workspaces_api`: 12 passed, 0 failed.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy -p orbit-server --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed, including all 12 workspace API regressions and every existing domain/platform/server suite.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `git diff --check`: passed.

### Concerns

- SMTP transport remains a later composition task. The invitation repository preserves SMTP/manual provenance and hands the secret only to the authorized delivery boundary; acceptance semantics no longer depend on transport-specific DTOs.
- The retention worker is implemented and integration-tested through the durable job queue. Production startup still needs to schedule the recurring enqueue cadence when the serving-process composition is introduced.
- OpenAPI error-response enumeration remains a Minor review item; success paths retain their existing Utoipa annotations, while runtime errors follow the shared stable problem contract.

## Re-review round 1 remediation (2026-09-04)

### Approach

Verified each Important finding against the repository and platform job/file primitives, then added focused regressions before changing behavior. Kept migrations 1-5 byte-for-byte unchanged and introduced additive migration 6 for owner-insert enforcement and durable retention/security state.

### Files changed

- `apps/server/migrations/0006_workspace_retention_security.sql`: owner insert triggers, durable attachment deletion outbox, bounded invitation probe summaries, and attachment purge summary counts.
- `crates/platform/src/db/migrate.rs`, `crates/platform/src/backup.rs`, platform migration/backup tests: embedded schema v6 and backup compatibility.
- `apps/server/src/repositories/workspaces.rs`: pre-hash invitation preflight with transactional final revalidation, ordered attachment metadata purge and durable file deletion, recurring retention scheduler/worker service, denial audits, and probe summary retention.
- `apps/server/src/workspace_routes.rs`: separate invitation-registration throttle, preflight ordering, and complete cursor-walking CSV export.
- `apps/server/src/repositories/identity.rs`, `apps/server/src/auth_routes.rs`: durable setup, login/throttle, logout, recovery, and session-revocation outcome events.
- `apps/server/tests/workspaces_api.rs`: regressions for owner-pointer insert bypass, pre-Argon2 invitation rejection/throttling/summarization, attachment metadata/file purge, recurring retention, and exports beyond 100 rows.

### Red / green evidence

- Owner-pointer, invitation preflight, and multi-page export regressions initially failed respectively because the cross-workspace insert succeeded, invalid tokens reached password validation (`422`), and export returned only 17/125 rows.
- Attachment purge regression initially retained `attachment_references`.
- Recurring retention regression initially failed to compile because `run_retention_service` did not exist.
- Authentication audit regression initially found no `authentication.login` failure event.
- Each focused regression passed after its minimal implementation; invitation throttling was additionally exercised until the stable `429 invitation_registration_throttled` response.

### Verification

- `cargo test -p orbit-server`: passed (23 library, 5 binary, 16 workspace integration tests).
- `cargo test --workspace`: passed across all crates and doc tests.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed after formatting.
- `git diff --check`: passed.

### Commit

`fix(workspaces): close retention and security gaps` (final hash in agent handoff).

### Concerns

- The current binary still exposes operational CLI commands rather than an HTTP `serve` command. `run_production_retention_service` is the production lifecycle entry point for the future server composition and pairs the durable daily schedule with its worker; the configurable variant is covered end-to-end.
- `WorkspaceRepository::new` uses the documented default `attachments` root; deployments with a configured attachment root must construct it with `with_blob_store` when composing the server.
