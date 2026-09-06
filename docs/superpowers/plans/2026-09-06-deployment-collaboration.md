# Deployment and task collaboration

## Scope and decisions

- Keep development seeds and login defaults. Production must reject seed/reset commands and require first-owner setup. Use separate production storage; never deploy the development database.
- Ship a Linux executable with embedded production frontend and a non-root Docker image with persistent data and backup volumes. Production sits behind a trusted HTTPS proxy. Publish versioned images alongside release binaries.
- Email delivery is explicitly out of scope. Share invitation/recovery links manually.
- Implement task collaboration using D-050/D-051: cookie-authenticated, origin-checked WebSockets; HTTP remains authoritative for writes and reads; per-workspace transactional outbox, seven-day replay, reconnect/resync, existing version conflicts.
- Each tab subscribes to its active workspace. Events contain invalidation signals, not task/user content. Check current session and membership before every delivery. Presence/typing and collaborative text merging remain separate features.

## Execution order

1. Test transactional notifications, replay boundaries, and authorization.
2. Add outbox writes and authenticated WebSocket delivery; wire workspace cache refresh/reconnect in the frontend.
3. Verify production seed guards, provide Docker Compose deployment and release publishing.
4. Run backend/frontend checks, build the image, smoke-test isolated deployment, and verify updates in two browser sessions.

Work is synchronous at the user's request. Never reset or replace the running development data.

## Verification

- 304 Rust tests pass, including seed/reset production guards, migration/backup compatibility, transactional outbox rollback, cursor validation, session suspension/revocation, and upgrade Origin enforcement.
- 74 frontend tests, lint, production web build, Clippy, and API generation drift checks pass.
- Two browser sessions at `http://127.0.0.1:18888` receive task creation/title/comment changes, preserve a focused draft, and catch up after reconnect. The Tailscale HTTPS endpoint also passed task/title/reconnect checks.
- Deployment image build and isolated production smoke are the final packaging gate; see the completion entry below when verified.

## Packaging completion

- Built `orbit:deployment-test` and tagged it `orbit:local`.
- Extracted `target/deploy/orbit-linux-x86_64` with SHA-256 checksum. `file` confirms a static-pie x86_64 Linux executable, approximately 18 MiB; it runs outside the container.
- The deployment smoke checks use a simulated trusted HTTPS proxy, not public certificate issuance. Docker may change its dynamically assigned host port on restart, so the smoke test rediscovers the port.
- Isolated production image smoke passed at `http://127.0.0.1:32775`: non-root/read-only execution, HTTPS enforcement, embedded SPA, setup requirement, rejected seeded credentials and dev seed/reset flags, and readiness after restart. Temporary containers/volumes are removed by the smoke script.
- No production domain was provisioned and no image/release was pushed. Set the deployment hostname and retain an off-host backup before using it for important data.
