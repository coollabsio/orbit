#!/usr/bin/env bash
# Takes back the release commit of scripts/release.sh before it is pushed.
set -euo pipefail

OPTION="$*"
CHANGE_DIRECTORY=.changes
RELEASE_FILES=(CHANGELOG.md "$CHANGE_DIRECTORY" Cargo.toml Cargo.lock apps/web/package.json bun.lock)

BOLD=$'\033[1m' BLUE=$'\033[34m' GREEN=$'\033[32m' RED=$'\033[31m' DIM=$'\033[2m' RESET=$'\033[0m'

heading() { printf '\n  %s%s%s\n' "$BOLD" "$*" "$RESET"; }
info() { printf '  %sℹ%s %s\n' "$BLUE" "$RESET" "$*"; }
ok() { printf '  %s✔%s %s\n' "$GREEN" "$RESET" "$*"; }
die() {
	printf '  %s✖%s %s\n' "$RED" "$RESET" "$*" >&2
	exit 1
}
confirm() {
	local answer
	printf '\n  %s [y/N] ' "$*"
	read -r answer
	[[ "$answer" =~ ^[Yy]$ ]]
}

cd "$(dirname "$0")/.."

main() {
	[ -z "$OPTION" ] || die "'$OPTION' is not an option, expected none"
	check_repository
	heading "Undoing $(git log -1 --format='%h %s')"
	info "resets the commit and restores ${RELEASE_FILES[*]}"
	confirm "Continue?" || die "aborted, nothing changed"
	git reset --quiet --soft HEAD~1
	git restore --staged --worktree -- "${RELEASE_FILES[@]}"
	ok "reset to $DIM$(git rev-parse --short HEAD)$RESET $(git log -1 --format=%s)"
	ok "the version is back to $BOLD$(bash scripts/version.sh)$RESET, the change files are back"
}

check_repository() {
	local branch excludes=() file
	branch="$(git symbolic-ref --quiet --short HEAD)" || die "HEAD is detached, expected main"
	[[ "$(git log -1 --format=%s)" == "chore(release): "* ]] || die "the last commit is not a release commit"
	for file in "${RELEASE_FILES[@]}"; do excludes+=(":(exclude)$file"); done
	git diff --quiet HEAD~1 HEAD -- . "${excludes[@]}" || die "the last commit changes more than ${RELEASE_FILES[*]}, expected a release commit"
	[ -z "$(git status --porcelain -- "${RELEASE_FILES[@]}")" ] || die "${RELEASE_FILES[*]} have uncommitted changes, commit or stash them first"
	git fetch --quiet origin || die "could not fetch origin"
	[ "$(git rev-list --count "origin/$branch..HEAD")" -ge 1 ] || die "the release commit is on origin/$branch already, nothing to undo locally"
}

main
