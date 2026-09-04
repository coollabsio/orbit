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

## Concerns

- Recovery request creates and hashes a token, but delivery still needs the planned SMTP job or installation-administrator copy flow. The public response cannot return the token without revealing whether the address exists.
- `CookieMode::LoopbackDevelopment` is explicit, but listener configuration must enforce loopback binding when the server startup path begins constructing this state.
