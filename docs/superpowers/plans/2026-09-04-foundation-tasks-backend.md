# Orbit Foundation and Tasks Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the mock-backed identity, workspace, project, task, comment, and attachment domains with a persistent Rust/SQLite backend while keeping all other mock features usable.

**Architecture:** Use a Cargo workspace with `platform`, `orbit`, and `server` crates. `orbit` contains domain types, use cases, and ports; `platform` contains reusable config, HTTP, database, jobs, files, auth, and observability modules; `server` wires Axum, SQLx, SQLite, local storage, and the embedded React app. The React frontend uses a generated OpenAPI client behind TanStack Query hooks.

**Tech Stack:** Rust 2024, Tokio, Axum, Tower, SQLx/SQLite, Serde, UUIDv7, Argon2id, tracing, utoipa, Bun, React 19, TanStack Query, Vitest, Testing Library, Playwright, Just.

## Global constraints

- The source of truth is `docs/superpowers/specs/2026-09-03-backend-platform-design.md`; preserve all D-001 through D-069 decisions.
- Milestone one includes foundation, workspaces, tasks, comments, and task attachments. Inbound SMTP, mailbox synchronization, docs, chat, DMs, and inbox persistence are separate plans.
- One running application instance owns one SQLite database; enable WAL, foreign keys, and a busy timeout; do not support network filesystems.
- Every workspace-owned row, query, job, event, file, cache key, and API path carries workspace context.
- Use UUIDv7 IDs, UTC Unix milliseconds in SQLite, RFC 3339 UTC strings in JSON, strict DTOs, Problem Details, and integer record versions.
- Write a failing test first, verify the expected failure, add the minimum implementation, rerun the focused test, then run the owning crate/package suite.
- Each task gets its own commit. Do not combine independent task commits or edit files owned by another active parallel task.
- Run `just check` before milestone completion.

## Parallel execution map

- **Wave 0:** Task 1 only.
- **Wave 1, parallel:** Tasks 2, 3, and 4. They consume only Task 1 manifests and the interfaces written below.
- **Wave 2, parallel:** Tasks 5, 6, 7, and 8 after Tasks 2-4 merge.
- **Wave 3, parallel:** Tasks 9, 10, 11, and 12 after their stated dependencies merge.
- **Wave 4, parallel:** Tasks 13 and 14.
- **Wave 5:** Task 15 integration and release gate.

Agents must own disjoint paths. When two tasks mention the same composition file, the earlier task creates a focused module and Task 15 performs final wiring.

---

### Task 1: Workspace, Bun, and Just foundation

**Files:**
- Create: `Cargo.toml`
- Create: `crates/platform/Cargo.toml`, `crates/platform/src/lib.rs`
- Create: `crates/orbit/Cargo.toml`, `crates/orbit/src/lib.rs`
- Modify: `apps/server/Cargo.toml`, `apps/server/src/main.rs`
- Create: `justfile`, `rust-toolchain.toml`
- Create: `apps/web/bun.lock`
- Modify: `apps/web/package.json`, `.ai/DEVELOPMENT.md`, `.gitignore`
- Delete: `apps/web/aube-lock.yaml`

**Interfaces:**
- Produces Cargo packages `orbit-platform`, `orbit-domain`, and binary package `orbit-server` with binary name `orbit`.
- Produces Just recipes `setup`, `dev`, `test`, `check`, `api`, `api-check`, `db-reset`, `seed`, `e2e`, and `build`.

- [ ] **Step 1: Add a failing workspace smoke check**

```bash
cargo metadata --no-deps --format-version 1 | jq -e '
  [.packages[].name] | sort == ["orbit-domain", "orbit-platform", "orbit-server"]
'
```

Expected: FAIL because no root workspace exists.

- [ ] **Step 2: Create the three-package workspace**

Use resolver 3 and shared edition/version metadata:

```toml
[workspace]
members = ["apps/server", "crates/orbit", "crates/platform"]
resolver = "3"

[workspace.package]
edition = "2024"
version = "0.1.0"
license = "Apache-2.0"
```

