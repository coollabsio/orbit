# Task 11 report: projects, statuses, labels, tasks, comments, and trash APIs

## Approach

I implemented the task slice from the API contract inward with focused red-green cycles:

1. Added an integration test target describing project/default-status creation, Member task-area mutations, task list filters/sorts/cursors, optimistic conflicts, assignee validity, atomic bulk/reorder behavior, soft-delete visibility, trash/restore, comments, and cross-workspace rejection.
2. Added the next additive schema migration and embedded it in the platform migration catalog without editing migrations 1 through 6.
3. Added explicit-SQL repository operations with immediate transactions for mutations, audit writes in the mutation transaction, database-backed scope validation, and version compare-and-write behavior.
4. Added strict Axum DTOs and a focused router for all task-area resources. Collection responses use the shared `items`/`next_cursor` shape with opaque, versioned, query-bound cursors.
5. Extended the existing durable retention job to purge expired standalone tasks/projects in dependency order and queue deletion of newly unreferenced attachment blobs.
6. Ran focused and full verification and performed a final contract/diff review.

The brief's stale filename `0004_tasks.sql` could not be used because migrations 1 through 6 are already released. The implementation deliberately uses `0007_tasks.sql` and keeps all prior migration bytes unchanged.

## Files

Created:

- `apps/server/migrations/0007_tasks.sql`
- `apps/server/src/repositories/tasks.rs`
- `apps/server/src/task_routes.rs`
- `apps/server/tests/tasks_api.rs`

Updated:

- `apps/server/src/lib.rs`: exports the focused task router.
- `apps/server/src/repositories/mod.rs`: exports the task repository.
- `apps/server/src/repositories/workspaces.rs`: extends the existing durable retention transaction to purge expired standalone projects/tasks and their attachment references/blobs.
- `crates/platform/src/db/migrate.rs`: embeds additive migration 7.
- `crates/platform/src/backup.rs`: accepts the new current schema version.
- `crates/platform/tests/backup.rs` and `crates/platform/tests/database.rs`: update schema-version expectations to 7.

## Schema and integrity

- Added workspace labels, tasks, task assignees, task labels, and threaded task comments.
- Added task/project/status/list indexes for the scoped queries and stable orderings used by the API.
- Added `projects.restore_project_key`, allowing a deleted project's unique key to be reused while retaining the exact key needed for conflict-aware restore. Deletion replaces the live unique key with an unreachable tombstone inside the same transaction.
- Task assignees retain both the user and membership identity. The membership foreign key cascades assignment removal when a membership is removed.
- Database triggers reject cross-workspace status/project, task/project/status, assignee/membership, task/label, and comment/task/parent relationships, including direct inserts and scope-changing updates.
- Existing migration files `0001` through `0006` were not modified.

## API behavior

The focused router provides authenticated workspace-scoped endpoints for:

- Projects: list/create/update/soft-delete, trash list, and restore.
- Project statuses: list/create/update/delete and transactional reorder.
- Labels: list/create/update/delete.
- Tasks: filtered/sorted list, create/detail/update/soft-delete, trash list/restore, atomic bulk update, and transactional reorder.
- Comments: list/create/update/hard-delete, including task-scoped replies.

All content operations require a current membership in the workspace from the URL. Owner, Admin, and Member memberships receive the domain's task-area CRUD capabilities. Inaccessible existing IDs and unknown IDs use the same not-found response. Assignment accepts only unique users with a current membership and non-suspended account in that workspace; membership removal cascades existing assignments.

Mutation DTOs reject unknown fields. Names, keys, colors, categories, priorities, IDs, and bounded content fields are validated before repository entry. Updates and deletes require an observed version. Every successful mutation increments its record version and records a scoped audit event before the transaction commits.

Stale writes return `409 conflict` with `conflict.current_version`, the current safe serialized record, and a refresh path. Project restore key collisions return `409 restore_conflict` with `conflict.field = "key"` and do not rename either record.

## Pagination and ordering

- Every task-area collection accepts an opaque cursor, defaults to 50 items, caps pages at 100, and returns `next_cursor: null` on the last page.
- Cursors carry a format version, the effective workspace/filter/sort fingerprint, the last sort value, and the UUIDv7 tie-breaker. Malformed cursors and reuse with a different workspace, filters, or sort return `400 invalid_cursor`.
- Task lists support project, status, assignee, label, priority, and text filters plus position, title, creation-time, and update-time sorts in either direction.
- Task keyset continuation executes in SQL with the effective sort column and UUIDv7 tie-breaker rather than loading the full task collection.
- Status/task reorder and task bulk update run under one immediate transaction. A missing, foreign, or stale item rolls back every earlier item in that request.

## Deletion and retention

- Project and task deletes are soft deletes. Normal task reads require both an active task and an active project, so deleting a project hides its descendants without rewriting each child.
- Project/task trash is workspace-scoped and includes only records still inside the 30-day recovery window.
- Restore rejects expired records. Restoring a project reveals children that were not separately deleted, while separately deleted tasks remain deleted.
- The existing durable `workspace.retention` worker now removes expired standalone tasks first, then expired projects. It deletes related attachment references, deletes unreferenced blob metadata, and records file deletion work in the durable file-deletion table before committing.
- Comment deletion is a hard delete performed only after current task/workspace authorization and version validation.

## Red-green evidence

