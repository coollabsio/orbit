# Development Bootstrap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make development start with an idempotently seeded login while consolidating explicit reset/seed behavior under `migrate run`.

**Architecture:** Extend the existing CLI migration operation rather than adding a second bootstrap path. Keep origin relaxation inside the existing HTTP origin policy and gate it with validated `EnvironmentMode::Development`; production behavior remains unchanged.

**Tech Stack:** Rust, Clap, Axum/Tower, SQLite/SQLx, Just, Jean, Bun.

## Global Constraints

- Development credentials are exactly `test@example.com` / `password`.
- `--seed` preserves existing data; `--reset` is development-only and requires `--yes`.
- Missing Origin remains rejected for unsafe requests in every environment.
- Production exact-origin and one-time setup behavior remain unchanged.

---

### Task 1: Migration bootstrap flags and seed identity

**Files:**
- Modify: `apps/server/src/cli.rs`
- Test: `apps/server/src/cli.rs`

**Interfaces:**
- Produces: `orbit migrate run [--reset] [--seed] [--yes]`.

- [ ] Add parser and behavioral tests for flag combinations, environment restrictions, reset confirmation, idempotence, preserved edits, exact credentials, and reset-plus-seed ordering.
- [ ] Run focused CLI tests and verify the new tests fail for missing flags/old credentials.
- [ ] Extend `MigrateCommand::Run` with flags and sequence reset → guarded migration → seed.
- [ ] Change seed identity to the exact development credentials while retaining representative data and idempotence.
- [ ] Run focused CLI tests and commit.

### Task 2: Development origin policy and Jean startup

**Files:**
- Modify: `crates/platform/src/http/security.rs`
- Modify: `crates/platform/src/http/mod.rs`
- Modify: `apps/server/src/app.rs`
- Modify: `justfile`
- Modify: `jean.json`
- Test: `crates/platform/tests/http_security.rs`
- Test: `apps/server/tests/app_smoke.rs`

**Interfaces:**
- Consumes: validated `EnvironmentMode` from `Config`.
- Produces: development-only any-valid-Origin policy and a non-destructive seeded `just dev`.

- [ ] Add failing tests proving development accepts distinct localhost/Tailscale Origins, rejects malformed/missing Origin, and production remains exact-match.
- [ ] Add an explicit development-mode option to `OriginPolicy` and select it only from development app composition.
- [ ] Update `just dev` to run `migrate run --seed` before starting services, without reset.
- [ ] Remove Jean's origin-specific environment variable while retaining dedicated ports.
- [ ] Run Rust, Bun, configuration, and drift checks; commit.

