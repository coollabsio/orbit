# Releases

Every release is tagged `v<version>`, such as `v0.6.0`. `[workspace.package] version` in `Cargo.toml` is the Orbit
version. Only `main` releases, and only stable versions.

> [!IMPORTANT]
> A push to `main` that changes the version in `Cargo.toml` releases that version, unless its tag exists already.

## Create a release

1. Describe each change in a change file with `scripts/change.sh`. The files become the release notes, see
   [Change files](#change-files)
2. Check the entries of the next release:

    ```sh
    scripts/change.sh --notes
    ```

3. Select the version. The script commits it and asks to push:

    ```sh
    scripts/release.sh
    ```

4. The push runs `.github/workflows/image.yml`:
    - it builds and smoke-tests the image, then tags it `<version>`, `<major>.<minor>` and `latest` on ghcr.io
    - it creates the GitHub release `v<version>` with the binaries, `SHA256SUMS` and the notes of the version's section
      in `CHANGELOG.md`
    - it sets the version in `https://cdn.coollabs.io/orbit/version.json` with `scripts/sync-cdn.sh`, so self-hosted
      servers show `Update to v<version>`
    - like every push to `main`, it deploys Orbit Cloud

> [!TIP]
> To take the release commit back before the push:
>
> ```sh
> scripts/undo-release.sh
> ```

When a release run fails, fix the cause and run the `Container image` workflow on `main` by hand
(`workflow_dispatch`). It releases the version in `Cargo.toml` when its tag does not exist yet.

## Change files

Every change that users can see has a file in `.changes/`, `AGENTS.md` describes the types and the wording:

```markdown
---
type: fixed
---
Lost unsaved comment when a task closed

- The draft stays until the comment is sent
```

- `scripts/change.sh` writes a file, it asks for what its flags leave out, and `--notes` prints the entries of the
  next release
- `scripts/release.sh` preselects the version that the files need (a breaking change needs a minor release before 1.0),
  writes them under `## [v<version>] - <date>` in `CHANGELOG.md` and removes them
- `scripts/undo-release.sh` brings the files back

## CDN

`scripts/sync-cdn.sh <version>` pushes `json/orbit/version.json` to `main` of `coollabsio/coollabs-cdn`, which
deploys `https://cdn.coollabs.io/orbit/version.json`. It refuses a version older than the one on the CDN.

- The release workflow reads `CDN_TOKEN` from the repository secrets: a fine-grained token for
  `coollabsio/coollabs-cdn` alone, with read and write access to contents
- Cloudflare keeps the file for up to 10 minutes. A server asks the CDN at most once a day
- To run it by hand, sign in with `gh auth login` and run `gh auth setup-git` first