Make `orbit-domain` depend only on small platform interfaces. Make `orbit-server` depend on both libraries. Replace `Hello, world!` with a CLI that supports `--help` and exits successfully; later tasks add subcommands.

- [ ] **Step 3: Switch the frontend lockfile to Bun**

Run:

```bash
cd apps/web
rm aube-lock.yaml
bun install
bun install --frozen-lockfile
```

Expected: `bun.lock` exists and the frozen install succeeds.

- [ ] **Step 4: Add the Just recipes**

Recipes must invoke explicit underlying commands and use traps in `just dev` so Rust and Vite stop together. Use backend port 8080 and Vite port 8888. `just check` runs `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `bun run lint`, `bun run test`, `just api-check`, `bun run build`, and `cargo build --release --workspace`.

- [ ] **Step 5: Verify and commit**

```bash
cargo metadata --no-deps --format-version 1 >/dev/null
cargo run -p orbit-server -- --help
cd apps/web && bun install --frozen-lockfile && bun run build
just --list
```

Commit:

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml crates apps/server apps/web justfile .ai/DEVELOPMENT.md .gitignore
git commit -m "build: create Orbit Rust workspace"
```

---

### Task 2: Platform IDs, time, config, and Problem Details

**Files:**
- Create: `crates/platform/src/id.rs`, `time.rs`, `config.rs`, `problem.rs`
- Create: `crates/platform/tests/config.rs`, `problem.rs`
- Modify: `crates/platform/src/lib.rs`, `crates/platform/Cargo.toml`

**Interfaces:**
- Produces `Id::new_v7()`, `TimestampMillis`, `Config::load(ConfigSources)`, and `Problem`.
- `ConfigSources { path: PathBuf, env: BTreeMap<String,String>, cli: ConfigOverride }` uses CLI > environment > TOML precedence.
- `Problem` serializes RFC 9457 fields plus `code`, `request_id`, optional field errors, and optional conflict metadata.

- [ ] **Step 1: Write failing value and precedence tests**

```rust
#[test]
fn cli_overrides_env_and_toml() {
    let cfg = load_fixture("http.port = 8080", [("ORBIT__HTTP__PORT", "8081")], Some(8082)).unwrap();
    assert_eq!(cfg.http.port, 8082);
}

#[test]
fn problem_has_stable_machine_code() {
    let p = Problem::validation("req-1", "/api/v1/tasks", [("title", "Title is required.")]);
    assert_eq!(p.code, "validation_failed");
    assert_eq!(p.status, 422);
}
```

Also cover unknown TOML keys, conflicting secret and `_FILE`, one trailing newline removal, UUIDv7 canonical round-trip, and RFC 3339 timestamp serialization.

- [ ] **Step 2: Run focused tests and confirm failure**

```bash
cargo test -p orbit-platform --test config --test problem
```

Expected: compile failure because the modules do not exist.

- [ ] **Step 3: Implement the public types**

