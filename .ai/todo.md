# Add Bash to the runtime image

- [x] Add a failing production-image check for Bash under the runtime user.
- [x] Replace the scratch runtime with a minimal Alpine and Bash runtime.
- [x] Verify the image shell, non-root process, health check, and production smoke flow.
- [x] Review and document the final result.

## Review

- The final image is based on Alpine 3.23 and includes Bash 5.3.3, CA certificates, and timezone data.
- Orbit and interactive Bash sessions still run as numeric user and group 65532.
- The old `v0.1.1` image failed the new Bash check with `/bin/bash` missing.
- The new image passes the Bash check and the full production image smoke flow, including restart readiness and the exec-form health check.
