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
- `cd apps/web && bun run test`: 30 passed, 0 failed.
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
- The server returns task label IDs but no expanded label names on task records; existing label pills therefore represent persisted IDs until the contract supplies expansion or a label-name join is added.
- Docs, mail, chat, profiles, notifications, custom roles, and their shell counts remain mock-backed and keep the development badge visible.