Use strict Serde configuration. Define secrets with a redacted `Debug`. Reject unknown keys and ambiguous secret sources. Store time internally as `i64` milliseconds and serialize as RFC 3339 UTC. Ensure `Problem::internal` never accepts a raw internal error as public detail.

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p orbit-platform
cargo clippy -p orbit-platform --all-targets -- -D warnings
```

```bash
git add crates/platform
git commit -m "feat(platform): add core configuration and API values"
```

---

### Task 3: SQLite pool, migrations, and repository test harness

**Files:**
- Create: `crates/platform/src/db/mod.rs`, `db/migrate.rs`, `db/test_db.rs`
- Create: `apps/server/migrations/0001_platform.sql`
- Create: `crates/platform/tests/database.rs`
- Modify: `crates/platform/src/lib.rs`, `crates/platform/Cargo.toml`

**Interfaces:**
- Produces `Database::open(&DatabaseConfig)`, `Database::transaction`, `MigrationRunner::run`, and `TestDatabase::new()`.
- Pool initialization enforces WAL, foreign keys, busy timeout, and one process ownership lock.
- Platform tables include migration metadata, installation state, jobs, job attempts, schedules, audit events, outbox events, attachment blobs, and pending uploads.

- [ ] **Step 1: Write failing SQLite configuration tests**

```rust
#[tokio::test]
async fn opens_sqlite_with_required_pragmas() {
    let db = TestDatabase::new().await.unwrap();
    assert_eq!(db.scalar::<String>("PRAGMA journal_mode").await.unwrap().to_lowercase(), "wal");
    assert_eq!(db.scalar::<i64>("PRAGMA foreign_keys").await.unwrap(), 1);
}
```

Add tests for migration checksum mismatch, schema newer than binary, rollback on failing transactional migration, and isolated temporary paths.

- [ ] **Step 2: Run and observe failure**

```bash
cargo test -p orbit-platform --test database
```

- [ ] **Step 3: Implement SQLite setup and embedded migrations**

Use SQLx. Keep transactions short. Store migration version, checksum, applied timestamp, and app version. Expose destructive-migration metadata to the server backup coordinator rather than creating backups inside the database module.

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p orbit-platform --test database
cargo test -p orbit-platform
```

```bash
git add crates/platform apps/server/migrations
git commit -m "feat(platform): add SQLite and migrations"
```

---

### Task 4: HTTP shell, security middleware, health, and observability

**Files:**
- Create: `crates/platform/src/http/mod.rs`, `http/request_id.rs`, `http/security.rs`, `http/limits.rs`, `observability.rs`, `health.rs`
- Create: `crates/platform/tests/http_security.rs`
- Modify: `crates/platform/src/lib.rs`, `crates/platform/Cargo.toml`

**Interfaces:**
- Produces `HttpPlatformLayer::new`, `RequestId`, `OriginPolicy`, `HealthRegistry`, and `init_tracing`.
- Middleware emits Problem Details, validates trusted proxy headers and origins, and adds CSP and security headers.

- [ ] **Step 1: Write failing router tests**

Test that unsafe methods with a foreign or missing browser Origin fail, untrusted forwarded headers are ignored, request IDs are generated, CSP and `nosniff` are present, and internal errors expose only a request ID.

```rust
let response = app.oneshot(post("/probe").header("origin", "https://evil.test")).await.unwrap();
assert_eq!(response.status(), StatusCode::FORBIDDEN);
assert_eq!(response.headers()[CONTENT_TYPE], "application/problem+json");
```

- [ ] **Step 2: Implement middleware and health primitives**

Keep liveness process-only. Model readiness checks as named async checks for migrations, DB integrity state, writable storage, scheduler, and critical config. Do not add workspace/user IDs as metric labels.

- [ ] **Step 3: Verify and commit**

```bash
cargo test -p orbit-platform --test http_security
cargo clippy -p orbit-platform --all-targets -- -D warnings
```

```bash
git add crates/platform
git commit -m "feat(platform): add HTTP security and health"
```

---

### Task 5: Durable jobs and recurring scheduler

**Files:**
- Create: `crates/platform/src/jobs/mod.rs`, `jobs/store.rs`, `jobs/worker.rs`, `jobs/schedule.rs`
- Create: `crates/platform/tests/jobs.rs`
- Modify: `crates/platform/src/lib.rs`, `crates/platform/Cargo.toml`

**Interfaces:**
- Produces typed `Job`, `JobKind`, `JobContext`, `JobError::{Retryable,Permanent}`, `JobQueue::enqueue`, and `Worker::run`.
- Claims return `{ job_id, attempt, claim_token, lease_expires_at }`; every heartbeat/final write requires the claim token.

- [ ] **Step 1: Write failing queue tests**

Cover FIFO inside priority, reserved critical capacity, eight-attempt schedule, positive jitter bounded to 20%, expired lease recovery, stale claim rejection, manual retry linkage, dead-job purge, schedule deduplication, UTC cron, and coalesced missed runs.