1. The initial `tasks_api` target failed to compile because `orbit_server::task_routes` did not exist. The additive migration, repository, and focused router made the initial nine API contracts green.
2. A descending-title cursor regression using `AA` followed by `A` failed against the first cursor-key encoding (`items[0]` was absent on page two). The corrected continuation encoding/SQL keyset path made the exact regression green.
3. The durable-retention regression initially retained both an expired project and its child task. Extending the existing queued retention transaction made the exact regression green and retained successful workspace-level cascade coverage.
4. The first full workspace run failed backup restore with `UnsupportedSchema { found: 7, maximum_supported: 6 }`. Updating the single supported-schema boundary and its expectations made the existing backup regression green.
5. The next full run exposed the database migration test's old `binary_version: 6` expectation. Updating it to the newly embedded version 7 made the exact regression green.

## Verification

- `cargo test -p orbit-server --test tasks_api`: 14 passed, 0 failed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.
- `cargo test --workspace`: passed across all domain, platform, server, integration, and doc-test targets.

## Commit

Commit subject: `feat(tasks): add persistent task APIs`

Base commit: `af8dff8`

## Concerns

- `task_router` is intentionally a focused composition unit. The plan reserves final production router composition for Task 15, so this task does not edit the shared CLI/server assembly point.
- Task/comment attachment upload, metadata, download, and attachment-only comment behavior remain Task 12. Retention already handles attachment-reference/blob cleanup for task/project purges so that later slice has a safe lifecycle boundary.
- OpenAPI contract generation and drift checks remain Task 13. Runtime DTOs and Problem responses are implemented here without adding parallel handwritten contract artifacts.

## Review remediation

The independent Task 11 review identified seven Important gaps. I reproduced each at the database or HTTP boundary before changing behavior, then fixed the underlying contracts without modifying migrations `0001` through `0007`:

- Added additive migration `0008_task_integrity.sql`. Scope-bearing parent identities are immutable where reassignment is not a supported operation, so project, status, task-workspace, label, membership, and comment-scope updates cannot invalidate existing joins. A suspension-side trigger removes assignments when an account becomes suspended.
- Task reads and assignee filters independently join current memberships and non-suspended users, so pre-migration assignments for already-suspended accounts stay invisible without a destructive migration backfill.
- Status PATCH models an omitted description explicitly and retains the current stored source rather than replacing it with an empty string.
- Names, titles, descriptions, comments, and task search text now have explicit character and byte checks. Descriptions remain byte-for-byte unchanged, while comment validation checks non-emptiness without trimming the stored body.
- Every collection validates a two-component typed cursor key, performs its `(sort_value, id)` continuation in SQL, and fetches at most `limit + 1` rows. The task list keeps its filter/sort-bound fingerprint and now filters assignees through current eligibility joins.
- Task bulk updates and task/status reorders reject duplicate target IDs before opening a mutation transaction.
- Version conflicts derive a readable, resource-specific task detail, trash, or containing collection URL from the stale record. Bulk and reorder responses therefore identify the exact stale record in `conflict.current` and provide a readable refresh location rather than the mutation endpoint.

Five high-risk integration regressions cover parent-side database corruption attempts, suspension cleanup/read/filter behavior, omitted status descriptions and source preservation, malformed generic cursor schemas, duplicate batches, and stale bulk/reorder refresh links. The focused Task 11 suite now contains 19 tests.

### Remediation red-green evidence

1. Parent-side project scope mutation succeeded before migration 8; the direct-SQL regression now confirms all covered parent scope changes abort.
2. Suspending an assigned user left one assignment row; suspension cleanup plus defensive joins made storage, detail reads, and assignee filtering return no assignment.
3. An omitted status PATCH description returned an empty string, comment whitespace was normalized, and multibyte titles exceeded the intended byte boundary; the source/limit regression now passes unchanged source bytes and rejects excessive bytes.
4. Correct-fingerprint cursors with empty, extra, or non-numeric keys returned `200`; strict cursor decoding and SQL keysets now return `400 invalid_cursor`.
5. Duplicate task bulk input returned `200`; duplicate task/status reorder inputs had the same double-mutation risk. All now return `422 validation_failed`. A stale bulk conflict previously refreshed `/tasks/bulk`; it and stale reorder now refresh the stale task detail URL successfully.
6. The first remediation workspace run exposed the database test's old `binary_version: 7` expectation after migration 8. Updating the schema-version expectation made the exact platform regression green.

### Remediation verification

- `cargo test -p orbit-server --test tasks_api`: 19 passed, 0 failed.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed after updating the schema-version expectation to 8.
- `git diff --check`: passed.

## Re-review remediation: status description nullability

The first re-review found that Serde's ordinary `Option<String>` represented both an omitted status description and explicit JSON `null` as `None`. The repository therefore preserved the description but still incremented the status version and recorded `status.updated` for a value the API did not honor.

`StatusDescriptionPatch` now represents the three input states separately. Omission preserves the current description. A string replaces it, including the empty string used to clear it. Explicit `null` is invalid because status descriptions are non-nullable in create responses, stored records, and the database schema.

The new regression first observed `200 OK` for a null description. It now expects `422 validation_failed`, reloads the status to prove its name, description, and version remain unchanged, and verifies that no `status.updated` audit event exists for the rejected request. The existing omission regression still passes, and it now also proves that an explicit empty string clears the description. The focused Task 11 suite contains 20 tests.
