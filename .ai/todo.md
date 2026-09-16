# Publish native multi-platform images

- [x] Split release quality, native image builds, and manifest publication.
- [x] Assert runner, Docker daemon, and image architecture without QEMU.
- [x] Verify the workflow contract and repository quality gate.
- [x] Review the final diff and document release/test results.

## Review

- The release matrix builds AMD64 on `ubuntu-24.04` and ARM64 on `ubuntu-24.04-arm`.
- Each job verifies the native kernel and Docker daemon architecture, checks the built image architecture, and runs the production smoke test before pushing.
- No QEMU action or foreign `--platform` build is present. The final tag is a manifest containing both tested architecture tags.
- Actionlint 1.7.7, `git diff --check`, and `just check` pass.
- A new `v*` tag must run the release workflow before an ARM64 image is available in GHCR; the existing `v0.1.0` remains AMD64-only.
