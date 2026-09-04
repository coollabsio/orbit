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
