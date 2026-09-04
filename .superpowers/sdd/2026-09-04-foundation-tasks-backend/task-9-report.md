# Task 9 report: authentication, setup, sessions, and recovery

## Approach

I split the work at the platform and HTTP/database boundary.

1. I wrote platform tests for passwords, opaque tokens, session expiry, activity write throttling, and login throttling.
2. I added the smallest platform implementations that made those tests pass.
3. I wrote SQLite repository tests for setup concurrency, email storage, and token hashes, then added migration 0002 and the repository.
4. I wrote route tests for cookie policy, generic responses, origin rejection, revocation, and durable sessions. I then added the Axum router and RFC 9457 error mapping.
5. I reran focused and workspace checks and reviewed the staged diff before committing.

## Files

Created:

- `crates/platform/src/auth/mod.rs`
- `crates/platform/src/auth/password.rs`
- `crates/platform/src/auth/session.rs`
- `crates/platform/src/auth/throttle.rs`
- `crates/platform/src/auth/token.rs`
- `crates/platform/tests/auth.rs`
- `apps/server/src/auth_routes.rs`
- `apps/server/src/repositories/mod.rs`
- `apps/server/src/repositories/identity.rs`
- `apps/server/migrations/0002_identity.sql`

Updated:

- `crates/platform/src/lib.rs` exports the authentication interfaces.
- `crates/platform/src/db/migrate.rs` embeds migration 0002.
- `crates/platform/src/db/mod.rs` exposes transactions and the pool to the server repository.
- `crates/platform/src/backup.rs` accepts schema version 2.
- `crates/platform/tests/backup.rs` and `crates/platform/tests/database.rs` expect schema version 2.
- `crates/platform/Cargo.toml`, `apps/server/Cargo.toml`, and `Cargo.lock` add the hashing, randomness, Unicode, HTTP, SQL, and OpenAPI dependencies.
- `apps/server/src/main.rs` exposes the route and repository modules.

## Behavior delivered

- Passwords accept 12 through 128 grapheme clusters, preserve exact Unicode input, reject the bundled common-password set, and use Argon2id PHC records. Successful verification returns a replacement hash when the configured Argon2 parameters have changed.
- Setup and recovery tokens contain 256 random bits. SQLite and the in-memory one-time token service retain SHA-256 hashes only. Comparisons do not short-circuit within a digest. Valid tokens are single-use.
- The setup repository uses `BEGIN IMMEDIATE`. It consumes the setup token and creates the first user, workspace, owner membership, default project, and five default statuses in one transaction through `WorkspaceDefaults`. The persistent `installation_state` flag prevents setup from reopening.
- Users keep a trimmed display email and a separately lowercased lookup email. No provider-specific rewriting occurs.
- Sessions persist in SQLite as token hashes. They have a 30-day idle lifetime and a 90-day absolute lifetime. Activity writes occur at most once every five minutes. Logout, recovery, and session deletion revoke records server-side.
- Failed-login throttling tracks normalized email and effective client IP. Email delays start after the fifth failure at 5 seconds, double, and cap at 15 minutes. An IP permits 50 failures in a rolling 15-minute window. Throttled responses include `Retry-After`.
- The router declares setup, login, logout, current-user, recovery, list-session, and revoke-session endpoints. Utoipa annotations document each route.
- Production cookies use `__Host-orbit_session`, `Secure`, `HttpOnly`, `SameSite=Lax`, and `Path=/`, with no `Domain`. Explicit loopback development mode uses `orbit_session_dev` without `Secure`.
- Authentication failures use the same `invalid_credentials` problem for unknown accounts and bad passwords. Recovery requests return the same accepted response for known and unknown addresses. Route failures use RFC 9457-shaped bodies and stable D-043 codes.

## Red and green evidence

