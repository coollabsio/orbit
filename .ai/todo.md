# Publish one image for every main commit

- [x] Replace the version-tag release trigger with a push trigger for `main`.
- [x] Tag each multi-platform image with the full commit SHA.
- [x] Remove GitHub Release and downloadable binary creation.
- [x] Keep native AMD64 and ARM64 builds and smoke tests.
- [x] Let all image runs finish so that no commit image is skipped.
- [ ] Validate, commit, push, and verify the image workflow.

## Review

- Each commit on `main` publishes `ghcr.io/coollabsio/orbit:<full-commit-sha>`.
- The workflow does not create version tags, GitHub Releases, or release binaries.
