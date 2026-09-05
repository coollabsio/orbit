# Development bootstrap report

## Result

Development bootstrap now has one migration entry point:

- `orbit migrate run --seed` migrates first, then installs the idempotent development dataset with `test@example.com` / `password`.
- `orbit migrate run --reset` works only with explicit development configuration and `--yes`. It clears the SQLite database, sidecars, and configured attachment data before migrating.
- `orbit migrate run --reset --seed --yes` runs reset, migration, and seed in that order.
- Repeated seeding keeps edited user and task fields and does not duplicate the representative records.
- `just dev` seeds before it starts Orbit and Vite. It has no reset step.
- Development accepts any valid HTTP or HTTPS Origin but still rejects missing, malformed, non-HTTP, and ambiguous Origin headers. Production keeps its configured exact-origin policy.
- Jean keeps frontend port `18888` and API port `18080`. Its run command no longer sets an origin-specific environment variable.

The compatibility `db-reset` and `seed` CLI commands remain available.

## TDD evidence

The CLI regression group first failed to compile because `MigrateCommand::Run` did not expose the planned flags. After the parser and execution changes, its 12 focused tests passed. Coverage verifies flag parsing, production rejection before data access, reset confirmation, exact credential verification, preserved edits, idempotence, attachment and sidecar cleanup, and reset-then-seed recreation.

The HTTP platform regression first failed because `OriginPolicy` had no development option. The app composition regression returned `403` for a distinct localhost Origin. Both passed after adding the development-only policy selection. A later malformed-origin regression reproduced a configured-origin bypass through trailing slash normalization, then passed after development mode began using strict URI validation instead of the exact-origin fallback.

Static startup checks first failed because `just dev` did not invoke `migrate run --seed` and Jean still contained `ORBIT_DEV_PUBLIC_ORIGIN`. The final checks confirm the seed command is present, the dev recipe contains no `--reset`, and Jean contains no origin override.

## Verification

- `just check`: passed on the final implementation.
  - 294 Rust tests passed.
  - 61 Bun tests passed with 185 assertions.
  - Rust formatting, warning-denied Clippy, frontend lint, deterministic API drift, checked-in web asset drift, and the release workspace build passed.
  - Vite emitted its existing large-chunk advisory.
- Focused CLI tests: 12 passed.
- Focused platform origin tests: 3 passed.
- Focused app composition test: passed.
- Jean configuration JSON parsing, `just --list`, startup seed/no-reset checks, origin-variable removal, and `git diff --check`: passed.
- Live Jean smoke at frontend `http://127.0.0.1:18888` and API `http://127.0.0.1:18080`:
  - login with `test@example.com` / `password` and Origin `http://localhost:18888`: `200`;
  - the same login with Origin `https://jean-server.tail661ee3.ts.net:8888`: `200`;
  - the same login without Origin: `403 origin_forbidden`.

The existing Jean process was left running until live verification required the new binary. It was then restarted through Jean with the updated command and dedicated ports.

The first final `just check` run hit the pre-existing timing-sensitive `cancelling_while_cleanup_is_waiting_still_removes_the_temporary_file` test once. The isolated test passed five consecutive runs, and the complete `just check` rerun passed.