```rust
#[tokio::test]
async fn stale_claim_cannot_complete_reclaimed_job() {
    let first = store.claim(now).await.unwrap().unwrap();
    let second = store.claim(now + FIVE_MINUTES).await.unwrap().unwrap();
    assert_ne!(first.claim_token, second.claim_token);
    assert!(!store.complete(first).await.unwrap());
    assert!(store.complete(second).await.unwrap());
}
```

- [ ] **Step 2: Implement store, scheduler, and worker**

Use the exact D-039 through D-042 defaults. Stop claiming on cancellation, drain for 30 seconds, and leave cancelled jobs leased. Redact declared sensitive payload fields from diagnostics.

- [ ] **Step 3: Verify and commit**

```bash
cargo test -p orbit-platform --test jobs
cargo test -p orbit-platform
```

```bash
git add crates/platform
git commit -m "feat(platform): add durable SQLite jobs"
```

---

### Task 6: Backups, integrity checks, and operations CLI

**Files:**
- Create: `crates/platform/src/backup.rs`, `integrity.rs`
- Create: `crates/platform/tests/backup.rs`, `integrity.rs`
- Create: `apps/server/src/cli.rs`
- Modify: `apps/server/src/main.rs`, `apps/server/Cargo.toml`

**Interfaces:**
- Produces `BackupService::{create,list,verify,restore}`, `IntegrityService::{quick,full}`, and CLI subcommands `config`, `migrate`, `backup`, `db-reset`, and `seed`.

- [ ] **Step 1: Write failing backup and corruption tests**

Build a fixture containing one DB record and one attachment. Assert the manifest contains both checksums, restore refuses a held ownership lock, a damaged blob fails verify, and a failed `quick_check` makes readiness fail.

- [ ] **Step 2: Implement verified snapshots and retention**

Use SQLite online backup, pause attachment mutations through a narrow coordination guard, verify before retention deletion, keep 7 daily and 4 weekly snapshots, and preserve pre-migration backups separately. Never implement automatic repair.

- [ ] **Step 3: Implement CLI behavior**

