# Task 14 report: frontend workspace and task cutover

## Approach

I cut the UI over from the generated contract inward, keeping the existing task and settings components while replacing their data sources and mutation calls:

1. Added red-green tests for one-time token removal, generated auth/workspace reads, route-scoped workspace selection, member role protection, task record adapters, task filters and cursors, optimistic rollback/reconciliation, stale-version classification, attachment-only comments, and session mapping.
2. Added feature hooks that call only the generated SDK for setup, login, logout, recovery, workspaces, invitations, members, sessions, projects, statuses, tasks, comments, attachments, trash, and restore operations.
3. Added a workspace context whose selected ID comes only from the server-returned workspace list. The URL and local storage are navigation preferences, not authorization inputs.
4. Migrated the task list, board, detail, project settings, home task summary, command palette task search, topbar task breadcrumbs, members, sessions, workspace settings, and user logout to the real hooks.
5. Removed migrated task, project, status, session, and workspace records/actions from the mock store. The development mock badge remains because docs, mail, chat, and related shell counts still use mock data.
6. Added the isolated `just e2e` setup and a Playwright flow covering setup, logout/login, invitation-link copy, focused task creation, attachment upload, delete, trash, and restore.

## Behavior

- Setup and recovery tokens are read once and removed with `history.replaceState`, preserving other query parameters and the hash.
- Authentication failures stay on their forms. An expired authenticated session still uses the centralized 401 handler.
- Workspace switching preserves the current path and filters. Only IDs present in `GET /api/v1/workspaces` can be selected.
- Member management uses membership IDs and expected versions. Admins cannot manage owners, and nobody can manage their own membership from the row menu.
- Task lists pass filters, ordering, limits, and cursors to the generated list operation. “Load more” follows `next_cursor`.
- Task writes capture every cached task entry under the affected workspace. Failures restore the snapshot; successes reconcile the returned record/version and then refresh the affected workspace prefix.
- Bulk updates and board reorders use the generated atomic operations. Stale update and restore conflicts offer an explicit refresh.
- Comments support text, replies, files, and attachment-only submissions. Task attachments report per-file completion progress and refetch persisted metadata.
- Deleted projects and tasks are available at `/tasks-trash` and can be restored. Restore conflicts use the same refresh prompt.
- Remaining mock-backed product areas stay labeled in development; the badge is no longer hidden by the removed API-mode switch.

## Red-green evidence

- Token-consumption tests first received `null`; the implementation now returns the secret and replaces the URL without it.
- Workspace-selection tests first returned no workspace and no navigation URL; they now reject unavailable preferences and preserve route filters.
- Generated auth and workspace boundary tests first made no HTTP requests; they now observe `/api/v1/auth/me` and `/api/v1/workspaces`, including propagated server failures.
- Task adapter tests first failed with an unimplemented conversion; they now verify versions, identifiers, comments, attachment metadata, and authenticated download URLs.
- Optimistic cache tests first left list/detail data unchanged; they now verify workspace-isolated patch, rollback, and authoritative version reconciliation.
- Task paging tests first had no request URL; they now verify generated query serialization and cursor continuation.
- Member permission, stale conflict, attachment-only comment, status mapping, and session mapping tests each failed against their initial minimal implementation before passing.
- The first milestone run exposed the server’s rejection of an empty task title. Creation now persists `Untitled`, opens the detail route, and autofocuses the title field before editing.

## Verification

- `cd apps/web && bun run lint`: passed without warnings.
- `cd apps/web && bun run test`: 45 passed, 0 failed.
- `cd apps/web && bun run build`: passed. Vite emitted its existing large-chunk advisory.
- `PLAYWRIGHT_CHROMIUM_EXECUTABLE=/root/.cache/ms-playwright/chromium-1234/chrome-linux64/chrome just e2e`: 1 passed, 0 failed.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
- `cargo test --workspace --all-targets --all-features`: passed across all Rust targets.
- `just api-check`: passed with no generated-contract drift.
- `git diff --check`: passed.

Jean reported no configured or running environment for the base workspace. The isolated milestone run therefore used the checked-in `just e2e` command at `http://127.0.0.1:8888`, proxying the isolated backend on `127.0.0.1:18080`. Downloading Playwright’s newer Chromium timed out, so the successful run used the already cached Chromium executable shown above.

## Scope notes

- The server task contract has no due-date field, so the existing due-date control remains visible but disabled rather than writing non-persisted local state.
- Task records retain label IDs, while a workspace-scoped label query now joins them to persisted names and colors, including labels not used by any loaded task.
- Docs, mail, chat, profiles, notifications, custom roles, and their shell counts remain mock-backed and keep the development badge visible.

## Review follow-up

The ten important findings in `task-14-review.md` were resolved regression-first:

