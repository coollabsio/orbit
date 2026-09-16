# Verify Orbit restart

- [x] Restart `orbit-local`.
- [x] Verify container health and Tailscale HTTPS readiness.
- [x] Confirm the setup state survives the restart.

## Review
- Restart completed at `2026-09-14T18:22:02.006848357Z`.
- The container is running and all readiness checks pass through Tailscale HTTPS.
- The first-run setup state and persistent volumes survived the restart.