`orbit config show` redacts secrets. Restore works only without the serving lock. Commands return nonzero with a concise recovery message on failure.

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p orbit-platform --test backup --test integrity
cargo test -p orbit-server
```

```bash
git add crates/platform apps/server
git commit -m "feat(server): add backup and database operations"
```

---

### Task 7: Local attachment storage

**Files:**
- Create: `crates/platform/src/files/mod.rs`, `files/local.rs`, `files/upload.rs`
- Create: `crates/platform/tests/files.rs`
- Modify: `crates/platform/src/lib.rs`, `crates/platform/Cargo.toml`

**Interfaces:**
- Produces `BlobStore` port and `LocalBlobStore`, plus `UploadService::stage`, `finalize`, and `reconcile`.
- `StagedUpload` includes workspace, owner, SHA-256, size, detected MIME, safe display name, and temporary path.

- [ ] **Step 1: Write failing streaming tests**

Cover 25 MiB file and 100 MiB request defaults, early rejection, temporary cleanup, MIME sniffing, same-workspace deduplication, cross-workspace isolation, atomic rename, 24-hour orphan quarantine, and `nosniff` download metadata.

- [ ] **Step 2: Implement storage and reconciliation**

Stream and hash without full buffering. Inline only JPEG, PNG, GIF, and WebP; download SVG and active content. Resolve attachment authorization before blob access. Query references before deletion rather than maintaining a reference counter.

- [ ] **Step 3: Verify and commit**

```bash
cargo test -p orbit-platform --test files
```

```bash
git add crates/platform
git commit -m "feat(platform): add local attachment storage"
```

---

### Task 8: Orbit domain models and policy matrix

**Files:**
- Create: `crates/orbit/src/identity.rs`, `workspace.rs`, `tasks.rs`, `policy.rs`, `ports.rs`
- Create: `crates/orbit/tests/policies.rs`, `tasks.rs`
- Modify: `crates/orbit/src/lib.rs`, `crates/orbit/Cargo.toml`

**Interfaces:**
- Produces global `User`, workspace `Membership`, `WorkspaceRole::{Owner,Admin,Member}`, `Workspace`, `Project`, `TaskStatus`, `Task`, `Comment`, `AttachmentRef`, and repository/service ports.
- Mutable aggregates expose `version: u64`; use cases accept `ExpectedVersion`.

- [ ] **Step 1: Write failing pure-domain tests**

Cover exactly one owner, transfer-before-leave, Admin cannot alter Owner, Member content CRUD, cross-workspace denial, default workspace/project/status construction, stale version conflicts, parent soft-delete visibility, and restore uniqueness conflict.

```rust
#[test]
fn admin_cannot_remove_owner() {
    let result = Policy::authorize(Role::Admin, Action::RemoveMembership, TargetRole::Owner);
    assert_eq!(result, Err(PolicyError::OwnerProtected));
}
```

- [ ] **Step 2: Implement minimal domain and ports**

No Axum or SQLx types. Keep repository methods workspace-scoped in their signatures. Use typed domain errors that server adapters map to Problem Details.

- [ ] **Step 3: Verify dependency direction and commit**

```bash
cargo test -p orbit-domain
! grep -R 'use axum\|use sqlx' crates/orbit/src
```

```bash
git add crates/orbit
git commit -m "feat(domain): add workspace and task models"
```

---

### Task 9: Authentication, setup, sessions, and recovery

**Files:**
- Create: `crates/platform/src/auth/mod.rs`, `auth/password.rs`, `auth/session.rs`, `auth/throttle.rs`, `auth/token.rs`
- Create: `crates/platform/tests/auth.rs`
- Create: `apps/server/src/auth_routes.rs`, `apps/server/src/repositories/identity.rs`
- Create: `apps/server/migrations/0002_identity.sql`

**Interfaces:**
- Endpoints: `/api/v1/setup/status`, `/api/v1/setup/complete`, `/api/v1/auth/login`, `/logout`, `/me`, `/recovery/request`, `/recovery/complete`, `/sessions`, and `/sessions/{id}` delete.
- Produces `AuthenticatedUser`, `SessionStore`, password hashing/rehash, setup/recovery token services, and throttler.

- [ ] **Step 1: Write failing auth tests**

Cover 12-128 character policy, common-password rejection, no Unicode normalization, Argon2id verify/rehash, one-time setup race, hashed token persistence, 30-day idle/90-day absolute session expiry, throttled activity writes, generic login/recovery errors, exact progressive throttling, secure cookie attributes, Origin failure, and immediate revocation.

- [ ] **Step 2: Implement auth services and schema**

Store normalized email separately from display email. Use cryptographic random tokens and constant-time hash comparison. Bootstrap user/workspace/default project in one transaction through the Orbit use case. Never reopen setup after initialization.

- [ ] **Step 3: Implement HTTP routes and OpenAPI annotations**

Cookies use `__Host-`, `Secure`, `HttpOnly`, `SameSite=Lax`, and path `/`; explicit loopback development mode uses a non-prefixed insecure cookie name. Map failures to D-043 codes.

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p orbit-platform --test auth
cargo test -p orbit-server auth
```

```bash
git add crates/platform apps/server
git commit -m "feat(auth): add setup and browser sessions"
```

---

### Task 10: Workspace membership, invitations, audit, and APIs

**Files:**
- Create: `apps/server/migrations/0003_workspaces.sql`
- Create: `apps/server/src/repositories/workspaces.rs`, `workspace_routes.rs`, `audit.rs`
- Create: `apps/server/tests/workspaces_api.rs`

**Interfaces:**
- Endpoints under `/api/v1/workspaces` for list/create and `/{workspace_id}` for settings, members, invitations, role changes, ownership transfer, delete, trash, and restore.
- Invitation token lifetime is seven days; resend atomically invalidates prior pending invitations for normalized email/workspace.

