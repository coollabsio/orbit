# Serialize GitHub Actions workflows

- [x] Allow only the newest CI run across the repository.
- [x] Cancel an older release run when a newer release starts.
- [x] Validate both workflow files and review the diff.

## Review

- CI uses one workflow-wide group, so a new run cancels any older CI run.
- Release uses one workflow-wide group, so the newest tag release cancels an older release run.
- Actionlint 1.7.7 and `git diff --check` pass.
