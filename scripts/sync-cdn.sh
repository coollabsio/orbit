#!/usr/bin/env bash
# Sets the latest stable Orbit version on the coolLabs CDN: json/orbit/version.json in coollabsio/coollabs-cdn,
# served at https://cdn.coollabs.io/orbit/version.json. Self-hosted servers read it to show "Update available".
# The release workflow runs it after a stable release; run it by hand with a version to fix the CDN.
set -euo pipefail

VERSION="${1:-}"
CDN_REPOSITORY=coollabsio/coollabs-cdn
VERSION_FILE=json/orbit/version.json
WORK_DIR="$(mktemp -d)"

GREEN=$'\033[32m' RED=$'\033[31m' RESET=$'\033[0m'
ok() { printf '  %s✔%s %s\n' "$GREEN" "$RESET" "$*"; }
die() {
	printf '  %s✖%s %s\n' "$RED" "$RESET" "$*" >&2
	exit 1
}

trap 'rm -rf "$WORK_DIR"' EXIT

[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "'$VERSION' is not a stable version such as 1.2.3"
gh repo clone "$CDN_REPOSITORY" "$WORK_DIR" -- --quiet --depth 1
mkdir -p "$WORK_DIR/$(dirname "$VERSION_FILE")"
previous="$(jq -r '.version // empty' "$WORK_DIR/$VERSION_FILE" 2> /dev/null || true)"
if [ -n "$previous" ] && [ "$previous" != "$VERSION" ] \
	&& [ "$(printf '%s\n' "$previous" "$VERSION" | sort -V | tail -n 1)" = "$previous" ]; then
	die "the CDN has Orbit $previous, newer than $VERSION"
fi
jq -n --arg version "$VERSION" '{version: $version}' > "$WORK_DIR/$VERSION_FILE"
git -C "$WORK_DIR" add "$VERSION_FILE"
if git -C "$WORK_DIR" diff --cached --quiet; then
	ok "$VERSION_FILE on $CDN_REPOSITORY has Orbit $VERSION already"
	exit 0
fi
git -C "$WORK_DIR" commit --quiet --message "chore: sync Orbit ${previous:+from v$previous }to v$VERSION"
git -C "$WORK_DIR" push --quiet origin HEAD:main
ok "pushed Orbit $VERSION to $VERSION_FILE on $CDN_REPOSITORY"