- [ ] **Step 1: Write failing API tests**

Cover global users in multiple workspaces, path-scoped authorization, unknown resource non-leakage, Owner/Admin invite and revoke, mismatched-email rejection, existing membership idempotence, manual-link non-verification, transfer-before-owner removal, global suspension, and workspace audit filtering. Define delivery provenance in the invitation model so the later SMTP transport can mark emailed invitations verified without changing acceptance DTOs.

- [ ] **Step 2: Implement SQLx repositories and use-case adapters**

Every query includes workspace scope. Enforce ownership and uniqueness in both use cases and database constraints. Audit security actions with safe bounded metadata and 365-day retention.

- [ ] **Step 3: Implement routes and OpenAPI annotations**

Return cursor-paginated members/invitations and strict DTO errors. Manual invite response returns the one-time URL only to authorized callers and never logs it.

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p orbit-server --test workspaces_api
```

```bash
git add apps/server
git commit -m "feat(workspaces): add memberships and invitations"
```

---

### Task 11: Projects, statuses, tasks, comments, and trash APIs

**Files:**
- Create: `apps/server/migrations/0004_tasks.sql`
- Create: `apps/server/src/repositories/tasks.rs`, `task_routes.rs`
- Create: `apps/server/tests/tasks_api.rs`

**Interfaces:**
- Endpoints under `/api/v1/workspaces/{workspace_id}` for projects, statuses, labels, tasks, comments, bulk mutations, trash, and restore.
- List requests use opaque cursor pagination with default 50/max 100 and UUIDv7 tie-breaker.

- [ ] **Step 1: Write failing repository and API tests**

Cover initial project/status transaction, project-scoped statuses, filtering/sorting/cursors, cursor/query mismatch, version conflicts, Member CRUD including deletion, task assignment only to active memberships, bulk atomicity, parent soft-delete hiding, 30-day trash, restore conflict, comments, and cross-workspace IDs.

- [ ] **Step 2: Implement schema and repositories**

Use explicit SQL. Soft-delete workspace/project/task rows. Hard-delete comments only through authorized actions. Implement manual ordering with stable numeric positions and transactions for reorder/bulk updates.

- [ ] **Step 3: Implement handlers and conflict metadata**

Strict DTOs reject unknown fields. `409` problems include current safe record/version data. All collection endpoints use D-045 cursors.

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p orbit-server --test tasks_api
```

```bash
git add apps/server
git commit -m "feat(tasks): add persistent task APIs"
```

---

### Task 12: Attachment HTTP integration

**Files:**
- Create: `apps/server/src/attachment_routes.rs`
- Create: `apps/server/tests/attachments_api.rs`
- Modify: `apps/server/migrations/0004_tasks.sql`

**Interfaces:**
- Endpoints: task/comment attachment upload, metadata list/delete, and authenticated download.
- Consumes Task 7 `UploadService` and Task 11 authorization/repositories.

- [ ] **Step 1: Write failing end-to-end storage tests**

Upload a real PNG fixture, attach duplicate bytes twice, assert one workspace blob/two metadata rows, deny another workspace, force DB finalization failure and verify orphan quarantine, reject oversized streams, and assert SVG downloads with attachment disposition.

- [ ] **Step 2: Implement staged upload endpoints**

Apply D-047 through D-049 exactly. Do not accept blob IDs from clients. Add startup/scheduled reconciliation and unreferenced-file cleanup jobs.

- [ ] **Step 3: Verify and commit**

```bash
cargo test -p orbit-server --test attachments_api
```

```bash
git add apps/server
git commit -m "feat(tasks): add authenticated attachments"
```

---

### Task 13: OpenAPI generation and frontend application boundary

**Files:**
- Create: `apps/server/src/openapi.rs`
- Create: `apps/web/src/api/generated/`, `api/client.ts`, `api/problem.ts`, `api/queryKeys.ts`
- Create: `apps/web/src/app/Providers.tsx`, `features/auth/AuthGate.tsx`, `components/shell/MockFeatureBadge.tsx`
- Modify: `apps/web/src/main.tsx`, `App.tsx`, `package.json`, `vite.config.ts`
- Create: `apps/web/src/api/problem.test.ts`, `api/queryKeys.test.ts`

