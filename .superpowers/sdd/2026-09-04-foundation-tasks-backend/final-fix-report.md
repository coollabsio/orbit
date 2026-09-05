# Final review fix report

## Result

All four Important findings and the related Minor in `final-review.md` are closed in one change:

- Login now carries the observed password hash into the session transaction. Session creation rechecks both that hash and the account's active state before rehashing or inserting, so recovery or suspension that commits after password verification wins the race.
- The no-SMTP public recovery request is a generic `202` no-op. It does not look up an account, create a token, replace an administrator-issued token, or enqueue an unreachable URL. The recovery page tells users to contact their installation administrator; token-bearing administrator links still open the completion form.
- A shared cursor-page loader exhausts members, invitations, projects, statuses, project trash, comments, task attachments, comment attachments, and task trash. Its regression follows a non-null opaque first-page cursor.
- `AttachmentRepository` now owns attachment authorization, comment validation, cursor handling, list SQL, delete/quarantine transactions and audits, and authorized download lookup. `attachment_routes.rs` contains no SQL or transaction calls, enforced by a source-boundary regression alongside the existing attachment API suite.
- Concurrent recovery completion now maps the losing consumed or expired-token path to the documented `400 invalid_recovery_token` Problem response.

The generated OpenAPI and TypeScript client contract did not change. `just api-check` passed without generated-file drift.

## Regression evidence

The focused red-green coverage includes:

- recovery committing between credential observation and session creation leaves no live session and cannot be overwritten by an old-password rehash;
- suspension between password verification and session creation rejects login;
- a public recovery request preserves a previously issued administrator token and returns administrator guidance;
- two simultaneous recovery completions produce one `204` and one documented `400`;
- the collection page helper requests the opaque continuation cursor and combines both pages;
- attachment handlers fail the structural test if `sqlx::` or `immediate_transaction` returns to the HTTP module.

A pre-existing test fixture lifetime bug surfaced under the full parallel suite: one auth test discarded its `TestDatabase` owner while retaining pooled repository handles, which intermittently removed the SQLite file. Keeping the fixture alive for the test removed the nondeterminism; the atomic logout regression passed repeatedly and in the full gate.

## Verification

- `just check`: passed on the final source.
  - 288 Rust tests passed.
  - 61 Bun tests passed with 185 assertions.
  - formatting, warning-denied Clippy, lint, deterministic API generation, checked-in web assets, and the release workspace build passed.
  - Vite emitted its existing large-chunk advisory.
- `PLAYWRIGHT_CHROMIUM_EXECUTABLE=/root/.cache/ms-playwright/chromium-1234/chrome-linux64/chrome just e2e`: 1 passed in about 1.3 minutes.
  - Web URL: `http://127.0.0.1:8888`.
  - API listener: `127.0.0.1:18080`.
- `just release-image orbit:final-review-fix`: passed, including `docker run --rm orbit:final-review-fix --help`.
- `git diff --check`: passed.

Jean reported no Orbit run environment before the live test. The first unprefixed E2E attempt could not find the lockfile-selected Chromium v1243, and downloading it timed out. The successful run used the already cached Chromium v1234 executable shown above.
