# Publish one image for every main commit

- [x] Replace the version-tag release trigger with a push trigger for `main`.
- [x] Tag each multi-platform image with the full commit SHA.
- [x] Remove GitHub Release and downloadable binary creation.
- [x] Keep native AMD64 and ARM64 builds and smoke tests.
- [x] Let all image runs finish so that no commit image is skipped.
- [x] Remove the old Git version tags and GitHub Releases.
- [x] Remove old `v<number>` container images, then remove the one-time cleanup job.
- [x] Validate, commit, push, and verify the image workflow.

## Review

- Each commit on `main` publishes `ghcr.io/coollabsio/orbit:<full-commit-sha>`.
- The workflow does not create version tags, GitHub Releases, or release binaries.
- Commit `68a2f85ce4e8a7e6f1d06bbd0d8f06ba11736ffc` published and passed native AMD64 and ARM64 smoke tests.
- Its manifest contains `linux/amd64` and `linux/arm64`; Bash 5.3.3 runs as user and group 65532.