**Interfaces:**
- `just api` writes deterministic OpenAPI JSON and generated TypeScript.
- `apiClient` sends same-origin credentials and contract ID; hooks use workspace-scoped query keys.

- [ ] **Step 1: Write failing generation and frontend tests**

Assert two OpenAPI generations are byte-identical, generated code is clean, Problem Details maps field paths, workspace query keys never collide, and AuthGate distinguishes setup/login/authenticated states.

- [ ] **Step 2: Generate and wrap the client**

Add TanStack Query. Do not hand-edit generated files. Centralize `credentials: 'include'`, contract header, Problem parsing, and 401 behavior.

- [ ] **Step 3: Add providers, routes, and mock indicators**

Add setup/login/recovery routes outside `AppShell`. Keep unmigrated routes working and show a development-only mock badge. Configure Vite `/api` and WebSocket proxy to backend port 8080.

- [ ] **Step 4: Verify and commit**

```bash
just api
just api-check
cd apps/web && bun run test && bun run build
```

```bash
git add apps/server apps/web justfile
git commit -m "feat(web): add generated API boundary"
```

---

### Task 14: Frontend workspace and task cutover

**Files:**
- Create: `apps/web/src/features/auth/`, `features/workspaces/`, `features/tasks/api/`, `features/settings/api/`
- Modify: `apps/web/src/App.tsx`, `components/shell/AppShell.tsx`, `features/settings/MembersPage.tsx`, `SessionsPage.tsx`, `features/tasks/TasksPage.tsx`, `ProjectSettingsPage.tsx`, and task components
- Modify: `apps/web/src/mock/store.ts`, `mock/actions.ts`, `mock/types.ts`
- Create tests next to new hooks/components and `apps/web/e2e/foundation-tasks.spec.ts`

**Interfaces:**
- Hooks: `useCurrentUser`, `useWorkspaces`, `useMembers`, `useProjects`, `useTasks`, `useTask`, and matching mutation hooks.
- Migrated components never call task/workspace/user mutation functions from `mock/actions.ts`.

- [ ] **Step 1: Add failing setup/auth/workspace UI tests**

Cover setup token exchange/removal from URL, login, logout, workspace switch, invite copy flow, role protections, sessions, loading/errors, and no mock fallback on API failure.

- [ ] **Step 2: Implement identity and workspace hooks/views**

Use generated calls behind feature hooks. Keep selected workspace in route/navigation preference, never as authorization state. Invalidate only keys under the affected workspace.

- [ ] **Step 3: Add failing task mutation tests**

Cover filters, cursor continuation, create/detail autofocus, inline status/priority, bulk updates, reorder rollback, stale-version refresh prompt, comments, upload progress, attachment-only comments, trash, and restore conflicts.

- [ ] **Step 4: Cut task components over incrementally**

Preserve current UI behavior. Remove only migrated mock selectors/actions/types. On optimistic failure restore the captured cache snapshot; on success reconcile with the server record/version.

- [ ] **Step 5: Add Playwright milestone flow**

```ts
test('setup through restored task', async ({ page }) => {
  await bootstrapOwner(page)
  await createWorkspaceAndInviteMember(page)
  await createTaskWithAttachment(page)
  await deleteAndRestoreTask(page)
  await expect(page.getByText('Restored task')).toBeVisible()
})
```

Use an isolated server/data directory started by `just e2e`.

- [ ] **Step 6: Verify and commit**

```bash
cd apps/web
bun run lint
bun run test
bun run build
bunx playwright test e2e/foundation-tasks.spec.ts
```

```bash
git add apps/web
git commit -m "feat(web): persist workspaces and tasks"
```

---

### Task 15: Server composition, embedded assets, and milestone gate