- The server now supports stable, cursor-safe priority sorting. Every advertised task sort has a regression test.
- Board drops reuse only the affected integer position slots and submit at most one generated bulk request. Operations above the 100-update atomic limit are rejected locally with guidance before any server request.
- Cross-project status filtering exhausts all task cursors before applying the merged status-group filter.
- Title and description drafts are controlled by an authoritative task key, so a confirmed conflict refresh visibly replaces rejected text.
- Multi-file task and comment uploads record the remaining queue, invalidate persisted attachment/comment reads after partial failure, and retry only unfinished files. Comment retries reuse the already-created comment, while changed drafts first clean up that partial comment. Progress and failures are announced in live regions.
- Workspace labels load through generated SDK calls across all cursor pages. Detail, list, board, and bulk controls join IDs to real label names and colors and offer unused labels.
- Ordinary invitations and role changes are limited to Admin and Member. Owners can instead use the generated ownership-transfer operation with workspace and membership versions after confirmation.
- Session responses now contain the server-identified current session. The frontend no longer relies on per-tab session storage, so new tabs and invitation-created sessions cannot accidentally present self-revocation controls.
- Migrated writes now expose disabled/pending states plus visible `role="alert"` failures and retry actions across workspace, member, invitation, session, project, status, task, comment, attachment, trash, and restore flows.
- Mounted hook/component coverage now exercises transfer payloads, current-session protection, partial upload resume, comment de-duplication, authoritative field refresh, labels, and owner-role protections. The Playwright milestone additionally covers real invitation acceptance, assigned role, second-workspace switching, new-tab session identity, labels, status/priority changes, text and attachment-only comments, conflict refresh, failed restore retry, and ownership transfer.

The follow-up regenerated the checked-in OpenAPI client for the `SessionRecord.current` contract field. Final verification remained green: 45 Bun tests, the production web build, all Rust targets/features under Clippy and tests, deterministic API generation, and the expanded Playwright milestone (`1 passed` in 56.9 seconds on the same `127.0.0.1:8888` / `127.0.0.1:18080` isolated environment). Jean again reported no configured run environment.

## Re-review follow-up, round 1

The four remaining important findings in `task-14-rereview-1.md` were fixed regression-first:

- Large board reindexes are partitioned into sequential generated bulk requests of at most 100 integer updates. Each request retains the server's transactional validation, and a partial failure records only the failed and unsent suffix for retry rather than replaying already-persisted chunks.
- Workspace creation, project creation, and task deletion now route both the initial write and its retry through the same completion function. Successful retries therefore clear and select the new workspace, select the new project, or return from the deleted task exactly like the original attempt.
- “Sign out all other sessions” is one aggregate mutation that revokes sessions sequentially. It continues after individual failures, reports the failed count and IDs, refreshes the session list after every outcome, and retries only failed sessions.
- Mounted coverage now drives a 102-item board reorder through the real hook and generated client, verifies the 100-item boundary, renders the failed-write alert, and retries successfully. Hook coverage separately verifies generated request bodies, optimistic list/detail cache changes, full rollback after rejection, and suffix-only recovery after a later batch fails.
- Mounted retry-flow coverage verifies the original workspace, project, and task success effects after a rejected first attempt. Session UI coverage verifies single-request concurrency, partial-failure visibility, continued processing, and failed-ID-only retry.

Final verification after this round:

- `cd apps/web && bun run lint && bun run test && bun run build`: passed; 53 tests, 153 assertions, and the existing Vite large-chunk advisory only.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
- `cargo test --workspace --all-targets --all-features`: passed across every Rust target.
- `just api-check` and `git diff --check`: passed with no contract drift or whitespace errors.
- `PLAYWRIGHT_CHROMIUM_EXECUTABLE=/root/.cache/ms-playwright/chromium-1234/chrome-linux64/chrome just e2e`: 1 passed in 54.0 seconds against `http://127.0.0.1:8888` and backend `127.0.0.1:18080`.

Jean reported no configured or running environment for the base workspace before the live check, so verification used the repository's isolated E2E command and ports above.

## Re-review follow-up, round 2

Round 2 supersedes the round-1 client batching strategy. A logical bulk edit or board reorder is never split across server transactions:

- Board moves now reuse the destination column's existing ordered integer position slots and update only the cards whose slots change. In a large column, moves near the destination edge therefore remain a small atomic request instead of reindexing every card.
- `useBulkTasks` sends exactly one generated bulk request when an operation contains at most 100 updates. Larger operations raise a typed local limit error before optimistic cache changes or HTTP, so no prefix can commit and no client rollback can disagree with the server.
- Board and bulk-selection surfaces explain how many tasks the rejected operation would affect and direct the user to drop within 100 affected cards or select 100 or fewer tasks. They do not offer a retry that must fail again.
- Retriable server failures preserve and resend the original valid atomic payload unchanged. Both `TaskBoard` and `BulkBar` use this shared retry behavior.
- Regression coverage proves that 101-item bulk edits and 102-item board moves issue zero requests and leave cached data unchanged. It also covers bounded large-column moves, in-column position-slot reuse, BulkBar's visible limit state, and exact-payload retry for valid failed requests.

Final verification after this round:

- `cd apps/web && bun run lint && bun run test && bun run build`: passed; 57 tests, 162 assertions, and the existing Vite large-chunk advisory only.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
- `cargo test --workspace --all-targets --all-features`: passed across every Rust target.
- `just api-check` and `git diff --check`: passed with no contract drift or whitespace errors.
- `PLAYWRIGHT_CHROMIUM_EXECUTABLE=/root/.cache/ms-playwright/chromium-1234/chrome-linux64/chrome just e2e`: 1 passed in 54.1 seconds against `http://127.0.0.1:8888` and backend `127.0.0.1:18080`.

Jean again reported no configured or running environment for the base workspace before live verification.
