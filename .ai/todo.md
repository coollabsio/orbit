# Move release quality gate to CI

- [x] Remove the duplicate general quality job from Release.
- [x] Run the production-image smoke test in CI.
- [x] Validate workflows and push the change.
- [ ] Wait for CI success before creating the next release tag.

## Review

- General checks, browser E2E, Docker build, CLI smoke, and production-image smoke now run in CI.
- Release starts native image builds directly and retains per-architecture smoke tests.
- Actionlint 1.7.7 and `git diff --check` pass.