**Files:**
- Create: `apps/server/src/router.rs`, `app.rs`, `static_assets.rs`, `main.rs`
- Create: `apps/server/tests/app_smoke.rs`
- Create: `Dockerfile`, `.dockerignore`, `config/orbit.example.toml`
- Modify: `.ai/ARCHITECTURE.md`, `.ai/DEVELOPMENT.md`, `.ai/PROJECT_OVERVIEW.md`, `.ai/FEATURES.md`

**Interfaces:**
- `App::build(Config)` wires database, migrations, repositories, jobs, storage, auth, routes, health, metrics, and shutdown.
- `orbit serve` binds HTTP only after config, integrity, backup-required migration, and readiness initialization succeed.

- [ ] **Step 1: Write failing composition smoke tests**

Cover unconfigured startup/setup URL, migration-before-listener, SPA fallback but API 404 Problem, embedded asset cache headers, readiness failure, graceful job drain, and production cookie behavior behind a trusted HTTPS proxy.

- [ ] **Step 2: Compose the application**

Keep route modules independent and merge them in `router.rs`. Start workers after migrations and quick check. Stop accepting HTTP, then drain workers for 30 seconds. Embed only a verified matching frontend build.

- [ ] **Step 3: Add production packaging**

Use a multi-stage OCI build. Final image runs the non-root `orbit` user, contains only the binary and required CA/timezone data, and declares data/config volumes plus HTTP and optional SMTP ports. Do not implement inbound SMTP in this milestone.

- [ ] **Step 4: Update project documentation**

Document exact Bun, Just, native binary, OCI, reverse proxy, backup, restore, and first-run setup commands. Mark foundation/tasks real and all other features mock-backed.

- [ ] **Step 5: Run the complete gate**

```bash
just check
just e2e
docker build -t orbit:milestone-1 .
docker run --rm orbit:milestone-1 --help
```

Start the Jean run environment before manual UI verification. Verify setup, login, two-workspace isolation, invitation, task CRUD, conflict handling, attachment download, trash/restore, session revocation, and mock badges at the exact Jean URL.

- [ ] **Step 6: Commit**

```bash
git add apps/server Dockerfile .dockerignore config .ai
git commit -m "feat: deliver persistent Orbit foundation and tasks"
```

## Follow-up plans, not part of milestone one

Create separate approved specs and plans before implementing:

1. WebSocket outbox, replay, resync, presence, and typing integration from D-050 through D-052. The first task milestone does not require realtime.
2. Transactional SMTP provider and template delivery from D-017 and D-053. Manual invitation/recovery links keep milestone one functional without SMTP.
3. Embedded inbound SMTP, public-MX/trusted-relay operation, domains, and mailboxes from D-054 through D-056.
4. Persistent docs, chat, DMs, inbox, and mail-client migrations.


## Spec coverage review

- D-001 through D-006 and D-019 through D-024 are covered by Tasks 1-4, 8, and 15.
- D-007 and D-036 through D-042 are covered by Tasks 3, 5, 6, and 15.
- D-008 through D-018 and D-029 through D-035 are covered by Tasks 8-10 and 13-14. SMTP sending from D-017 is explicitly a follow-up; milestone one implements the accepted administrator-link fallback.
- D-025, D-026, D-028, D-043 through D-046, and D-062 through D-069 are covered by Tasks 4 and 13-15.
- D-027 and D-047 through D-049 are covered by Tasks 7, 12, and 14.
- D-050 through D-052 are explicitly deferred because D-010 says realtime is not a milestone-one prerequisite. Their contract remains fixed for the follow-up plan.
- D-053 through D-056 are explicitly deferred except for the outbound transport port needed by auth jobs. Inbound SMTP and mailbox behavior stay out of milestone one.
- D-057 through D-061 are covered by Tasks 1-6 and 15.
- No mock-to-production importer, public registration, Visitor role, custom permission roles, public attachment URLs, inbound SMTP listener, or persistent backend for docs/chat/DMs/inbox/mail is included.