1. `cargo test -p orbit-platform --test auth` first failed to compile because all requested auth exports were absent. After the first implementation, eight tests passed and the exact progressive throttle test failed because email history expired at the 15-minute boundary. Keeping capped backoff state through that boundary made all nine initial tests pass.
2. `cargo test -p orbit-server auth` first failed to compile because `IdentityRepository`, `SetupRequest`, and `SetupError` did not exist. Repository implementation made the setup race and normalized-email tests pass. The token-hash fixture then exposed its invalid foreign key; creating the user through setup fixed the fixture and all three repository tests passed.
3. Route tests first failed to compile because `AuthState`, `CookieMode`, and `auth_router` did not exist. After implementation, seven route/repository tests passed. The generic-error comparison failed only on per-request `request_id`; comparing the stable problem fields made all eight pass.
4. A token-kind regression failed because a mismatched endpoint removed a valid token. Checking kind and expiry before removal fixed it. Two later redaction assertions failed because derived `Debug` output contained session and one-time tokens; explicit redacted implementations fixed both.
5. The setup-cookie regression returned no `Set-Cookie`. Creating the initial session in the setup handler made it pass.
6. The durable-session regression returned `401` after rebuilding HTTP state. Moving route sessions into the SQLite repository made the rebuilt state authenticate the same cookie.

## Verification

- `cargo test -p orbit-platform --test auth`: 10 passed, 0 failed.
- `cargo test -p orbit-server auth`: 10 passed, 0 failed.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy -p orbit-platform --test auth -- -D warnings`: passed.
- `cargo clippy -p orbit-server --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed, including 14 server unit tests and every platform/domain integration suite.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.

## Commit

Commit subject: `feat(auth): add setup and browser sessions`

## Concerns (initial implementation; superseded by the remediation below)

- Recovery request creates and hashes a token, but delivery still needs the planned SMTP job or installation-administrator copy flow. The public response cannot return the token without revealing whether the address exists.
- `CookieMode::LoopbackDevelopment` is explicit, but listener configuration must enforce loopback binding when the server startup path begins constructing this state.

---

## Important-finding remediation

### Approach

I converted each Important review finding into a regression at the narrowest responsible boundary, observed the regression fail, and then changed the production contract. Repository mutations that must succeed together now share immediate SQLite transactions; request-admission and session-touch races use atomic reservation or conditional-write protocols; and HTTP/startup behavior is validated at construction and extraction boundaries.

### Files changed

- `apps/server/src/auth_routes.rs`: adds an injectable recovery-delivery boundary with a bounded administrator-copy implementation, mapped JSON extraction, atomic repository calls, bounded asynchronous password work, logout error propagation, validated cookie modes, and first-start setup initialization.
- `apps/server/src/repositories/identity.rs`: makes setup plus its initial session atomic, makes recovery consumption/password replacement/session revocation atomic, conditionally touches activity, manages setup-token initialization and rotation, bounds recovery-token rows, and queries session hashes directly.
- `apps/server/src/cli.rs`: adds `setup-token init` and `setup-token rotate`, which print browser setup URLs without persisting plaintext secrets.
- `crates/platform/src/auth/password.rs` and `common-passwords-2026-09-04.v1.txt`: add the named/versioned local common-password data set and a semaphore-bounded `spawn_blocking` password executor.
- `crates/platform/src/auth/throttle.rs`: adds atomic login reservations, completion, and expiry of abandoned reservations.
- `crates/platform/src/http/security.rs`: requires an allowed Origin for every unsafe method.
- Auth and HTTP tests, public exports, and the server's `tracing` dependency were updated for these contracts.

### Result

- Recovery requests create one durable hashed token per user and deliver a browser URL through `RecoveryDelivery`. `AdminRecoveryDelivery` provides a bounded administrator-copy queue; an SMTP implementation can use the same boundary without changing the route or public generic response.
- Recovery completion performs token cleanup and consumption, password replacement, and active-session revocation in one `BEGIN IMMEDIATE` transaction. Failure rolls back every mutation.
- Logout clears its cookie only after successful durable revocation.
- Concurrent login work is admitted through reservations, including in-flight attempts; abandoned reservations expire after the rolling window.
- Persistent activity refresh uses a compare-and-set on the previously read timestamp, so concurrent requests perform at most one write per interval.
- Unsafe requests without Origin are rejected by the shared platform layer. Malformed, missing-field, unknown-field, and unsupported JSON requests are mapped to `application/problem+json` with the stable `invalid_request` code and request correlation.
- Setup creates its initial session in the same transaction as the installation data and setup-token consumption.
- Common-password screening uses the local `2026-09-04.v1` data file rather than an inline fixture.
- Insecure cookie mode can only be constructed with an explicit loopback `SocketAddr` and emits a visible warning. Secure mode remains the default constructor path.
- First-run setup-token initialization persists only the hash and returns the plaintext URL once. The operations CLI supports explicit unused-token rotation.
- Argon2 hashing, verification, and rehashing run in `spawn_blocking` behind a two-permit request-state semaphore.
- Recovery no longer creates unused in-memory token-store records. Expired durable records are purged during writes/completion, and replacement limits storage to one live recovery token per user.
- Session authentication now uses the unique token-hash lookup rather than scanning all active sessions.

