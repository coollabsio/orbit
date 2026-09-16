# Serialize GitHub Actions workflows

- [x] Cancel superseded CI runs on the same branch or pull request.
- [x] Cancel an older release run when a newer release starts.
- [x] Validate both workflow files and review the diff.

## Review

- CI uses a workflow-and-ref group, so a new run cancels an older run only for the same branch or pull request.
- Release uses one workflow-wide group, so the newest tag release cancels an older release run.
- Actionlint 1.7.7 and `git diff --check` pass.