### Red and green evidence

1. The new platform tests initially failed to compile because `PasswordExecutor`, the data-set version export, and throttle reservations did not exist. The missing-Origin regression then returned 204 instead of the required 403. After implementation, `cargo test -p orbit-platform --test auth` passed 14 tests and `cargo test -p orbit-platform --test http_security` passed 16 tests.
2. Repository and route regressions initially failed to compile because setup initialization/rotation, atomic recovery, validated cookie constructors, delivery injection, and custom extraction did not exist. After the first implementation, the server auth suite passed 20 tests.
3. The durable-token bound regression then failed with two recovery rows instead of one. Deleting expired rows and the user's prior token in the same immediate write transaction made it pass.
4. The abandoned-reservation regression failed because five cancelled/incomplete reservations blocked an email forever. Timestamping and expiring reservations with the 15-minute window made it pass.
5. Parallel server auth execution exposed that the test helper dropped its SQLite temporary-directory guard while the router still held pooled connections. Retaining the guard for each application test removed that nondeterminism; the complete server auth suite now passes 21 tests and the CLI suite passes 5 tests.

### Verification

- `cargo test -p orbit-platform --test auth`: 14 passed, 0 failed.
- `cargo test -p orbit-platform --test http_security`: 16 passed, 0 failed.
- `cargo test -p orbit-server auth`: 21 passed, 0 failed.
- `cargo test -p orbit-server cli`: 5 passed, 0 failed.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy -p orbit-platform --test auth -- -D warnings`: passed.
- `cargo clippy -p orbit-server --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed, including Task 7's 24 attachment tests and all 26 server tests.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.

### Concerns

- The administrator-copy delivery is deliberately bounded and in-process. Production composition must retain the delivery handle for an operator UI or provide an SMTP-backed `RecoveryDelivery`; the authentication route already exposes the required injection point and never returns the secret publicly.
- Setup-token URLs and recovery URLs contain bearer secrets and therefore must only be shown through trusted operator channels and redacted from ordinary request logs.

---

## Re-review round 1 remediation

### Approach and result

- Added a regression that builds five failures, waits through the five-second backoff, races the next admission, and verifies exactly one probe is accepted. A failed probe advances the policy to ten seconds, after which the next probe is admitted. The reservation capacity now permits one in-flight probe once progressive throttling has started while still limiting the initial wave to five total attempts.
- Added a regression that formats setup and recovery delivery values with `Debug` and verifies the bearer token is absent. `SetupLaunch` and `RecoveryMessage` now provide explicit redacted `Debug` implementations.
- Removed the full setup URL from tracing. `initialize_auth` returns the secret-bearing launch value to its caller, and the operations CLI remains the deliberate console-output boundary.

### Red and green evidence

- `throttle_admits_one_probe_after_backoff_and_advances_progressive_delay` initially failed because the first post-backoff reservation returned a one-second retry. It passes after separating the one-probe capacity from the initial five-attempt capacity.
- `auth_secret_bearing_urls_are_redacted_from_debug_output` initially failed because derived output contained the full URL and no redaction marker. It passes with manual redacted formatters.

### Verification

- `cargo test -p orbit-platform --test auth`: 15 passed, 0 failed.
- `cargo test -p orbit-server auth`: 22 passed, 0 failed.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy -p orbit-platform --test auth -- -D warnings`: passed.
- `cargo clippy -p orbit-server --all-targets -- -D warnings`: passed.

### Concerns

- No blocking concerns remain from the re-review. Bearer URLs still intentionally cross the explicit operator/SMTP delivery boundaries and must not be logged by their consumers.
